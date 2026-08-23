//! `bench ceiling` — WHI-1247's out-of-competition ceiling lane: how much edge a price
//! re-anchor the arbitrageur cannot front-run can extract, measured as the host-side Orbic
//! oracle curve (`crate::oracle`) against the allowlisted 0-line reference. Nothing this
//! command produces is submittable or ranked (`ceilings/README.md`) — every report this
//! writes says so as its own literal first line (guard (c) below), and every table in it is
//! deliberately shaped differently from `compare.rs`'s own paired-comparison table so the
//! two are never mistaken for each other at a glance.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use clap::{Args, ValueEnum};
use prop_amm_shared::config::SimulationConfig;
use prop_amm_shared::result::BatchResult;

use crate::commands::resolve_strategy_lib_path;
use crate::compile::{self, Slot};
use crate::config::{BenchConfig, Segment, SegmentSelector};
use crate::oracle::{self, OracleParams, OracleVariant, StalenessSummary};
use crate::params::ParamSpec;
use crate::report::DEFAULT_REPORT_DIR;
use crate::search::{self, PointOutcome};
use crate::stats;

/// This issue's only cursor rung (WHI-1247 step 5), and the only value [`CeilingArgs::cursor`]
/// accepts today — there is nothing else to select yet; a future rung (the exact-step
/// cursor / L=1 fixed lag this issue explicitly Blocks) adds its own accepted value, not a
/// retrofit of this one.
const CURSOR_MODE: &str = "trade-triggered";

/// The hardcoded reference allowlist (WHI-1247 step 7 guard (a)): the ceiling lane may only
/// ever be measured against these two strategies — the trusted `000-normalizer` (for
/// `--self-check`) and the committed `001-cpmm-fee` 0-line (for the real ceiling
/// measurement). Deliberately not configurable from the CLI or `config/bench.toml`: an
/// out-of-competition ceiling must never silently drift to compare against a stronger, more
/// recent strategy and be read as if it beat the actual 0-line.
const ALLOWED_REFERENCE_SLUGS: [&str; 2] = ["000-normalizer", "001-cpmm-fee"];

/// WHI-1247 step 3's two `target_x` variants, as a CLI-selectable value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum VariantArg {
    /// The headline variant: `target_x` pinned to the pair's starting `initial_x`.
    Anchored,
    /// The degenerate/diagnostic variant: `target_x` re-read as `reserve_x` on every call.
    /// Never a result on its own (WHI-1247 step 3) — useful only as a contrast against
    /// `Anchored`.
    Floating,
}

impl From<VariantArg> for OracleVariant {
    fn from(v: VariantArg) -> Self {
        match v {
            VariantArg::Anchored => OracleVariant::Anchored,
            VariantArg::Floating => OracleVariant::Floating,
        }
    }
}

impl VariantArg {
    fn slug(self) -> &'static str {
        match self {
            VariantArg::Anchored => "anchored",
            VariantArg::Floating => "floating",
        }
    }
}

#[derive(Args, Debug)]
pub struct CeilingArgs {
    #[command(flatten)]
    pub segment_selector: SegmentSelector,

    /// Which of WHI-1247 step 3's two `target_x` variants to run.
    #[arg(long, value_enum, default_value = "anchored")]
    pub variant: VariantArg,

    /// Which cursor rung to run. Accepted only as an explicit spelling-match against
    /// [`CURSOR_MODE`] (`"trade-triggered"`, the only rung this issue delivers) — present so
    /// the spec's own literal example commands (`ceiling --variant anchored --cursor
    /// trade-triggered ...`) actually work, without pretending there is a real choice here
    /// yet. A future rung (the exact-step cursor / L=1 fixed lag this issue explicitly
    /// Blocks) adds its own accepted value here, not a retrofit of this one.
    #[arg(long, default_value = CURSOR_MODE)]
    pub cursor: String,

    /// The allowlisted 0-line reference to measure against — a `strategies/<slug>`
    /// directory (WHI-1247 step 7 guard (a); ignored by `--self-check`, which always
    /// measures `strategies/000-normalizer`).
    #[arg(long, default_value = "strategies/001-cpmm-fee")]
    pub reference: String,

