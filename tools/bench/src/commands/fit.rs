use std::path::Path;
use std::time::{Duration, Instant};

use clap::Args;
use prop_amm_shared::config::SimulationConfig;

use crate::config::BenchConfig;
use crate::fast_compile;
use crate::params;
use crate::report::{self, ReportMeta, ReportSection, DEFAULT_REPORT_DIR};
use crate::search;

/// `bench fit` — the search protocol (docs/DESIGN.md §2.5): coarse grid then coordinate
/// descent over a strategy's declared PARAMS block, on the fixed `screening` segment (common
/// random numbers), followed by re-evaluating the winner on the full `train` and
/// `validation` segments.
#[derive(Args, Debug)]
pub struct FitArgs {
    /// Path to the strategy directory (e.g. `strategies/001-cpmm-fee`) — must contain
    /// `lib.rs` with a `// === PARAMS BEGIN/END ===` block.
    #[arg(long)]
    strategy: String,
    // WHI-1205: an escape hatch for cheap, uncommitted smoke-testing (e.g. of the fast
    // path's compile timing) — never for a committed run, so `run` requires --no-report
    // whenever this is set (docs/DESIGN.md §2.5's 300-point budget stays the only value
    // that ever produces committed evidence).
    /// Bound this run to fewer search points than the configured budget, for a quick,
    /// uncommitted check (must be paired with `--no-report`). Same 1..=300 range
    /// `config/bench.toml`'s `[search] max_points` is validated against.
    #[arg(long)]
    max_points: Option<usize>,
    /// Skip writing a `results/` snapshot — required alongside `--max-points`, since a
    /// bounded run doesn't represent the protocol's full search budget.
    #[arg(long)]
    no_report: bool,
}

/// Validates a `--max-points` override against the same bound `config/bench.toml`'s
/// `[search] max_points` is checked against — both route through
/// `search::validate_budget`, so the bound lives in one place.
fn validate_max_points(max_points: usize) -> anyhow::Result<usize> {
    search::validate_budget(max_points, "--max-points")?;
    Ok(max_points)
}

/// `--max-points` is only for uncommitted smoke-testing — docs/DESIGN.md §2.5's 300-point
/// budget is what a `results/` snapshot is supposed to represent, so a bounded run must
/// never produce one.
fn validate_max_points_requires_no_report(
    max_points: Option<usize>,
    no_report: bool,
) -> anyhow::Result<()> {
    if max_points.is_some() && !no_report {
        anyhow::bail!(
            "--max-points requires --no-report (a bounded run is never committed evidence)"
        );
    }
    Ok(())
}

/// The acceptance criterion this module measures against: "compiles a parameter point in
/// < 1s on a warm build directory" (WHI-1194's own wording). A per-point claim — checked
/// here as "every warm sample's compile time is under this", not just the mean.
const WARM_COMPILE_TARGET_SECS: f64 = 1.0;

/// Per-point compile timings collected across the whole run — the evidence for
/// [`WARM_COMPILE_TARGET_SECS`], rather than something left to be eyeballed from cargo's own
/// stderr. `cold_start`, checked once via `fast_compile::fast_build_dir_is_warm()` **before**
/// the first compile of a run, is a fact about `.build/fast/`'s prior state, not a guess from
/// a sample's position — if the directory was already warm, even the first sample is a fair
/// warm-compile measurement.
struct CompileTimings {
    samples: Vec<Duration>,
    cold_start: bool,
}

impl CompileTimings {
    fn new(cold_start: bool) -> Self {
        Self {
            samples: Vec::new(),
            cold_start,
        }
    }

    fn record(&mut self, d: Duration) {
        self.samples.push(d);
    }

    /// Samples that are fair warm-compile measurements: every sample if the directory was
    /// already warm before this run, otherwise every sample after the first (which paid the
    /// one-time cost of compiling `pinocchio`/`wincode`/`prop-amm-submission-sdk`).
    fn warm_samples(&self) -> &[Duration] {
        if self.cold_start && !self.samples.is_empty() {
            &self.samples[1..]
        } else {
            &self.samples
        }
    }

    fn summary(&self) -> String {
        let warm = self.warm_samples();
        let cold_note = if self.cold_start {
            match self.samples.first() {
                Some(first) => format!(
                    "cold start (one-time dependency build): {:.3}s; ",
                    first.as_secs_f64()
                ),
                None => String::new(),
            }
        } else {
            String::new()
        };

        if warm.is_empty() {
            return format!("{cold_note}no warm samples recorded");
        }

        let min = warm.iter().min().unwrap();
        let max = warm.iter().max().unwrap();
        let mean = warm.iter().sum::<Duration>() / warm.len() as u32;
        let verdict = if max.as_secs_f64() < WARM_COMPILE_TARGET_SECS {
            format!("MEETS the <{WARM_COMPILE_TARGET_SECS:.0}s target (max warm sample below it)")
        } else {
            format!(
                "EXCEEDS the <{WARM_COMPILE_TARGET_SECS:.0}s target (max warm sample \
                 {:.3}s) — see NOTES.md for whether this reflects system contention rather \
                 than the fast path itself",
                max.as_secs_f64()
            )
        };
        format!(
            "{cold_note}{} warm compiles: min={:.3}s, mean={:.3}s, max={:.3}s — {verdict}",
            warm.len(),
            min.as_secs_f64(),
            mean.as_secs_f64(),
            max.as_secs_f64(),
        )
    }
}

