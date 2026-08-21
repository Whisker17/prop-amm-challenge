use std::panic::{catch_unwind, AssertUnwindSafe};
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
    // path's compile timing) — never for a committed run, so `bench fit` requires
    // --no-report whenever this is set (docs/DESIGN.md §2.5's 300-point budget stays the
    // only value that ever produces committed evidence).
    /// Override the search budget for a quick, uncommitted check (must be paired with
    /// `--no-report`) — independent of `config/bench.toml`'s `[search] max_points`, only
    /// the protocol's own 1..=300 range.
    #[arg(long)]
    max_points: Option<usize>,
    /// Skip writing a `results/` snapshot — required alongside `--max-points`, since a
    /// bounded run doesn't represent the protocol's full search budget.
    #[arg(long)]
    no_report: bool,
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

/// Runs `configs` against `loaded`, catching a shape-check panic (docs/DESIGN.md §2.5,
/// WHI-1213) instead of letting it abort the process. `crates/sim/src/curve_checks.rs`
/// panics from inside a rayon worker spawned by `run_batch`'s own pool — `crates/sim` is
/// upstream-owned (§3.2), so this is the only place the behavior can be fixed. The panic
/// hook is suppressed for the duration of the call: this is an *expected*, handled outcome
/// for parameter sub-regions three of M1's five families have by construction, not a bug
/// worth a backtrace on every invalid point across a 300-point search. A caught panic from
/// any other cause inside `run_batch` is indistinguishable from a shape-check one and is
/// reported the same way — `curve_checks.rs` is the only known panic site reachable from
/// submission code today.
///
/// `set_hook`/`take_hook` are process-global, not scoped to this call's thread — under
/// `cargo test`'s default parallel execution an unrelated test panicking in this exact
/// window would also lose its backtrace. Accepted, not fixed: stable `std` has no
/// thread-scoped panic hook, and `bench fit` itself is single-threaded at this call site
/// (the rayon workers `run_batch` spawns are all inside this one call), so the real CLI
/// path never actually hits the shared-process case this could affect.
fn run_batch_catching_panics(
    loaded: &fast_compile::LoadedFast,
    configs: Vec<SimulationConfig>,
) -> anyhow::Result<search::PointOutcome> {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = catch_unwind(AssertUnwindSafe(|| loaded.run_batch(configs)));
    std::panic::set_hook(previous_hook);

    match result {
        Ok(Ok(batch)) => Ok(search::PointOutcome::Valid(batch.avg_edge())),
        Ok(Err(e)) => Err(e),
        Err(payload) => Ok(search::PointOutcome::Invalid(panic_message(&payload))),
    }
}

/// Extracts a human-readable message from a caught panic payload — `curve_checks.rs`'s own
/// `panic!("submission shape violation during {context}: {message}")` is always a `&str` or
/// `String`, so this recovers it verbatim; anything else falls back to a fixed string
/// rather than failing to produce an `Invalid` outcome at all.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panicked with a non-string payload".to_string()
    }
}

/// A point's edge (or lack of one) for println!/report text — shared by the search
/// closure's own logging and the train/validation re-evaluation below.
fn describe_outcome(outcome: &search::PointOutcome) -> String {
    match outcome {
        search::PointOutcome::Valid(edge) => format!("avg edge {edge:.6}"),
        search::PointOutcome::Invalid(reason) => {
            format!("INVALID (caught shape-check panic) — {reason}")
        }
    }
}

/// Same as [`describe_outcome`], but for the `validation` re-evaluation, which is skipped
/// entirely (rather than run and found invalid) once `train` has already panicked.
fn describe_optional_outcome(outcome: Option<&search::PointOutcome>) -> String {
    match outcome {
        Some(outcome) => describe_outcome(outcome),
        None => "SKIPPED (winner already invalid on `train`)".to_string(),
    }
}

/// The `results/` snapshot's "Invalid points" section (docs/DESIGN.md §2.5/WHI-1213): every
/// panicked point, by parameter vector and panic message — or an explicit "none" body, so
/// an empty search-time panic history is a stated fact in the report rather than a missing
/// heading a reader might mistake for "this run never checked".
fn invalid_points_section(invalid: &[(Vec<i128>, String)]) -> ReportSection {
    ReportSection {
        heading: "Invalid points".to_string(),
        body: if invalid.is_empty() {
            "None — every evaluated point produced a valid edge.\n".to_string()
        } else {
            invalid
                .iter()
                .map(|(point, reason)| format!("- {point:?} -> INVALID: {reason}\n"))
                .collect::<String>()
        },
    }
}