    /// Runs the loop-parity validation gate instead of a measurement (WHI-1247 step 6):
    /// drives `strategies/000-normalizer` through both the trusted `compile::build_and_load`
    /// path and this lane's own native batch loop, and requires them to agree per seed on
    /// `--segment`'s configs. Writes no report either way.
    #[arg(long)]
    pub self_check: bool,

    /// Fits `(concentration, spread_bps)` — or, for `--variant floating`, `spread_bps`
    /// alone, holding `--concentration` fixed at the value already fit for `anchored` — on
    /// the fixed `screening` segment under `config/bench.toml`'s `[search] max_points`
    /// budget (WHI-1247 step 10), instead of measuring an explicit fixed point.
    #[arg(long)]
    pub fit: bool,

    /// A fixed concentration to measure. Required together with `--spread-bps` when `--fit`
    /// is not given; required (and held fixed) alongside `--fit --variant floating`.
    #[arg(long)]
    pub concentration: Option<f64>,

    /// A fixed spread, in basis points, to measure. Required together with `--concentration`
    /// when `--fit` is not given.
    #[arg(long)]
    pub spread_bps: Option<f64>,

    /// Overrides `config/bench.toml`'s `[search] max_points` for `--fit`, for a quick,
    /// uncommitted check — mirrors `fit.rs`'s own escape hatch and carries the same
    /// requirement: never for a committed run.
    #[arg(long)]
    pub max_points: Option<usize>,

    /// Skip writing a `results/` snapshot — required alongside `--max-points`.
    #[arg(long)]
    pub no_report: bool,
}

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

/// WHI-1247 step 7 guard (a): the reference must resolve (via [`resolve_strategy_lib_path`])
/// to one of [`ALLOWED_REFERENCE_SLUGS`] — anything else is refused before it is ever
/// compiled.
///
/// [`resolve_strategy_lib_path`] derives its `slug` purely from the final path component
/// (`Path::file_name`), with no check that the path it resolved actually lives under
/// `strategies/`. Checking only that string would let `--reference
/// ../anywhere/001-cpmm-fee` (a directory that merely shares a final component with an
/// allowlisted slug, but whose `lib.rs` is really e.g. `008`'s) pass this guard and produce
/// a report headed `Reference (0-line): 001-cpmm-fee` while actually measuring against
/// `008` — exactly the "ceiling vs `008`" report this issue requires to be impossible to
/// generate. So after the slug check, this also requires the resolved `lib_path` to
/// canonicalize to the *real* `strategies/<slug>/lib.rs`, not just share its basename.
fn validate_reference_allowlist(reference: &str) -> anyhow::Result<String> {
    let (slug, lib_path) = resolve_strategy_lib_path(reference)?;
    if !ALLOWED_REFERENCE_SLUGS.contains(&slug.as_str()) {
        anyhow::bail!(
            "`--reference {reference}` (slug `{slug}`) is not on the ceiling lane's \
             allowlist {ALLOWED_REFERENCE_SLUGS:?} (WHI-1247 step 7 guard (a)) — this lane \
             may only ever be measured against the trusted normalizer or the committed \
             0-line, never a stronger or more recent strategy"
        );
    }

    let expected = Path::new("strategies").join(&slug).join("lib.rs");
    let actual_canon = lib_path.canonicalize().map_err(|e| {
        anyhow::anyhow!(
            "failed to resolve `--reference {reference}` ({}): {e}",
            lib_path.display()
        )
    })?;
    let expected_canon = expected.canonicalize().map_err(|e| {
        anyhow::anyhow!(
            "failed to resolve the allowlisted path {} ({e}) — is `strategies/{slug}/lib.rs` \
             missing?",
            expected.display()
        )
    })?;
    if actual_canon != expected_canon {
        anyhow::bail!(
            "`--reference {reference}` (slug `{slug}`) resolves to `{}`, which is not the \
             allowlisted `{}` (WHI-1247 step 7 guard (a)) — a directory that merely shares a \
             final path component with an allowlisted slug is not the same file, and this \
             lane must never be measured against whatever that other directory actually \
             holds",
            actual_canon.display(),
            expected_canon.display()
        );
    }
    Ok(slug)
}