/// Builds `safe_source` through the fast path, recording the build-and-load wall time —
/// `compile_and_load_fast`'s `cargo build` call **plus** locating and `dlopen`-ing the
/// resulting dylib (a fresh tempfile copy each time, `fast_compile::load_fast`), not just
/// the build (WHI-1205 measured `dlopen`+copy at roughly 1-2x the build itself — real, not
/// previously accounted for separately) and not the simulation that follows.
fn compile_timed(
    safe_source: &str,
    timings: &mut CompileTimings,
) -> anyhow::Result<fast_compile::LoadedFast> {
    let start = Instant::now();
    let loaded = fast_compile::compile_and_load_fast(safe_source)?;
    timings.record(start.elapsed());
    Ok(loaded)
}

pub fn run(args: FitArgs) -> anyhow::Result<()> {
    let strategy_dir = Path::new(&args.strategy);
    let slug = strategy_dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "`--strategy` must be a directory path, got `{}`",
                args.strategy
            )
        })?;
    let stage = format!("fit-{slug}");

    // Fail fast, before any compiling/simulating: a bad --max-points, --max-points without
    // its required --no-report (a bounded run must never produce committed evidence at a
    // non-protocol budget — docs/DESIGN.md §2.5's 300-point budget is what `results/`
    // reports represent), or (unless --no-report) an already-taken report slot.
    let max_points = args.max_points.map(validate_max_points).transpose()?;
    validate_max_points_requires_no_report(max_points, args.no_report)?;
    if !args.no_report {
        report::ensure_report_slot_free(Path::new(DEFAULT_REPORT_DIR), &stage)?;
    }

    let bench_config = BenchConfig::load_default()?;
    // Fixed, not user-selectable: the search inner loop always runs on `screening` (common
    // random numbers), and the final point is always re-evaluated on the full `train` and
    // `validation` segments (docs/DESIGN.md §2.5) — `bench fit` has no `--segment` flag.
    let screening = bench_config.segment("screening")?;
    let train = bench_config.segment("train")?;
    let validation = bench_config.segment("validation")?;
    let budget = max_points.unwrap_or_else(|| bench_config.search_max_points());

    let lib_path = strategy_dir.join("lib.rs");
    let source = std::fs::read_to_string(&lib_path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", lib_path.display()))?;
    let specs = params::parse_params_block(&source)?;

    println!(
        "Searching {} parameter(s) over budget {budget} on segment `screening` ({} seeds)...",
        specs.len(),
        screening.seeds().len(),
    );
    for spec in &specs {
        println!("  {} ({}): {}..={}", spec.name, spec.ty, spec.min, spec.max);
    }

    let base = SimulationConfig::default();
    let screening_configs = screening.sim_configs(&base);

    let mut timings = CompileTimings::new(!fast_compile::fast_build_dir_is_warm());
    let outcome = search::coarse_grid_then_coordinate_descent(&specs, budget, |values| {
        let rewritten = params::rewrite_params(&source, values)?;
        let safe_source = fast_compile::make_safe_source(&rewritten)?;
        let loaded = compile_timed(&safe_source, &mut timings)?;
        let batch = loaded.run_batch(screening_configs.clone())?;
        let edge = batch.avg_edge();
        println!("  {values:?} -> avg edge {edge:.6}");
        Ok(edge)
    })?;

    if outcome.budget_exhausted {
        println!(
            "Search stopped at the {budget}-evaluation-point budget without full \
             coordinate-descent convergence (docs/DESIGN.md §2.5's hard cap)."
        );
    } else {
        println!(
            "Search converged after {} of {budget} points.",
            outcome.points_evaluated
        );
    }
    println!(
        "Winner: {:?} -> screening avg edge {:.6}",
        outcome.best, outcome.best_edge
    );
    println!(
        "Fast-path compile timings (search phase): {}",
        timings.summary()
    );

    let frozen_space_section = ReportSection {
        heading: "Frozen parameter space".to_string(),
        body: specs
            .iter()
            .map(|s| format!("- `{}` ({}): {}..={}\n", s.name, s.ty, s.min, s.max))
            .collect::<String>(),
    };
    let budget_section = ReportSection {
        heading: "Search budget".to_string(),
        body: format!(
            "- Cap: {budget}\n- Spent: {}\n- Stopped: {}\n",
            outcome.points_evaluated,
            if outcome.budget_exhausted {
                "budget exhausted"
            } else {
                "converged"
            },
        ),
    };
    let compile_timing_section = ReportSection {
        heading: "Fast-path compile timing".to_string(),
        body: format!(
            "- {}\n- Every sample is a `cargo build` invocation against the single, reused \
             `.build/fast/` directory (search phase only).\n",
            timings.summary()
        ),
    };

    let mut curve_section = ReportSection {
        heading: "Evaluated curve".to_string(),
        body: outcome
            .history
            .iter()
            .map(|(point, edge)| format!("- {point:?} -> {edge:.6}\n"))
            .collect::<String>(),
    };

    let mut self_check_section: Option<ReportSection> = None;
    if specs.len() == 1 {
        let points: Vec<(i128, f64)> = outcome
            .history
            .iter()
            .map(|(point, edge)| (point[0], *edge))
            .collect();
        let mut sorted = points.clone();
        sorted.sort_by_key(|(p, _)| *p);
        let mut body = format!("| {} | avg edge (screening) |\n|---|---|\n", specs[0].name);
        for (p, edge) in &sorted {
            body.push_str(&format!("| {p} | {edge:.6} |\n"));
        }
        curve_section.body = body;

        let scale = points.iter().map(|(_, e)| e.abs()).fold(1.0_f64, f64::max);
        let tolerance = scale * 1e-4;
        if !search::is_unimodal_1d(&points, tolerance) {
            // Write what was measured before bailing — a multi-modal result "blocks this
            // issue rather than being reported as a finding" (WHI-1194), but blocking must
            // not mean losing the evidence that triggered the block. Train/validation are
            // skipped: they'd waste compute confirming a point the self-check already says
            // not to trust.
            let meta = ReportMeta {
                stage: stage.clone(),
                segment: "screening".to_string(),
                n_sims: screening.seeds().len(),
                n_steps: base.n_steps,
                execution_path: "native (fast path)".to_string(),
            };
            let failure_section = ReportSection {
                heading: "Single-peaked self-check".to_string(),
                body: format!(
                    "FAILED (docs/DESIGN.md §2.8) at tolerance {tolerance:.6} — the \
                     `{}`<->edge response is not single-peaked. This blocks the issue: \
                     investigate the bench mechanism before trusting any fitted point from \
                     this search. Train/validation re-evaluation was skipped.\n",
                    specs[0].name
                ),
            };
            let sections = vec![
                frozen_space_section,
                budget_section,
                compile_timing_section,
                failure_section,
                curve_section,
            ];
            let evidence_note = if args.no_report {
                "Curve and search budget were not written (--no-report).".to_string()
            } else {
                let path = report::write_report(Path::new(DEFAULT_REPORT_DIR), &meta, &sections)?;
                format!("Curve and search budget written to {}.", path.display())
            };
            anyhow::bail!(
                "the `{}`<->edge response is not single-peaked (docs/DESIGN.md §2.8's \
                 self-check, tolerance {tolerance:.6}) — this blocks the issue; investigate \
                 the bench mechanism before trusting any fitted point from this search. \
                 {evidence_note}",
                specs[0].name,
            );
        }
        println!("Single-peaked self-check: PASS (tolerance {tolerance:.6}).");
        self_check_section = Some(ReportSection {
            heading: "Single-peaked self-check".to_string(),
            body: format!(
                "PASS (docs/DESIGN.md §2.8) at tolerance {tolerance:.6} — the `{}`<->edge \
                 response over the searched range is single-peaked.\n",
                specs[0].name
            ),
        });
    }

    // Final point evaluation (docs/DESIGN.md §2.5): full train, then validation.
    let winner_source = params::rewrite_params(&source, &outcome.best)?;
    let winner_safe = fast_compile::make_safe_source(&winner_source)?;

    println!(
        "Re-evaluating the winner on full `train` ({} sims)...",
        train.seeds().len()
    );
    let train_configs = train.sim_configs(&base);
    let train_batch = {
        let loaded = compile_timed(&winner_safe, &mut timings)?;
        loaded.run_batch(train_configs)?
    };

    println!(
        "Re-evaluating the winner on full `validation` ({} sims)...",
        validation.seeds().len()
    );
    let validation_configs = validation.sim_configs(&base);
    let validation_batch = {
        let loaded = compile_timed(&winner_safe, &mut timings)?;
        loaded.run_batch(validation_configs)?
    };

    println!(
        "Train avg edge: {:.4}  Validation avg edge: {:.4}",
        train_batch.avg_edge(),
        validation_batch.avg_edge()
    );

    let param_summary = specs
        .iter()
        .zip(&outcome.best)
        .map(|(spec, value)| format!("{} = {value}", spec.name))
        .collect::<Vec<_>>()
        .join(", ");

    let meta = ReportMeta {
        stage: stage.clone(),
        segment: "screening".to_string(),
        n_sims: screening.seeds().len(),
        n_steps: base.n_steps,
        execution_path: "native (fast path)".to_string(),
    };
    let winning_point_section = ReportSection {
        heading: "Winning point".to_string(),
        body: format!(
            "- {param_summary}\n- Screening avg edge: {:.6}\n- Train avg edge (1000 sims): {:.6}\n- Validation avg edge (1000 sims): {:.6}\n",
            outcome.best_edge,
            train_batch.avg_edge(),
            validation_batch.avg_edge(),
        ),
    };
    // Recompute the compile-timing section now that it also covers the two final-evaluation
    // builds, not just the search phase.
    let compile_timing_section = ReportSection {
        heading: "Fast-path compile timing".to_string(),
        body: format!(
            "- {}\n- (search phase plus the two final train/validation builds) Every sample \
             is a `cargo build` invocation against the single, reused `.build/fast/` \
             directory.\n",
            timings.summary()
        ),
    };
    let mut sections = vec![frozen_space_section, budget_section, compile_timing_section];
    sections.extend(self_check_section);
    sections.push(winning_point_section);
    sections.push(curve_section);
    if args.no_report {
        println!("Report not written (--no-report).");
    } else {
        let path = report::write_report(Path::new(DEFAULT_REPORT_DIR), &meta, &sections)?;
        println!("Report written to {}", path.display());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_start_excludes_the_first_sample_from_warm_stats() {
        let mut timings = CompileTimings::new(true);
        timings.record(Duration::from_secs_f64(10.0));
        timings.record(Duration::from_secs_f64(0.5));
        timings.record(Duration::from_secs_f64(0.6));

        assert_eq!(timings.warm_samples().len(), 2);
        let summary = timings.summary();
        assert!(summary.contains("cold start"));
        assert!(summary.contains("MEETS"));
    }

    #[test]
    fn a_warm_start_treats_every_sample_as_warm() {
        let mut timings = CompileTimings::new(false);
        timings.record(Duration::from_secs_f64(0.5));
        timings.record(Duration::from_secs_f64(0.6));

        assert_eq!(timings.warm_samples().len(), 2);
        assert!(!timings.summary().contains("cold start"));
    }

    #[test]
    fn a_slow_warm_sample_reports_exceeds_not_meets() {
        let mut timings = CompileTimings::new(false);
        timings.record(Duration::from_secs_f64(0.5));
        timings.record(Duration::from_secs_f64(1.5));

        let summary = timings.summary();
        assert!(summary.contains("EXCEEDS"), "unexpected summary: {summary}");
    }

    #[test]
    fn no_samples_reports_no_warm_samples() {
        let timings = CompileTimings::new(true);
        assert_eq!(timings.summary(), "no warm samples recorded");
    }

    #[test]
    fn a_single_cold_sample_has_no_warm_samples_to_judge() {
        let mut timings = CompileTimings::new(true);
        timings.record(Duration::from_secs_f64(10.0));

        assert!(timings.warm_samples().is_empty());
        let summary = timings.summary();
        assert!(summary.contains("cold start"));
        assert!(summary.contains("no warm samples recorded"));
    }

    #[test]
    fn zero_max_points_is_rejected() {
        let err = validate_max_points(0).unwrap_err();
        assert!(err.to_string().contains("at least 1"));
    }

    #[test]
    fn max_points_above_the_protocol_cap_is_rejected() {
        let err = validate_max_points(search::MAX_SEARCH_POINTS + 1).unwrap_err();
        assert!(err.to_string().contains("exceeds the protocol's hard cap"));
    }

    #[test]
    fn max_points_at_the_protocol_cap_is_allowed() {
        assert_eq!(
            validate_max_points(search::MAX_SEARCH_POINTS).unwrap(),
            search::MAX_SEARCH_POINTS
        );
    }

    #[test]
    fn max_points_of_one_is_allowed() {
        assert_eq!(validate_max_points(1).unwrap(), 1);
    }

    #[test]
    fn max_points_without_no_report_is_rejected() {
        let err = validate_max_points_requires_no_report(Some(8), false).unwrap_err();
        assert!(err.to_string().contains("requires --no-report"));
    }

    #[test]
    fn max_points_with_no_report_is_allowed() {
        assert!(validate_max_points_requires_no_report(Some(8), true).is_ok());
    }

    #[test]
    fn no_report_without_max_points_is_allowed() {
        assert!(validate_max_points_requires_no_report(None, true).is_ok());
    }

    #[test]
    fn neither_flag_is_allowed() {
        assert!(validate_max_points_requires_no_report(None, false).is_ok());
    }
}
