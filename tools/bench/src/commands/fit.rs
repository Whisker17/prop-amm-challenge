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
}

/// Per-point compile timings collected across the whole run — the evidence for "the fast
/// path compiles a parameter point in < 1s on a warm build directory" (WHI-1194's own
/// acceptance criterion), rather than something left to be eyeballed from cargo's own stderr.
#[derive(Default)]
struct CompileTimings {
    samples: Vec<Duration>,
}

impl CompileTimings {
    fn record(&mut self, d: Duration) {
        self.samples.push(d);
    }

    /// Every sample after the first. If `.build/fast/` was empty before this run, the very
    /// first `cargo build` there also compiles `pinocchio`/`wincode`/`prop-amm-submission-sdk`
    /// from scratch — a one-time cost the "< 1s on a warm build directory" criterion doesn't
    /// describe. Excluding it (rather than folding it into one min/mean/max) is what keeps a
    /// single cold-start sample from making every subsequent, genuinely warm compile look
    /// worse than it is.
    fn warm_samples(&self) -> &[Duration] {
        match self.samples.len() {
            0 => &[],
            _ => &self.samples[1..],
        }
    }

    fn summary(&self) -> String {
        let Some(first) = self.samples.first() else {
            return "no compiles recorded".to_string();
        };
        let warm = self.warm_samples();
        if warm.is_empty() {
            return format!(
                "n=1, first={:.3}s (only sample — nothing to compare it against as \
                 warm-vs-cold)",
                first.as_secs_f64()
            );
        }
        let min = warm.iter().min().unwrap();
        let max = warm.iter().max().unwrap();
        let mean = warm.iter().sum::<Duration>() / warm.len() as u32;
        format!(
            "first={:.3}s (may include a one-time dependency build if `.build/fast/` started \
             empty); remaining {} compiles: min={:.3}s, mean={:.3}s, max={:.3}s",
            first.as_secs_f64(),
            warm.len(),
            min.as_secs_f64(),
            mean.as_secs_f64(),
            max.as_secs_f64(),
        )
    }
}

/// Builds `safe_source` through the fast path, recording the compile-only wall time (the
/// `cargo build` call inside `compile_and_load_fast`, not the simulation that follows).
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

    // Fail fast, before any compiling/simulating, if today's report slot is already taken.
    report::ensure_report_slot_free(Path::new(DEFAULT_REPORT_DIR), &stage)?;

    let bench_config = BenchConfig::load_default()?;
    // Fixed, not user-selectable: the search inner loop always runs on `screening` (common
    // random numbers), and the final point is always re-evaluated on the full `train` and
    // `validation` segments (docs/DESIGN.md §2.5) — `bench fit` has no `--segment` flag.
    let screening = bench_config.segment("screening")?;
    let train = bench_config.segment("train")?;
    let validation = bench_config.segment("validation")?;
    let budget = bench_config.search_max_points();

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

    let mut timings = CompileTimings::default();
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
            "- {}\n- This is the evidence for \"compiles a parameter point in < 1s on a \
             warm build directory\" — every sample above is a `cargo build` invocation \
             against the single, reused `.build/fast/` directory.\n",
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
            let path = report::write_report(Path::new(DEFAULT_REPORT_DIR), &meta, &sections)?;
            anyhow::bail!(
                "the `{}`<->edge response is not single-peaked (docs/DESIGN.md §2.8's \
                 self-check, tolerance {tolerance:.6}) — this blocks the issue; investigate \
                 the bench mechanism before trusting any fitted point from this search. \
                 Curve and search budget written to {}.",
                specs[0].name,
                path.display(),
            );
        }
        println!("Single-peaked self-check: PASS (tolerance {tolerance:.6}).");
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
            "- {} (search phase plus the two final train/validation builds)\n- This is the \
             evidence for \"compiles a parameter point in < 1s on a warm build directory\" \
             — every sample is a `cargo build` invocation against the single, reused \
             `.build/fast/` directory.\n",
            timings.summary()
        ),
    };
    let sections = vec![
        frozen_space_section,
        budget_section,
        compile_timing_section,
        winning_point_section,
        curve_section,
    ];
    let path = report::write_report(Path::new(DEFAULT_REPORT_DIR), &meta, &sections)?;
    println!("Report written to {}", path.display());

    Ok(())
}