/// WHI-1247 step 7 guard (b): the `test` segment is refused unconditionally — even with
/// `--i-am-spending-the-test-segment` — and *before* `config::BenchConfig::segment` is ever
/// called for anything else, since nothing in this out-of-competition lane has a ranking
/// claim that spending `test` could protect. `SegmentSelector::resolve` cannot be reused
/// here unmodified: it only blocks a `single_use` segment in the *absence* of the spend
/// flag, which would let the flag defeat this guard. The rest of its own single_use logic is
/// reproduced below so a future single-use segment besides `test` is still caught.
fn resolve_ceiling_segment<'s, 'c>(
    selector: &'s SegmentSelector,
    config: &'c BenchConfig,
) -> anyhow::Result<(&'s str, &'c Segment)> {
    if selector.segment == "test" {
        anyhow::bail!(
            "the ceiling lane never spends the `test` segment, with or without \
             --i-am-spending-the-test-segment: nothing out-of-competition here has a \
             ranking claim to protect it for (WHI-1247 step 7 guard (b))"
        );
    }
    let segment = config.segment(&selector.segment)?;
    if segment.single_use && !selector.i_am_spending_the_test_segment {
        anyhow::bail!(
            "segment `{}` is single-use; pass --i-am-spending-the-test-segment to spend it",
            selector.segment
        );
    }
    Ok((selector.segment.as_str(), segment))
}

/// WHI-1247 step 10's frozen, self-chosen parameter space (rationale in
/// `ceilings/C-orbic-oracle/NOTES.md`): `concentration_x100` in `[100, 10_000]` (÷100 ->
/// concentration `1.00..=100.00`) and `spread_bps` in `[0, 1000]` (0%..=10%, direct bps).
fn concentration_spec() -> ParamSpec {
    ParamSpec {
        name: "concentration_x100".to_string(),
        ty: "i128".to_string(),
        min: 100,
        max: 10_000,
        current: 1_000,
        indent: String::new(),
    }
}

fn spread_bps_spec() -> ParamSpec {
    ParamSpec {
        name: "spread_bps".to_string(),
        ty: "i128".to_string(),
        min: 0,
        max: 1_000,
        current: 20,
        indent: String::new(),
    }
}