/// The single-peak self-check's verdict (docs/DESIGN.md §2.8, WHI-1213). `Inconclusive` is
/// distinct from `Pass`: with fewer than two valid points there is nothing to compare, and
/// reporting that as a vacuous `PASS` (an empty/singleton curve trivially has no detected
/// decrease-then-increase) would misrepresent an untested curve as a verified one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelfCheckVerdict {
    Inconclusive,
    Pass,
    Fail,
}

fn classify_self_check(points: &[(i128, f64)], tolerance: f64) -> SelfCheckVerdict {
    if points.len() < 2 {
        SelfCheckVerdict::Inconclusive
    } else if search::is_unimodal_1d(points, tolerance) {
        SelfCheckVerdict::Pass
    } else {
        SelfCheckVerdict::Fail
    }
}

/// Every self-check verdict reports under the same heading — only `body` differs.
fn self_check_report_section(body: String) -> ReportSection {
    ReportSection {
        heading: "Single-peaked self-check".to_string(),
        body,
    }
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
    if let Some(n) = args.max_points {
        search::validate_budget(n, "--max-points")?;
    }
    validate_max_points_requires_no_report(args.max_points, args.no_report)?;
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
    let budget = args
        .max_points
        .unwrap_or_else(|| bench_config.search_max_points());

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
        let point_outcome = run_batch_catching_panics(&loaded, screening_configs.clone())?;
        println!("  {values:?} -> {}", describe_outcome(&point_outcome));
        Ok(point_outcome)
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
            "- Cap: {budget}\n- Spent: {}\n- Invalid: {}\n- Stopped: {}\n",
            outcome.points_evaluated,
            outcome.invalid.len(),
            if outcome.budget_exhausted {
                "budget exhausted"
            } else {
                "converged"
            },
        ),
    };
    // Docs/DESIGN.md §2.5/WHI-1213: every point that panicked during simulation, listed by
    // parameter vector and panic message — built once and reused by both the early-bail
    // report (single-peak failure or invalid winner) and the success-path report below.
    let invalid_section = invalid_points_section(&outcome.invalid);
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
        match classify_self_check(&points, tolerance) {
            SelfCheckVerdict::Fail => {
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
                let failure_section = self_check_report_section(format!(
                    "FAILED (docs/DESIGN.md §2.8) at tolerance {tolerance:.6} — the \
                     `{}`<->edge response is not single-peaked. This blocks the issue: \
                     investigate the bench mechanism before trusting any fitted point from \
                     this search. Train/validation re-evaluation was skipped.\n",
                    specs[0].name
                ));
                let sections = vec![
                    frozen_space_section,
                    budget_section,
                    invalid_section,
                    compile_timing_section,
                    failure_section,
                    curve_section,
                ];
                let evidence_note = if args.no_report {
                    "Curve and search budget were not written (--no-report).".to_string()
                } else {
                    let path =
                        report::write_report(Path::new(DEFAULT_REPORT_DIR), &meta, &sections)?;
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
            SelfCheckVerdict::Inconclusive => {
                let detail = format!(
                    "only {} of {} evaluated point(s) produced a valid edge ({} invalid), so \
                     there is no curve to check for single-peakedness",
                    points.len(),
                    outcome.points_evaluated,
                    outcome.invalid.len(),
                );
                println!(
                    "Single-peaked self-check: INCONCLUSIVE — {detail}. The winner below is \
                     unvalidated by this check."
                );
                self_check_section = Some(self_check_report_section(format!(
                    "INCONCLUSIVE (docs/DESIGN.md §2.8/WHI-1213) — {detail}. The winner below \
                     is unvalidated by this check.\n"
                )));
            }
            SelfCheckVerdict::Pass => {
                println!("Single-peaked self-check: PASS (tolerance {tolerance:.6}).");
                self_check_section = Some(self_check_report_section(format!(
                    "PASS (docs/DESIGN.md §2.8) at tolerance {tolerance:.6} — the `{}`<->edge \
                     response over the searched range is single-peaked.\n",
                    specs[0].name
                )));
            }
        }
    }

    // Final point evaluation (docs/DESIGN.md §2.5): full train, then validation.
    let winner_source = params::rewrite_params(&source, &outcome.best)?;
    let winner_safe = fast_compile::make_safe_source(&winner_source)?;

    println!(
        "Re-evaluating the winner on full `train` ({} sims)...",
        train.seeds().len()
    );
    let train_configs = train.sim_configs(&base);
    let train_outcome = {
        let loaded = compile_timed(&winner_safe, &mut timings)?;
        run_batch_catching_panics(&loaded, train_configs)?
    };
    let train_invalid = matches!(train_outcome, search::PointOutcome::Invalid(_));

    // A point already known invalid on `train` doesn't need confirming on `validation` too
    // (docs/DESIGN.md §2.5/WHI-1213) — same "don't waste compute confirming a point already
    // not trusted" reasoning as the single-peak failure path skipping train/validation
    // entirely.
    let validation_outcome = if train_invalid {
        println!(
            "Skipping full `validation` re-evaluation — the winner already panicked on \
             `train`."
        );
        None
    } else {
        println!(
            "Re-evaluating the winner on full `validation` ({} sims)...",
            validation.seeds().len()
        );
        let validation_configs = validation.sim_configs(&base);
        let loaded = compile_timed(&winner_safe, &mut timings)?;
        Some(run_batch_catching_panics(&loaded, validation_configs)?)
    };

    println!(
        "Train: {}  Validation: {}",
        describe_outcome(&train_outcome),
        describe_optional_outcome(validation_outcome.as_ref()),
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
            "- {param_summary}\n- Screening avg edge: {:.6}\n- Train (1000 sims): {}\n- \
             Validation (1000 sims): {}\n",
            outcome.best_edge,
            describe_outcome(&train_outcome),
            describe_optional_outcome(validation_outcome.as_ref()),
        ),
    };
    // Recompute the compile-timing section now that it also covers the final-evaluation
    // build(s) — one or two, depending on whether validation ran — not just the search
    // phase.
    let compile_timing_section = ReportSection {
        heading: "Fast-path compile timing".to_string(),
        body: format!(
            "- {}\n- (search phase plus the two final train/validation builds) Every sample \
             is a `cargo build` invocation against the single, reused `.build/fast/` \
             directory.\n",
            timings.summary()
        ),
    };
    let mut sections = vec![
        frozen_space_section,
        budget_section,
        invalid_section,
        compile_timing_section,
    ];
    sections.extend(self_check_section);
    sections.push(winning_point_section);
    sections.push(curve_section);
    if args.no_report {
        println!("Report not written (--no-report).");
    } else {
        let path = report::write_report(Path::new(DEFAULT_REPORT_DIR), &meta, &sections)?;
        println!("Report written to {}", path.display());
    }

    // docs/DESIGN.md §2.5/WHI-1213: a point valid on `screening`'s seeds is not guaranteed
    // valid on a different, larger seed set (Orbic's own quantization jitter is exactly
    // this — probabilistic across seeds, not just across parameter values). The evidence
    // above is still written either way; this blocks the ranking the same way a failed
    // single-peak check does, rather than reporting a trusted number for a point that just
    // panicked.
    if train_invalid || matches!(validation_outcome, Some(search::PointOutcome::Invalid(_))) {
        anyhow::bail!(
            "the search winner {:?} panicked during full train/validation re-evaluation — \
             valid on `screening`'s seeds does not guarantee valid on a different, larger \
             seed set (docs/DESIGN.md §2.5/WHI-1213); do not trust this point without \
             investigating why. This is also exactly the kind of gap a fuzz-coverage gate \
             (WHI-1212) exists to catch earlier, before a search ever runs.",
            outcome.best,
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use prop_amm_shared::config::HyperparameterVariance;

    /// A fixture strategy for WHI-1213: `MODE == 0` is an ordinary safe CPMM payout,
    /// `MODE == 1` deliberately panics `crates/sim`'s runtime shape check on essentially the
    /// first routed order (see the fixture file's own doc comment for why).
    const PANICKING_FIXTURE: &str =
        include_str!("../../tests/fixtures/whi_1213_panicking_point.rs");

    /// `fast_compile`'s shared `.build/fast/` directory and its global loaded-function-
    /// pointer statics are a deliberate single slot ("the search never evaluates more than
    /// one candidate at a time" — `fast_compile.rs`'s own module doc): whichever call last
    /// wrote+loaded a dylib there wins the global pointer, so two `#[test]` fns racing
    /// through `evaluate_fixture_mode` under `cargo test`'s default parallel execution can
    /// silently run the *other* test's compiled function instead of their own. Real `bench
    /// fit` usage never hits this (its search loop is single-threaded and never has two
    /// calls in flight), but more than one test in this file now drives the fixture through
    /// this same shared path, so it needs an explicit guard rather than relying on every
    /// future test author remembering to keep calls sequential by construction.
    static FAST_BUILD_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn small_configs() -> Vec<SimulationConfig> {
        let base = SimulationConfig {
            n_steps: 200,
            ..SimulationConfig::default()
        };
        vec![HyperparameterVariance::default().apply(&base, 1)]
    }

    /// Compiles/loads/runs `PANICKING_FIXTURE` at the given `MODE` value through the same
    /// real pipeline `run()`'s own search closure uses (`rewrite_params` ->
    /// `make_safe_source` -> `compile_and_load_fast` -> `run_batch_catching_panics`) — used
    /// by every test below so the pipeline itself is written once, not once per test.
    /// Serialized by [`FAST_BUILD_TEST_LOCK`] against every other caller.
    fn evaluate_fixture_mode(mode: i128) -> search::PointOutcome {
        let _guard = FAST_BUILD_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let rewritten = params::rewrite_params(PANICKING_FIXTURE, &[mode]).unwrap();
        let safe_source = fast_compile::make_safe_source(&rewritten).unwrap();
        let loaded = fast_compile::compile_and_load_fast(&safe_source).unwrap();
        run_batch_catching_panics(&loaded, small_configs()).unwrap()
    }

    #[test]
    fn a_deliberately_panicking_point_becomes_invalid_and_a_safe_point_stays_valid() {
        let panicking_outcome = evaluate_fixture_mode(1);
        assert!(
            matches!(panicking_outcome, search::PointOutcome::Invalid(_)),
            "expected an Invalid outcome for MODE=1, got {panicking_outcome:?}"
        );

        let safe_outcome = evaluate_fixture_mode(0);
        assert!(
            matches!(safe_outcome, search::PointOutcome::Valid(_)),
            "expected a Valid outcome for MODE=0, got {safe_outcome:?}"
        );
    }

    #[test]
    fn the_real_fixture_driven_through_the_actual_search_loop_does_not_abort_and_reaches_inconclusive(
    ) {
        // Drives `PANICKING_FIXTURE`'s real `MODE: 0..=1` PARAMS block through the actual
        // search protocol (`search::coarse_grid_then_coordinate_descent`), not a synthetic
        // `eval` — the end-to-end path the acceptance criteria describe: "a deliberately
        // panicking parameter point ... does not abort a `bench fit` run." Budget 4 gives
        // the 1-D grid phase exactly enough points-per-dim (`budget.max(2)`) to land on
        // both declared values (0 and 1) without either being truncated away.
        let specs = params::parse_params_block(PANICKING_FIXTURE).unwrap();
        let outcome = search::coarse_grid_then_coordinate_descent(&specs, 4, |values| {
            Ok(evaluate_fixture_mode(values[0]))
        })
        .unwrap();

        assert_eq!(
            outcome.best,
            vec![0],
            "MODE=1 must never win despite panicking"
        );
        assert_eq!(outcome.invalid.len(), 1);
        assert_eq!(outcome.invalid[0].0, vec![1]);
        assert_eq!(outcome.history.len(), 1);
        assert_eq!(outcome.history[0].0, vec![0]);

        // Only one valid point was ever evaluated (MODE=1 panicked every time it was
        // tried), so the single-peak self-check on the resulting curve must be
        // INCONCLUSIVE, not a vacuous PASS — this is the case docs/DESIGN.md §2.8/WHI-1213's
        // "the single-peak check's behaviour in the presence of invalid points is defined
        // and tested" acceptance criterion is actually about.
        let points: Vec<(i128, f64)> = outcome.history.iter().map(|(p, e)| (p[0], *e)).collect();
        assert_eq!(
            classify_self_check(&points, 1e-4),
            SelfCheckVerdict::Inconclusive
        );
    }

    #[test]
    fn invalid_points_section_states_none_explicitly_when_empty() {
        let section = invalid_points_section(&[]);
        assert_eq!(section.heading, "Invalid points");
        assert!(section.body.contains("None"));
    }

    #[test]
    fn invalid_points_section_lists_every_point_and_its_reason() {
        let invalid = vec![
            (vec![7i128], "point 7 panicked".to_string()),
            (vec![9i128], "point 9 panicked".to_string()),
        ];
        let section = invalid_points_section(&invalid);
        assert!(section.body.contains("[7]"));
        assert!(section.body.contains("point 7 panicked"));
        assert!(section.body.contains("[9]"));
        assert!(section.body.contains("point 9 panicked"));
    }

    #[test]
    fn classify_self_check_is_inconclusive_under_two_points() {
        assert_eq!(
            classify_self_check(&[], 1e-9),
            SelfCheckVerdict::Inconclusive
        );
        assert_eq!(
            classify_self_check(&[(0, 1.0)], 1e-9),
            SelfCheckVerdict::Inconclusive
        );
    }

    #[test]
    fn classify_self_check_delegates_to_is_unimodal_1d_from_two_points_on() {
        let unimodal: Vec<(i128, f64)> = (0i128..=10)
            .map(|x| (x, -((x - 5).pow(2)) as f64))
            .collect();
        assert_eq!(classify_self_check(&unimodal, 1e-9), SelfCheckVerdict::Pass);

        let multi_modal: Vec<(i128, f64)> = (0i128..=10)
            .map(|x| {
                let a = -((x - 2).pow(2)) as f64;
                let b = -((x - 8).pow(2)) as f64;
                (x, a.max(b))
            })
            .collect();
        assert_eq!(
            classify_self_check(&multi_modal, 1e-9),
            SelfCheckVerdict::Fail
        );
    }

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