/// Runs `f`, catching any panic (e.g. a shape-check panic from `crates/sim/src/curve_checks.rs`
/// the same way `commands/fit.rs::run_batch_catching_panics` does — `curve_checks.rs` treats a
/// native submission fn identically to a compiled one, keying only on the AMM's own
/// `"submission"` name, so a pathological corner of the oracle curve's own search space can
/// panic mid-simulation exactly like a compiled candidate's PARAMS point can) into
/// `Ok(Err(panic_message))` rather than propagating it, while suppressing the default panic
/// hook's stderr print for the duration. Shared by every catch-and-continue call site in this
/// file — the search loop's own per-point evaluation ([`run_catching_panics`]) and the final
/// re-evaluation of the fitted point ([`run_final_eval_catching_panics`]) — since both need
/// the exact same `take_hook`/`set_hook`/`catch_unwind` bracket and differ only in what they
/// do with a successful `T`.
fn catch_panicking<T>(
    f: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<Result<T, String>> {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = catch_unwind(AssertUnwindSafe(f));
    std::panic::set_hook(previous_hook);

    match result {
        Ok(Ok(value)) => Ok(Ok(value)),
        Ok(Err(e)) => Err(e),
        Err(payload) => Ok(Err(panic_message(&payload))),
    }
}

fn run_catching_panics(
    f: impl FnOnce() -> anyhow::Result<BatchResult>,
) -> anyhow::Result<PointOutcome> {
    match catch_panicking(f)? {
        Ok(batch) => Ok(PointOutcome::Valid(batch.avg_edge())),
        Err(msg) => Ok(PointOutcome::Invalid(msg)),
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panicked with a non-string payload".to_string()
    }
}

/// The raw outcome of the *final* re-evaluation of a fitted point (as opposed to a point
/// evaluated during the search itself, which goes through [`run_catching_panics`] into a
/// [`PointOutcome`]) — mirrors `commands/fit.rs`'s own train/validation re-evaluation
/// pattern (docs/DESIGN.md §2.4/§2.5, WHI-1213): a point valid on `screening`'s 200 seeds is
/// not guaranteed valid on a different, larger seed set (this is the Orbic family's own
/// "quantization jitter" — probabilistic across seeds, not just across parameter values,
/// `docs/DESIGN.md` §6.2's `002` entry). The search's own `catch_unwind` only ever wraps the
/// screening-segment search loop; without an equivalent guard here, a fitted point that
/// panics on `--segment observation`/`train`/`validation` would abort the whole process
/// with an unhandled panic and no report at all, rather than the established "evidence
/// gathered so far is still written, the point is not entered into a ranking" behavior.
enum OracleBatchOutcome {
    Valid {
        batch: BatchResult,
        staleness: Vec<StalenessSummary>,
    },
    Invalid(String),
}

/// Runs the real oracle measurement, catching a panic via the same [`catch_panicking`] guard
/// [`run_catching_panics`] uses for the search loop — see [`OracleBatchOutcome`] for why this
/// call site needs to keep its own wrapper (it preserves the staleness summaries on success,
/// which `run_catching_panics`'s `PointOutcome`-shaped return throws away).
fn run_final_eval_catching_panics(
    params: OracleParams,
    configs: &[SimulationConfig],
) -> anyhow::Result<OracleBatchOutcome> {
    let owned_configs = configs.to_vec();
    match catch_panicking(move || oracle::run_batch(params, &owned_configs))? {
        Ok((batch, staleness)) => Ok(OracleBatchOutcome::Valid { batch, staleness }),
        Err(msg) => Ok(OracleBatchOutcome::Invalid(msg)),
    }
}

/// What [`write_ceiling_report`] renders for the "## Edge vs the 0-line" and "## Trade-
/// triggered cursor staleness" sections — either the real numbers, or an honest account of
/// why they don't exist (see [`OracleBatchOutcome`] for the panic-catching this comes from).
enum FinalEvalSummary<'a> {
    Valid {
        paired: &'a stats::PairedStat,
        staleness: &'a StalenessAggregate,
    },
    Invalid(&'a str),
}

/// The fitted (or explicitly supplied) point this run measures at.
struct FittedPoint {
    concentration: f64,
    spread_bps: f64,
    /// `None` when the point came straight from `--concentration`/`--spread-bps` rather
    /// than a search.
    search_outcome: Option<search::SearchOutcome>,
}

fn run_fit(
    variant: VariantArg,
    fixed_concentration: Option<f64>,
    screening_configs: &[SimulationConfig],
    budget: usize,
) -> anyhow::Result<FittedPoint> {
    match variant {
        VariantArg::Anchored => {
            if fixed_concentration.is_some() {
                anyhow::bail!(
                    "--fit --variant anchored searches concentration jointly with \
                     spread_bps; --concentration must not be passed alongside it"
                );
            }
            let specs = [concentration_spec(), spread_bps_spec()];
            let outcome = search::coarse_grid_then_coordinate_descent(&specs, budget, |values| {
                let concentration = values[0] as f64 / 100.0;
                let spread_bps = values[1] as f64;
                let params = OracleParams {
                    variant: variant.into(),
                    concentration,
                    spread_bps,
                };
                let configs = screening_configs.to_vec();
                run_catching_panics(move || {
                    oracle::run_batch(params, &configs).map(|(batch, _staleness)| batch)
                })
            })?;
            let concentration = outcome.best[0] as f64 / 100.0;
            let spread_bps = outcome.best[1] as f64;
            Ok(FittedPoint {
                concentration,
                spread_bps,
                search_outcome: Some(outcome),
            })
        }
        VariantArg::Floating => {
            let concentration = fixed_concentration.ok_or_else(|| {
                anyhow::anyhow!(
                    "--fit --variant floating requires --concentration (held fixed — only \
                     spread_bps is re-fit, per WHI-1247 step 10)"
                )
            })?;
            let specs = [spread_bps_spec()];
            let outcome = search::coarse_grid_then_coordinate_descent(&specs, budget, |values| {
                let spread_bps = values[0] as f64;
                let params = OracleParams {
                    variant: variant.into(),
                    concentration,
                    spread_bps,
                };
                let configs = screening_configs.to_vec();
                run_catching_panics(move || {
                    oracle::run_batch(params, &configs).map(|(batch, _staleness)| batch)
                })
            })?;
            let spread_bps = outcome.best[0] as f64;
            Ok(FittedPoint {
                concentration,
                spread_bps,
                search_outcome: Some(outcome),
            })
        }
    }
}

/// The staleness distribution across every simulation in the batch (WHI-1247 step 5),
/// aggregated from each simulation's own [`StalenessSummary`] into one reportable set of
/// numbers: the mean and median of each simulation's own mean staleness, the largest p95
/// seen across simulations, and the largest max seen across simulations. Deliberately
/// aggregates rather than dumping one row per simulation — 1,000 rows would swamp the
/// report — while still surfacing the tail (`p95_of_p95`, `max_of_max`) rather than only an
/// average-of-averages that could hide a badly stale minority of simulations.
struct StalenessAggregate {
    mean_of_means: f64,
    median_of_means: f64,
    p95_of_p95: f64,
    max_of_max: f64,
}

fn aggregate_staleness(summaries: &[StalenessSummary]) -> StalenessAggregate {
    let means: Vec<f64> = summaries.iter().map(|s| s.mean).collect();
    let p95s: Vec<f64> = summaries.iter().map(|s| s.p95).collect();
    let maxes: Vec<f64> = summaries.iter().map(|s| s.max).collect();

    let mean_of_means = if means.is_empty() {
        0.0
    } else {
        means.iter().sum::<f64>() / means.len() as f64
    };
    let median_of_means = stats::median(&means).unwrap_or(0.0);
    let p95_of_p95 = p95s.iter().cloned().fold(0.0, f64::max);
    let max_of_max = maxes.iter().cloned().fold(0.0, f64::max);

    StalenessAggregate {
        mean_of_means,
        median_of_means,
        p95_of_p95,
        max_of_max,
    }
}

/// The run's own identity — every field [`write_ceiling_report`] needs to describe *what*
/// was measured, as opposed to [`FittedPoint`]/[`FinalEvalSummary`] which describe the
/// measurement itself. These six always travel together (one run has exactly one stage, one
/// segment, one sim/step count, one variant, one reference) — bundled into one struct rather
/// than six positional parameters so the call site reads as "the run" and not an
/// order-sensitive tuple, and so `write_ceiling_report` no longer needs
/// `#[allow(clippy::too_many_arguments)]` to satisfy that lint honestly rather than by
/// suppression.
struct CeilingRunMeta<'a> {
    stage: &'a str,
    segment_name: &'a str,
    n_sims: usize,
    n_steps: u32,
    variant: VariantArg,
    reference_slug: &'a str,
}

/// WHI-1247 step 7 guard (c): the report's own machine-readable marker, as the literal first
/// line, plus every field a reader needs to know what was measured and how — reusing only
/// [`crate::report::ensure_report_slot_free`] and [`DEFAULT_REPORT_DIR`] from `report.rs`
/// (never `write_report`/`ReportMeta`, whose own first line is always `# {stage} — {date}`,
/// structurally incompatible with this guard). Tables here are deliberately shaped
/// differently from `compare.rs`'s own `| regime | n | mean diff | 95% CI |` — this is an
/// out-of-competition measurement and must never be mistaken for a ranked comparison at a
/// glance.
fn write_ceiling_report(
    meta: CeilingRunMeta<'_>,
    fitted: &FittedPoint,
    final_eval: FinalEvalSummary<'_>,
) -> anyhow::Result<std::path::PathBuf> {
    let CeilingRunMeta {
        stage,
        segment_name,
        n_sims,
        n_steps,
        variant,
        reference_slug,
    } = meta;

    let dir = Path::new(DEFAULT_REPORT_DIR);
    std::fs::create_dir_all(dir)
        .map_err(|e| anyhow::anyhow!("failed to create report dir {}: {e}", dir.display()))?;
    crate::report::ensure_report_slot_free(dir, stage)?;

    let date = today();
    let path = dir.join(format!("{date}-{stage}.md"));

    let mut out = String::new();
    out.push_str("out_of_competition: true\n\n");
    out.push_str(&format!("# {stage} — {date}\n\n"));
    out.push_str(
        "This is the ceiling lane (WHI-1247, `ceilings/README.md`): an out-of-competition \
         measurement of an oracle re-anchor the arbitrageur cannot front-run. Nothing on this \
         page is submittable or ranked.\n\n",
    );
    out.push_str(
        "**Three honesty constraints bound what this number means** (WHI-1247 § Context): \
         (1) it is a one-sided **lower bound** on what perfect price knowledge is worth — \
         Orbic-with-a-spread is one member of the perfect-information class, not its \
         maximum, so this number does not bound the remaining headroom above a stronger \
         submission from above; (2) once the quote is accurate and spread, the arbitrageur \
         mostly stops trading against it, so most of the number is `retail volume x \
         captured spread x flow share(spread)` — the only genuinely non-closed-form content \
         is the flow-share-vs-spread curve the router grants against the normalizer's own \
         sampled fee/liquidity; (3) that content generalizes to *any* oracle-centered \
         quoter and carries little content specific to the Orbic curve itself.\n\n",
    );
    out.push_str(&format!("- Commit: `{}`\n", commit_sha()));
    out.push_str(&format!("- Segment: `{segment_name}`\n"));
    out.push_str(&format!("- Simulations: {n_sims}\n"));
    out.push_str(&format!("- Steps: {n_steps}\n"));
    out.push_str("- Execution path: native (host-side, never BPF-compiled)\n");
    out.push_str(&format!("- Variant: {}\n", variant.slug()));
    out.push_str(&format!("- Cursor mode: {CURSOR_MODE}\n"));
    out.push_str(&format!("- Reference (0-line): `{reference_slug}`\n"));
    out.push('\n');

    out.push_str("## Fitted point\n\n");
    out.push_str(&format!(
        "- concentration: {:.4}\n- spread_bps: {:.4}\n",
        fitted.concentration, fitted.spread_bps
    ));
    if let Some(outcome) = &fitted.search_outcome {
        out.push_str(&format!(
            "- Search budget spent: {} (exhausted: {})\n- Best avg edge on `screening`: {:.6}\n",
            outcome.points_evaluated, outcome.budget_exhausted, outcome.best_edge
        ));
        if !outcome.invalid.is_empty() {
            out.push_str(&format!(
                "- Invalid points during search: {}\n",
                outcome.invalid.len()
            ));
        }
    } else {
        out.push_str("- Point supplied directly via --concentration/--spread-bps (no search).\n");
    }
    out.push('\n');

    match final_eval {
        FinalEvalSummary::Valid { paired, staleness } => {
            out.push_str("## Edge vs the 0-line\n\n");
            out.push_str("| field | value |\n|---|---|\n");
            out.push_str(&format!("| n | {} |\n", paired.n));
            out.push_str(&format!(
                "| mean edge diff (oracle - reference) | {:.6} |\n",
                paired.mean_diff
            ));
            out.push_str(&format!("| std error | {:.6} |\n", paired.std_error));
            out.push_str(&format!(
                "| 95% interval | [{:.6}, {:.6}] |\n",
                paired.ci_low, paired.ci_high
            ));
            out.push('\n');

            out.push_str(
                "## Trade-triggered cursor staleness (steps since last executed trade)\n\n",
            );
            out.push_str("| aggregate | value |\n|---|---|\n");
            out.push_str(&format!(
                "| mean of per-sim means | {:.3} |\n",
                staleness.mean_of_means
            ));
            out.push_str(&format!(
                "| median of per-sim means | {:.3} |\n",
                staleness.median_of_means
            ));
            out.push_str(&format!(
                "| max of per-sim p95 | {:.3} |\n",
                staleness.p95_of_p95
            ));
            out.push_str(&format!(
                "| max of per-sim max | {:.3} |\n",
                staleness.max_of_max
            ));
            out.push('\n');
        }
        FinalEvalSummary::Invalid(reason) => {
            out.push_str("## Final re-evaluation: INVALID\n\n");
            out.push_str(&format!(
                "The fitted point above was chosen from a search on the `screening` segment, \
                 but re-evaluating it on the full `{segment_name}` segment triggered a caught \
                 panic instead of producing a number. This is the documented failure mode in \
                 `docs/DESIGN.md` §2.4/§2.5 (WHI-1213): a point valid on `screening`'s seeds \
                 is not guaranteed valid on a different, larger seed set — exactly what made \
                 the Orbic family's own \"quantization jitter\" (`docs/DESIGN.md` §6.2, \
                 strategy `002`, WHI-1206, Canceled) probabilistic across seeds, not just \
                 across parameter values. The panic message below is the only evidence of \
                 cause captured; it is not confirmed to be a `crates/sim/src/curve_checks.rs` \
                 shape-check specifically (a non-string panic payload defeated this lane's own \
                 `panic_message` downcast, so the cause is not pinned down further than \
                 \"a panic occurred\"). No edge-vs-0-line or staleness numbers exist for this \
                 run; the absence of a crash on `screening` is not evidence this variant/point \
                 is safe on other segments or seeds.\n\n"
            ));
            out.push_str(&format!("Panic message: `{reason}`\n\n"));
        }
    }

    std::fs::write(&path, out)
        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", path.display()))?;
    Ok(path)
}

fn commit_sha() -> String {
    let sha = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .ok()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);
    if dirty {
        format!("{sha}+dirty")
    } else {
        sha
    }
}

/// Duplicated from `report.rs::today`/`civil_from_days` (both private there): WHI-1247's
/// own constraint set forbids extending `report.rs`'s public surface (see the issue body's
/// "what must not change"), so this report writer duplicates the same days-since-epoch
/// civil-date algorithm rather than editing that module.
/// https://howardhinnant.github.io/date_algorithms.html#civil_from_days
fn today() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn run_self_check(configs: Vec<SimulationConfig>) -> anyhow::Result<()> {
    let normalizer_dir = "strategies/000-normalizer";
    let (_slug, lib_path) = resolve_strategy_lib_path(normalizer_dir)?;
    let lib_path_str = lib_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF8 path {}", lib_path.display()))?;

    let trusted = compile::build_and_load(lib_path_str, Slot::Zero)?.run_batch(configs.clone())?;
    let via_lane = oracle::run_batch_native_loop(
        prop_amm_shared::normalizer::compute_swap,
        Some(prop_amm_shared::normalizer::after_swap),
        &configs,
    )?;

    if trusted.results.len() != via_lane.results.len() {
        anyhow::bail!(
            "self-check produced different sim counts: trusted {} vs lane {}",
            trusted.results.len(),
            via_lane.results.len()
        );
    }

    let tolerance = 1e-6;
    for (a, b) in trusted.results.iter().zip(via_lane.results.iter()) {
        if a.seed != b.seed {
            anyhow::bail!(
                "self-check unpaired: trusted seed {} vs lane seed {}",
                a.seed,
                b.seed
            );
        }
        let diff = (a.submission_edge - b.submission_edge).abs();
        if diff > tolerance {
            anyhow::bail!(
                "self-check FAILED at seed {}: trusted edge {} vs lane edge {} (diff {diff} \
                 exceeds tolerance {tolerance})",
                a.seed,
                a.submission_edge,
                b.submission_edge
            );
        }
    }

    println!(
        "self-check PASSED: {} seeds agree within {tolerance} between the trusted \
         compile+run path and this lane's own native batch loop",
        trusted.results.len()
    );
    Ok(())
}

pub fn run(args: CeilingArgs) -> anyhow::Result<()> {
    if args.cursor != CURSOR_MODE {
        anyhow::bail!(
            "`--cursor {}` is not a recognised cursor rung — only `{CURSOR_MODE}` exists yet \
             (WHI-1247 step 5); a future issue adds the exact-step / L=1 fixed-lag rung this \
             one Blocks",
            args.cursor
        );
    }
    validate_max_points_requires_no_report(args.max_points, args.no_report)?;

    let bench_config = BenchConfig::load_default()?;
    let (segment_name, segment) = resolve_ceiling_segment(&args.segment_selector, &bench_config)?;
    crate::commands::note_if_not_decision_input(segment_name, segment);

    let base_config = SimulationConfig::default();
    let configs = segment.sim_configs(&base_config);

    if args.self_check {
        return run_self_check(configs);
    }

    if args.fit {
        if matches!(args.variant, VariantArg::Anchored) && args.concentration.is_some() {
            anyhow::bail!(
                "--fit --variant anchored searches concentration jointly; do not pass \
                 --concentration alongside it"
            );
        }
        if args.spread_bps.is_some() {
            anyhow::bail!("--fit always re-fits spread_bps; do not pass --spread-bps alongside it");
        }
    } else if args.concentration.is_none() || args.spread_bps.is_none() {
        anyhow::bail!("either --fit or both --concentration and --spread-bps must be given");
    }

    let reference_slug = validate_reference_allowlist(&args.reference)?;
    let variant_slug = args.variant.slug();
    let stage = format!(
        "ceiling-{variant_slug}-{CURSOR_MODE}-{segment_name}-orbic-oracle-vs-{reference_slug}"
    );

    if !args.no_report {
        crate::report::ensure_report_slot_free(Path::new(DEFAULT_REPORT_DIR), &stage)?;
    }

    let (_ref_slug, reference_lib_path) = resolve_strategy_lib_path(&args.reference)?;
    let reference_lib_path_str = reference_lib_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF8 path {}", reference_lib_path.display()))?;
    let reference_batch =
        compile::build_and_load(reference_lib_path_str, Slot::One)?.run_batch(configs.clone())?;

    let fitted = if args.fit {
        let screening = bench_config.segment("screening")?;
        let screening_configs = screening.sim_configs(&base_config);
        let budget = args
            .max_points
            .unwrap_or_else(|| bench_config.search_max_points());
        run_fit(args.variant, args.concentration, &screening_configs, budget)?
    } else {
        FittedPoint {
            concentration: args.concentration.expect("validated above"),
            spread_bps: args.spread_bps.expect("validated above"),
            search_outcome: None,
        }
    };

    let params = OracleParams {
        variant: args.variant.into(),
        concentration: fitted.concentration,
        spread_bps: fitted.spread_bps,
    };
    let outcome = run_final_eval_catching_panics(params, &configs)?;

    match outcome {
        OracleBatchOutcome::Valid { batch, staleness } => {
            let paired = stats::paired_stat(&batch.results, &reference_batch.results)?;
            let staleness = aggregate_staleness(&staleness);

            println!(
                "ceiling `{stage}`: mean edge diff (oracle - {reference_slug}) = {:.6} \
                 (95% CI [{:.6}, {:.6}], n={})",
                paired.mean_diff, paired.ci_low, paired.ci_high, paired.n
            );

            if !args.no_report {
                let path = write_ceiling_report(
                    CeilingRunMeta {
                        stage: &stage,
                        segment_name,
                        n_sims: batch.n_sims(),
                        n_steps: base_config.n_steps,
                        variant: args.variant,
                        reference_slug: &reference_slug,
                    },
                    &fitted,
                    FinalEvalSummary::Valid {
                        paired: &paired,
                        staleness: &staleness,
                    },
                )?;
                println!("wrote {}", path.display());
            }

            Ok(())
        }
        OracleBatchOutcome::Invalid(reason) => {
            eprintln!(
                "ceiling `{stage}`: the fitted point (concentration={:.4}, spread_bps={:.4}) \
                 panicked during final re-evaluation on `{segment_name}` — caught, not \
                 crashed: {reason}",
                fitted.concentration, fitted.spread_bps
            );

            if !args.no_report {
                let path = write_ceiling_report(
                    CeilingRunMeta {
                        stage: &stage,
                        segment_name,
                        n_sims: configs.len(),
                        n_steps: base_config.n_steps,
                        variant: args.variant,
                        reference_slug: &reference_slug,
                    },
                    &fitted,
                    FinalEvalSummary::Invalid(&reason),
                )?;
                println!("wrote {} (marked INVALID — see the report)", path.display());
            }

            anyhow::bail!(
                "the fitted point {:?} panicked during final re-evaluation on `{segment_name}` \
                 — valid on `screening`'s seeds does not guarantee valid on a different, \
                 larger seed set (docs/DESIGN.md §2.4/§2.5/WHI-1213; the Orbic family's own \
                 quantization jitter, WHI-1206, is exactly this). Do not trust this point \
                 without investigating why: {reason}",
                (fitted.concentration, fitted.spread_bps),
            );
        }
    }
}
