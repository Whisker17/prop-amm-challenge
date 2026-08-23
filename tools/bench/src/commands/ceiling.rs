//! `bench ceiling` — WHI-1247's out-of-competition ceiling lane: how much edge a price
//! re-anchor the arbitrageur cannot front-run can extract, measured as the host-side Orbic
//! oracle curve (`crate::oracle`) against the allowlisted 0-line reference. Nothing this
//! command produces is submittable or ranked (`ceilings/README.md`) — every report this
//! writes says so as its own literal first line (guard (c) below), and every table in it is
//! deliberately shaped differently from `compare.rs`'s own paired-comparison table so the
//! two are never mistaken for each other at a glance.
//!
//! WHI-1248 adds a second cursor rung alongside WHI-1247's trade-triggered one: the
//! exact-step "fingerprint" cursor (`--cursor fingerprint --lag {0,1}`), which advances only
//! when a `compute_swap` call's own `(side, input_amount)` matches the real arbitrageur's
//! own next-step probe, reconstructed host-side (`oracle.rs`'s module doc comment). Its
//! final evaluation never silently drops a tripped seed from the paired statistic — see
//! [`run_fingerprint_final_eval`] for the two-phase (parallel-then-serial) architecture that
//! guarantees every tripped seed is individually named and classified in the report.

use std::collections::{HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use clap::{Args, ValueEnum};
use prop_amm_shared::config::SimulationConfig;
use prop_amm_shared::result::{BatchResult, SimResult};

use crate::commands::resolve_strategy_lib_path;
use crate::compile::{self, Slot};
use crate::config::{BenchConfig, Segment, SegmentSelector};
use crate::oracle::{self, CursorMode, OracleParams, OracleVariant, StalenessSummary};
use crate::params::ParamSpec;
use crate::regime;
use crate::report::DEFAULT_REPORT_DIR;
use crate::search::{self, PointOutcome};
use crate::stats;

/// The one guard checked at two sites: [`run`] checks it before dispatching to [`run_fit`]
/// (so a bad combination fails before the segment/config machinery below it even runs),
/// and [`run_fit`] checks it again in case a future caller ever reaches it a different way.
/// Round-2 review of WHI-1247 caught that the two checks had drifted to differently worded
/// messages for the identical condition — a user could only ever see the first one fire,
/// but the second existed with different wording, ready to surprise the next reader.
/// One shared string keeps both checks (both still need to run) without collapsing them
/// into one call site.
const ANCHORED_FIT_CONCENTRATION_CONFLICT: &str =
    "--fit --variant anchored searches concentration jointly with spread_bps; \
     --concentration must not be passed alongside it";

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

/// WHI-1248's own CLI-selectable cursor rungs, alongside WHI-1247's existing `--variant`.
/// `TradeTriggered` is WHI-1247 step 5's rung (the pre-existing default, no lag concept of
/// its own — its cursor already IS the last executed trade). `Fingerprint` is this issue's
/// exact-step rung, and always requires `--lag` (only `0` and `1` are implemented; `L in
/// {5, 25}` is explicitly out of scope here — see [`validate_cursor_and_lag`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CursorArg {
    TradeTriggered,
    Fingerprint,
}

impl From<CursorArg> for CursorMode {
    fn from(c: CursorArg) -> Self {
        match c {
            CursorArg::TradeTriggered => CursorMode::TradeTriggered,
            CursorArg::Fingerprint => CursorMode::Fingerprint,
        }
    }
}

/// The `stage`/report slug fragment for a given `(cursor, lag)` combination — always called
/// after [`validate_cursor_and_lag`] has already normalized `lag`, so `Fingerprint` always
/// has a real value here (never the `None` the raw CLI arg can hold).
fn cursor_slug(cursor: CursorArg, lag: u64) -> String {
    match cursor {
        CursorArg::TradeTriggered => "trade-triggered".to_string(),
        CursorArg::Fingerprint => format!("fingerprint-l{lag}"),
    }
}

/// Validates the `--cursor`/`--lag` combination and normalizes `lag` to a concrete value
/// (`0` for `TradeTriggered`, which has no lag concept of its own but still needs *some*
/// value threaded into [`OracleParams::fingerprint_lag`], which `oracle.rs` simply ignores
/// outside `Fingerprint` mode). `Fingerprint` requires an explicit `--lag`, and only `0`
/// (the `L=0` clairvoyant diagnostic rung) or `1` (the `L=1` headline deployment rung) are
/// implemented — `L in {5, 25}` is explicitly out of scope for WHI-1248 unless the `L=1`
/// vs. trade-triggered gap turns out to be surprising, in which case it is a follow-up
/// issue, not scope creep here.
fn validate_cursor_and_lag(cursor: CursorArg, lag: Option<u64>) -> anyhow::Result<u64> {
    match cursor {
        CursorArg::TradeTriggered => {
            if lag.is_some() {
                anyhow::bail!(
                    "--lag is only meaningful alongside `--cursor fingerprint` — `--cursor \
                     trade-triggered` has no fixed-lag concept of its own (WHI-1247 step 5's \
                     own cursor already IS the last executed trade)"
                );
            }
            Ok(0)
        }
        CursorArg::Fingerprint => match lag {
            None => anyhow::bail!(
                "`--cursor fingerprint` requires --lag (0 for the L=0 clairvoyant diagnostic \
                 rung, 1 for the L=1 headline deployment rung — WHI-1248)"
            ),
            Some(l) if l == 0 || l == 1 => Ok(l),
            Some(l) => anyhow::bail!(
                "--lag {l} is out of scope for WHI-1248 (only 0 and 1 are implemented; L in \
                 {{5, 25}} is explicitly deferred to a follow-up issue unless the L=1-vs-\
                 trade-triggered gap turns out to be surprising)"
            ),
        },
    }
}

#[derive(Args, Debug)]
pub struct CeilingArgs {
    #[command(flatten)]
    pub segment_selector: SegmentSelector,

    /// Which of WHI-1247 step 3's two `target_x` variants to run.
    #[arg(long, value_enum, default_value = "anchored")]
    pub variant: VariantArg,

    /// Which cursor rung to run. `trade-triggered` (WHI-1247 step 5, the default) moves the
    /// oracle's reading to the last *executed* trade's own step. `fingerprint` (WHI-1248) is
    /// the exact-step rung: it advances only when a `compute_swap` call's own `(side,
    /// input_amount)` matches the real arbitrageur's own next-step probe, reconstructed
    /// host-side (`oracle.rs`'s module doc comment); it requires `--lag` (`0` for the `L=0`
    /// clairvoyant diagnostic rung, `1` for the `L=1` headline deployment rung — `L in {5,
    /// 25}` is out of scope for this issue).
    #[arg(long, value_enum, default_value = "trade-triggered")]
    pub cursor: CursorArg,

    /// The fixed re-anchor lag, in steps, for `--cursor fingerprint` (WHI-1248) — required
    /// alongside it, and only `0` or `1` are implemented. Must not be passed alongside
    /// `--cursor trade-triggered`, which has no lag concept of its own.
    #[arg(long)]
    pub lag: Option<u64>,

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
fn validate_reference_allowlist(reference: &str) -> anyhow::Result<(String, PathBuf)> {
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
    // Return the already-resolved, already-canonicalized lib_path (not just the slug) so
    // `run` never has to call `resolve_strategy_lib_path` a second time for the same
    // reference — round-3 review caught that the second call was not just redundant I/O,
    // it reopened the exact window (a second, independent filesystem resolve) that could
    // in principle disagree with the one this guard just validated.
    Ok((slug, lib_path))
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

/// Last `Location` captured by [`catch_panicking`]'s own hook (below) — a caught panic's
/// payload alone is often uninformative, but the default hook's `file:line:column` is
/// always available regardless of payload type, since `PanicHookInfo::location()` doesn't
/// go through the payload at all. Reset to `None` before every `catch_unwind` so a stale
/// location from an earlier point can never be attributed to a later one; read back only
/// inside `catch_panicking`'s own `Err(payload)` arm.
static LAST_PANIC_LOCATION: Mutex<Option<String>> = Mutex::new(None);

/// WHI-1248: the fingerprint-mode "calls since the cursor last advanced" diagnostic
/// (`oracle::fingerprint_panic_diagnostics`), captured the same way as
/// [`LAST_PANIC_LOCATION`] — from *inside* the installed panic hook, which runs
/// synchronously on the panicking thread before any unwinding begins, so the thread-local
/// counter `oracle.rs` maintains is still valid to read there. Reading it back from
/// `catch_panicking`'s own `Err(payload)` arm (which runs on the *calling* thread, not
/// necessarily the one that panicked when the panic propagated up through a rayon `join`)
/// would read the wrong thread's own thread-local value — this static is what carries the
/// right one across that boundary. `None` outside `Fingerprint` mode (`oracle.rs` returns
/// `None` there unconditionally).
static LAST_PANIC_FP_CALLS: Mutex<Option<u64>> = Mutex::new(None);

/// A caught panic's recovered evidence — [`catch_panicking`]'s `Err` payload. `message` is
/// display-ready (any [`LAST_PANIC_LOCATION`] hit is already appended); `recovered` says
/// whether that text actually came from the panic's own `&str`/`String` payload (`true`, the
/// `curve_checks.rs` shape-check case) or was synthesized from the payload's own `TypeId`
/// because it downcast to neither (`false`, WHI-1248's fix to the original "defeated the
/// downcast" placeholder — see [`panic_message`]). `fingerprint_calls_since_advance` is
/// `Some(_)` only when the run that panicked was in `Fingerprint` cursor mode; used by
/// [`classify_fingerprint_panic`] to distinguish a genuine hardening-check assertion from a
/// `curve_checks.rs` shape panic that itself resulted from an early/false cursor advance.
struct PanicOutcome {
    message: String,
    recovered: bool,
    fingerprint_calls_since_advance: Option<u64>,
}

/// Runs `f`, catching any panic (e.g. a shape-check panic from `crates/sim/src/curve_checks.rs`
/// the same way `commands/fit.rs::run_batch_catching_panics` does — `curve_checks.rs` treats a
/// native submission fn identically to a compiled one, keying only on the AMM's own
/// `"submission"` name, so a pathological corner of the oracle curve's own search space can
/// panic mid-simulation exactly like a compiled candidate's PARAMS point can, and WHI-1248's
/// own fingerprint hardening-check assertions panic the same way too) into
/// `Ok(Err(panic_message))` rather than propagating it. The default hook's stderr print is
/// still suppressed for the duration (this is an expected, load-bearing control-flow path
/// during a 300-point search, not a real crash — printing one stderr line per invalid point
/// would swamp the terminal), but unlike a bare no-op hook, the installed hook first records
/// the panic's source location into [`LAST_PANIC_LOCATION`] and (WHI-1248) the fingerprint
/// call-since-advance counter into [`LAST_PANIC_FP_CALLS`] before doing nothing else — so a
/// caught panic with a non-string payload still carries a real `file:line:column` an
/// investigator can jump to, independent of the payload's type. Shared by every
/// catch-and-continue call site in this file — the search loop's own per-point evaluation
/// ([`run_catching_panics`]), the final re-evaluation of the fitted point
/// ([`run_final_eval_catching_panics`]), and WHI-1248's own serial per-tripped-seed re-run
/// ([`run_fingerprint_final_eval`]) — since all three need the exact same
/// `take_hook`/`set_hook`/`catch_unwind` bracket and differ only in what they do with a
/// successful `T`. `oracle::run_batch`'s own per-simulation loop is rayon-parallel
/// (`oracle.rs`'s `native_pool`), so more than one worker thread can panic before the first
/// unwind reaches this frame; the hook is process-global regardless of which thread panics,
/// and both statics intentionally keep only the most recent write rather than every one —
/// enough to point at *a* real panic site, not a claim that it is uniquely the one
/// `catch_unwind` observed unwinding.
fn catch_panicking<T>(
    f: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<Result<T, PanicOutcome>> {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        if let Ok(mut loc) = LAST_PANIC_LOCATION.lock() {
            *loc = Some(info.location().map(|l| l.to_string()).unwrap_or_default());
        }
        if let Ok(mut fp) = LAST_PANIC_FP_CALLS.lock() {
            *fp = crate::oracle::fingerprint_panic_diagnostics();
        }
    }));
    if let Ok(mut loc) = LAST_PANIC_LOCATION.lock() {
        *loc = None;
    }
    if let Ok(mut fp) = LAST_PANIC_FP_CALLS.lock() {
        *fp = None;
    }
    let result = catch_unwind(AssertUnwindSafe(f));
    std::panic::set_hook(previous_hook);

    match result {
        Ok(Ok(value)) => Ok(Ok(value)),
        Ok(Err(e)) => Err(e),
        Err(payload) => {
            let location = LAST_PANIC_LOCATION.lock().ok().and_then(|mut g| g.take());
            let fp_calls = LAST_PANIC_FP_CALLS.lock().ok().and_then(|mut g| g.take());
            let (message, recovered) = panic_message(&payload);
            let with_location = match location {
                Some(loc) if !loc.is_empty() => format!("{message} (at {loc})"),
                _ => message,
            };
            Ok(Err(PanicOutcome {
                message: with_location,
                recovered,
                fingerprint_calls_since_advance: fp_calls,
            }))
        }
    }
}

fn run_catching_panics(
    f: impl FnOnce() -> anyhow::Result<BatchResult>,
) -> anyhow::Result<PointOutcome> {
    match catch_panicking(f)? {
        Ok(batch) => Ok(PointOutcome::Valid(batch.avg_edge())),
        Err(outcome) => Ok(PointOutcome::Invalid(outcome.message)),
    }
}

/// WHI-1248: the fingerprint-mode counterpart to [`run_catching_panics`] used by
/// [`run_fit`]'s search loop once a candidate point's `OracleParams::cursor_mode ==
/// CursorMode::Fingerprint`. Runs the same per-seed-isolated Phase 1 pipeline
/// ([`oracle::run_batch_fingerprint_checked`]) the final evaluation uses, but — unlike
/// [`run_fingerprint_final_eval`] — never serially re-runs a tripped seed to recover its
/// message: the search loop only ever consumes the returned `f64`, so paying that cost on
/// every one of up to 300 candidate points would be pure waste. `Invalid` only when not a
/// single seed survived (nothing to average); otherwise `Valid` over whatever did survive,
/// exactly mirroring how the final report's own number is computed so a search that
/// prefers one point over another is optimizing the same quantity that gets reported.
fn evaluate_fingerprint_point(
    params: OracleParams,
    configs: &[SimulationConfig],
) -> anyhow::Result<PointOutcome> {
    let (oks, tripped) = oracle::run_batch_fingerprint_checked(params, configs)?;
    if oks.is_empty() {
        return Ok(PointOutcome::Invalid(format!(
            "all {} seed(s) tripped a WHI-1248 fingerprint hardening check on this point              (nothing survived to average)",
            tripped.len()
        )));
    }
    let results: Vec<_> = oks.into_iter().map(|(result, _staleness)| result).collect();
    let batch = BatchResult::from_results(results);
    Ok(PointOutcome::Valid(batch.avg_edge()))
}

/// Originally byte-identical to `commands/fit.rs::panic_message` (which has the same job
/// for `fit`'s own train/validation re-evaluation) — a deliberate duplicate, not a missed
/// dedup: WHI-1247's "what must not change" list forbids editing `fit.rs` or exposing its
/// private helpers, and `panic_message` is private there, so there is no way to share one
/// definition without violating that constraint. Diverges from that copy in its return
/// type (round-3 review of WHI-1247, standards finding #2): `fit.rs` never needs to tell a
/// caller whether the message it got back was actually recovered from the payload or is a
/// placeholder, but this lane's own `Invalid` report section does.
///
/// WHI-1248 changes this function twice over (the amendment's scope item #4).
///
/// First, and most importantly: the original two-branch version (`&str`, then `String`,
/// then a fixed placeholder) was verified against synthetic unit-test payloads only, and
/// turned out to be broken end-to-end against the *real* fingerprint-cursor pipeline — a
/// live smoke test on `screening` (103/200 seeds tripped by the hardening checks below)
/// showed every single trip falling to the placeholder branch, even though the underlying
/// panics are ordinary `assert_eq!` (hardening check (a), `oracle.rs`) and `panic!`
/// (`curve_checks.rs`'s shape violation) calls whose payloads are, at the origin, plain
/// `String`. Root cause: `run_batch`/`run_batch_fingerprint_checked` run each seed inside a
/// `rayon` pool via `.par_iter().map(...).collect()`; whenever the panicking seed actually
/// executes on a pool *worker* thread rather than the thread that called `.install()` (true
/// for nearly every seed, since 8 seeds run concurrently), `rayon-core`'s own cross-thread
/// panic-propagation machinery catches the panic on the worker, stores it, and
/// `resume_unwind`s it on the joining thread wrapped in *its own* `Box<dyn Any + Send>` —
/// so the payload `catch_panicking`'s `catch_unwind` hands back here is one layer of
/// `Box<dyn Any + Send>` deeper than the original panic site, and a flat `&str`/`String`
/// downcast on it always misses. (Confirmed directly: instrumenting this function inside a
/// real run showed `Any::type_id()` matching `Box<dyn Any + Send>` at depth 0 and matching
/// `String` at depth 1, on all 103/103 tripped seeds; three isolated repro programs did
/// *not* reproduce this, because none of them forced the panicking closure onto a non-
/// calling worker thread under real contention.) The fix: peel any `Box<dyn Any + Send>`
/// layers (bounded by `MAX_UNWRAP_DEPTH`, defensively, in case a future rayon version boxes
/// more than once) before attempting the `&str`/`String` downcasts, rather than assuming a
/// single flat layer.
///
/// Second, the fallback branch's own behavior: the original returned `None` for a
/// non-string payload, which `catch_panicking` turned into the fixed, uninformative
/// placeholder `"panicked with a non-string payload"`. For whatever still isn't a
/// `&str`/`String` after unwrapping — e.g. an arithmetic overflow panic — the fallback now
/// synthesizes a message from the payload's own `Any::type_id()` and how many box layers
/// were peeled, instead of a fixed placeholder: strictly more informative (it names the
/// concrete type a reader can go looking for in the source), never less, and never invents
/// text that isn't true of the payload.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> (String, bool) {
    // WHI-1248 amendment scope item #4: a real panic caught through `catch_panicking` here
    // is not always a bare `&str`/`String` payload one level down. Rayon's own cross-thread
    // panic propagation (`rayon-core`'s job/unwind machinery, exercised whenever the
    // panicking work actually runs on a pool worker thread rather than the thread that
    // called `.install()`) re-wraps the original payload in its own `Box<dyn Any + Send>`
    // before `resume_unwind`-ing it to the joining thread — so the payload this function
    // receives is one level of `Box<dyn Any + Send>` deeper than the original `panic!`/
    // `assert_eq!` call site. Peel that (and, defensively, any further nesting) before
    // attempting the `&str`/`String` downcasts, rather than assuming a single flat layer.
    const MAX_UNWRAP_DEPTH: u8 = 10;
    let mut current: &(dyn std::any::Any + Send) = payload;
    let mut depth = 0u8;
    loop {
        if let Some(s) = current.downcast_ref::<&str>() {
            return (s.to_string(), true);
        }
        if let Some(s) = current.downcast_ref::<String>() {
            return (s.clone(), true);
        }
        if depth >= MAX_UNWRAP_DEPTH {
            break;
        }
        match current.downcast_ref::<Box<dyn std::any::Any + Send>>() {
            Some(inner) => {
                current = &**inner;
                depth += 1;
            }
            None => break,
        }
    }
    (
        format!(
            "non-string panic payload (Any::type_id = {:?}, peeled {depth} nested box \
             layer(s)) — recovered no message text, only that a panic occurred and this \
             payload's concrete type",
            current.type_id()
        ),
        false,
    )
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
///
/// Used only for the `TradeTriggered` cursor — [`run_fingerprint_final_eval`] is
/// `Fingerprint` mode's own final-evaluation path, with the stronger "never silently drop a
/// tripped seed" guarantee WHI-1248 requires.
enum OracleBatchOutcome {
    Valid {
        batch: BatchResult,
        staleness: Vec<StalenessSummary>,
    },
    Invalid(PanicOutcome),
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
        Err(outcome) => Ok(OracleBatchOutcome::Invalid(outcome)),
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
    Invalid(&'a PanicOutcome),
}

/// The fitted (or explicitly supplied) point this run measures at.
struct FittedPoint {
    concentration: f64,
    spread_bps: f64,
    /// `None` when the point came straight from `--concentration`/`--spread-bps` rather
    /// than a search.
    search_outcome: Option<search::SearchOutcome>,
}

/// The `screening`-segment fit loop is kept generically usable under either cursor mode
/// (`cursor_mode`/`fingerprint_lag` are threaded straight into every point's own
/// [`OracleParams`]). For `CursorMode::TradeTriggered` it keeps the pre-existing single
/// batch-level-panic [`run_catching_panics`]/`oracle::run_batch` pattern unchanged from
/// WHI-1247: a search point that panics on even one of `screening`'s own seeds is simply
/// `Invalid` for that point.
///
/// For `CursorMode::Fingerprint` that all-or-nothing rule is unusable in practice, not just
/// theoretically stricter: a live measurement (docs/... `ceilings/C-orbic-oracle/NOTES.md`)
/// found the per-seed hardening-check trip rate on `screening` running ~50% under the L=1
/// rung, and 200 IID screening seeds each with *any* positive trip probability are, with
/// near certainty, never simultaneously panic-free — confirmed directly: before this fix,
/// `--fit --max-points 30` reported "every evaluated grid point (9) was invalid (a caught
/// shape-check panic)" and could never produce a fitted point at all. So every candidate
/// point evaluated under fingerprint mode instead goes through
/// [`evaluate_fingerprint_point`], which mirrors [`run_fingerprint_final_eval`]'s own
/// semantics (mean edge over the *surviving* seeds only, `Invalid` only if literally none
/// survive) rather than this function's own now-inapplicable all-or-nothing rule.
/// WHI-1248's "never silently dropped" per-seed architecture (full message recovery, not
/// just a valid/invalid split) still applies only to the *final* evaluation
/// ([`run_fingerprint_final_eval`]) — this search loop only needs a number per point, so it
/// skips that function's serial per-tripped-seed re-run to avoid multiplying wall-clock
/// cost across a budget of up to 300 points for messages nothing here reads.
fn run_fit(
    variant: VariantArg,
    fixed_concentration: Option<f64>,
    screening_configs: &[SimulationConfig],
    budget: usize,
    cursor_mode: CursorMode,
    fingerprint_lag: u64,
) -> anyhow::Result<FittedPoint> {
    match variant {
        VariantArg::Anchored => {
            if fixed_concentration.is_some() {
                anyhow::bail!("{ANCHORED_FIT_CONCENTRATION_CONFLICT}");
            }
            let specs = [concentration_spec(), spread_bps_spec()];
            let outcome = search::coarse_grid_then_coordinate_descent(&specs, budget, |values| {
                let concentration = values[0] as f64 / 100.0;
                let spread_bps = values[1] as f64;
                let params = OracleParams {
                    variant: variant.into(),
                    concentration,
                    spread_bps,
                    cursor_mode,
                    fingerprint_lag,
                };
                let configs = screening_configs.to_vec();
                if cursor_mode == CursorMode::Fingerprint {
                    evaluate_fingerprint_point(params, &configs)
                } else {
                    run_catching_panics(move || {
                        oracle::run_batch(params, &configs).map(|(batch, _staleness)| batch)
                    })
                }
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
                    cursor_mode,
                    fingerprint_lag,
                };
                let configs = screening_configs.to_vec();
                if cursor_mode == CursorMode::Fingerprint {
                    evaluate_fingerprint_point(params, &configs)
                } else {
                    run_catching_panics(move || {
                        oracle::run_batch(params, &configs).map(|(batch, _staleness)| batch)
                    })
                }
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
/// numbers rather than dumping one row per simulation (1,000 rows would swamp the report).
/// Two different things are deliberately both kept, not one substituted for the other:
///
/// - `mean_of_means`, `median_of_means`, `p95_of_means` are genuine statistics of the
///   per-simulation *mean* staleness — the mean, p50, and a real (not maximum-as-a-proxy)
///   95th percentile of that distribution across simulations.
/// - `max_of_p95` and `max_of_max` are deliberately worst-case (not percentile) figures —
///   the largest per-sim p95 and the largest per-sim max seen anywhere in the batch — kept
///   because an average-of-averages alone could hide a badly stale minority of simulations
///   that a pure percentile-of-means statistic would also under-weight.
struct StalenessAggregate {
    mean_of_means: f64,
    median_of_means: f64,
    p95_of_means: f64,
    max_of_p95: f64,
    max_of_max: f64,
}

fn aggregate_staleness(summaries: &[StalenessSummary]) -> StalenessAggregate {
    let mut means: Vec<f64> = summaries.iter().map(|s| s.mean).collect();
    let p95s: Vec<f64> = summaries.iter().map(|s| s.p95).collect();
    let maxes: Vec<f64> = summaries.iter().map(|s| s.max).collect();

    let mean_of_means = if means.is_empty() {
        0.0
    } else {
        means.iter().sum::<f64>() / means.len() as f64
    };
    let median_of_means = stats::median(&means).unwrap_or(0.0);
    means.sort_by(|a, b| a.partial_cmp(b).expect("staleness means are finite"));
    let p95_of_means = oracle::nearest_rank(&means, 0.95);
    let max_of_p95 = p95s.iter().cloned().fold(0.0, f64::max);
    let max_of_max = maxes.iter().cloned().fold(0.0, f64::max);

    StalenessAggregate {
        mean_of_means,
        median_of_means,
        p95_of_means,
        max_of_p95,
        max_of_max,
    }
}

/// The run's own identity — every field [`write_ceiling_report`] needs to describe *what*
/// was measured, as opposed to [`FittedPoint`]/[`FinalEvalSummary`] which describe the
/// measurement itself. These seven always travel together (one run has exactly one stage,
/// one segment, one sim/step count, one variant, one cursor, one reference) — bundled into
/// one struct rather than positional parameters so the call site reads as "the run" and not
/// an order-sensitive tuple, and so `write_ceiling_report` no longer needs
/// `#[allow(clippy::too_many_arguments)]` to satisfy that lint honestly rather than by
/// suppression.
struct CeilingRunMeta<'a> {
    stage: &'a str,
    segment_name: &'a str,
    n_sims: usize,
    n_steps: u32,
    variant: VariantArg,
    reference_slug: &'a str,
    cursor_slug: &'a str,
}

/// WHI-1247 step 7 guard (c): the report's own machine-readable marker, as the literal first
/// line, plus every field a reader needs to know what was measured and how — reusing only
/// [`crate::report::ensure_report_slot_free`] and [`DEFAULT_REPORT_DIR`] from `report.rs`
/// (never `write_report`/`ReportMeta`, whose own first line is always `# {stage} — {date}`,
/// structurally incompatible with this guard). Tables here are deliberately shaped
/// differently from `compare.rs`'s own `| regime | n | mean diff | 95% CI |` — this is an
/// out-of-competition measurement and must never be mistaken for a ranked comparison at a
/// glance.
///
/// `extra_sections` (WHI-1248) is appended verbatim after the core content below, for
/// whatever additional material only one cursor mode needs (the per-sigma-tier slices for
/// every rung; the fingerprint hardening-check trip table and, for `--lag 0`, the analytic
/// envelope check, both `Fingerprint`-only) — empty for a plain `TradeTriggered` run.
fn write_ceiling_report(
    meta: CeilingRunMeta<'_>,
    fitted: &FittedPoint,
    final_eval: FinalEvalSummary<'_>,
    extra_sections: &str,
) -> anyhow::Result<std::path::PathBuf> {
    let CeilingRunMeta {
        stage,
        segment_name,
        n_sims,
        n_steps,
        variant,
        reference_slug,
        cursor_slug,
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
    match &final_eval {
        FinalEvalSummary::Valid { .. } => out.push_str(
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
        ),
        FinalEvalSummary::Invalid(_) => out.push_str(
            "**This run produced no number.** The same three honesty constraints (WHI-1247 \
             § Context) — one-sided lower bound, mostly `retail volume x captured spread x \
             flow share(spread)`, generalizing beyond the Orbic curve specifically — would \
             bound what an edge figure means here, but they bind nothing until a valid \
             re-evaluation exists. See \"## Final re-evaluation: INVALID\" below for why this \
             run has none.\n\n",
        ),
    }
    out.push_str(&format!("- Commit: `{}`\n", commit_sha()));
    out.push_str(&format!("- Segment: `{segment_name}`\n"));
    out.push_str(&format!("- Simulations: {n_sims}\n"));
    out.push_str(&format!("- Steps: {n_steps}\n"));
    out.push_str("- Execution path: native (host-side, never BPF-compiled)\n");
    out.push_str(&format!("- Variant: {}\n", variant.slug()));
    out.push_str(&format!("- Cursor mode: {cursor_slug}\n"));
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
            if variant == VariantArg::Floating {
                out.push_str(
                    "**Diagnostic caveat (variant: floating):** WHI-1247 step 3 scopes this \
                     variant as \"never a result on its own\" — a contrast against `anchored`, \
                     not a second headline figure. The numbers below are real, but do not read \
                     them as this lane's finding; see `ceilings/C-orbic-oracle/NOTES.md` § \
                     \"Anchored vs. floating\" for why.\n\n",
                );
            }
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
                "**Reminder:** per the one-sided-bound honesty constraint above, this is a \
                 lower bound on perfect-information value, not an upper bound on any specific \
                 submission's remaining headroom — a stronger submission clearing this number \
                 is expected and not itself informative about how much further headroom \
                 remains.\n\n",
            );

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
                "| p95 of per-sim means | {:.3} |\n",
                staleness.p95_of_means
            ));
            out.push_str(&format!(
                "| max of per-sim p95 | {:.3} |\n",
                staleness.max_of_p95
            ));
            out.push_str(&format!(
                "| max of per-sim max | {:.3} |\n",
                staleness.max_of_max
            ));
            out.push('\n');
        }
        FinalEvalSummary::Invalid(panic) => {
            out.push_str("## Final re-evaluation: INVALID\n\n");
            let cause_sentence = if panic.recovered {
                "The panic message below was recovered verbatim from the panic's own payload \
                 — a `curve_checks.rs`-style `panic!(\"submission shape violation ...\")` or a \
                 WHI-1248 hardening-check `assert!`/`assert_eq!` always downcasts cleanly to a \
                 `String` — so it is the actual cause, not a placeholder."
            } else {
                "The panic message below was synthesized from the panic payload's own \
                 `Any::type_id()` (WHI-1248): the payload was neither `&str` nor `String`, so \
                 no literal message text could be recovered, but the message still names the \
                 payload's concrete type rather than falling back to a fixed, uninformative \
                 placeholder."
            };
            out.push_str(&format!(
                "The fitted point above was chosen from a search on the `screening` segment, \
                 but re-evaluating it on the full `{segment_name}` segment triggered a caught \
                 panic instead of producing a number. This is the documented failure mode in \
                 `docs/DESIGN.md` §2.4/§2.5 (WHI-1213): a point valid on `screening`'s seeds \
                 is not guaranteed valid on a different, larger seed set — exactly what made \
                 the Orbic family's own \"quantization jitter\" (`docs/DESIGN.md` §6.2, \
                 strategy `002`, WHI-1206, Canceled) probabilistic across seeds, not just \
                 across parameter values. {cause_sentence} No edge-vs-0-line or staleness \
                 numbers exist for this run; the absence of a crash on `screening` is not \
                 evidence this variant/point is safe on other segments or seeds.\n\n"
            ));
            out.push_str(&format!("Panic message: `{}`\n\n", panic.message));
        }
    }

    out.push_str(extra_sections);

    std::fs::write(&path, out)
        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", path.display()))?;
    Ok(path)
}

/// Duplicated from `report.rs::commit_sha` (private there, byte-for-byte the same logic,
/// including the `--untracked-files=no` rationale below): round-2 review of WHI-1247
/// suggested making the original `pub(crate)` instead, but WHI-1247's own "what must not
/// change" list is not "don't extend `report.rs`'s public surface" — it is **zero edits**
/// to `tools/bench/src/report.rs` at all, full stop, alongside a fixed list of other
/// upstream-adjacent modules. A visibility-only change is still an edit to that file, so
/// this duplicate (and `today`/`civil_from_days` below, and `panic_message` above, shared
/// with `fit.rs` under the identical constraint) is the correct call under the spec as
/// written, not a missed dedup opportunity.
fn commit_sha() -> String {
    let sha = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    // `--untracked-files=no`: an untracked scratch file (a build artifact, a not-yet-added
    // report) shouldn't mark the *code* dirty — only uncommitted changes to tracked files
    // should, since that's what actually means "this measurement's code differs from HEAD".
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
/// own constraint set is **zero edits** to `tools/bench/src/report.rs` (see the issue
/// body's "what must not change" — not merely "don't extend its public surface"; even a
/// visibility-only `pub(crate)` change is still an edit to that file), so this report
/// writer duplicates the same days-since-epoch civil-date algorithm rather than touching
/// that module. See [`commit_sha`] immediately above for the same reasoning applied to a
/// second duplicated helper.
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

/// WHI-1248: one tripped seed's own re-run outcome — the "never silently dropped from a
/// paired statistic" acceptance criterion means every seed that trips a fingerprint
/// hardening check during Phase 1 (`oracle::run_batch_fingerprint_checked`'s parallel,
/// per-seed `catch_unwind`) is individually named here, whether or not it survives Phase 2's
/// serial re-run. `label` classifies the recovered message per [`classify_fingerprint_panic`].
struct TrippedSeedOutcome {
    seed: u64,
    label: &'static str,
    message: String,
}

/// Named alias for `run_fingerprint_final_eval`'s return type — clippy's `type_complexity`
/// lint objects to the bare nested tuple-of-Vecs spelled out inline.
type FingerprintFinalEvalOutcome = (Vec<(SimResult, StalenessSummary)>, Vec<TrippedSeedOutcome>);

/// A rough upper bound on how many `compute_swap` calls a single simulation step's own
/// bisection-style price search (`crates/sim/src/arbitrageur.rs`'s bracket-then-refine
/// search for the arbitrageur's own optimal trade size) can issue before it either finds a
/// profitable size or gives up — used only to distinguish, heuristically, "the cursor
/// advanced on a false match very early in a step's own search" (few calls since the last
/// advance) from "a shape panic that has nothing to do with the fingerprint cursor at all"
/// (many calls since the last advance, or a message that doesn't look like a shape
/// violation to begin with). This is a diagnostic heuristic, not a hard guarantee — every
/// tripped seed's own message is still reported verbatim regardless of which bucket it
/// lands in, so a wrong guess here costs a misleading label, never a lost seed.
const FP_EARLY_ADVANCE_CALL_THRESHOLD: u64 = 44;

/// Classifies a fingerprint-mode panic into one of three buckets, purely from the recovered
/// [`PanicOutcome`] (WHI-1248's own required distinction: "an early cursor advance may
/// surface first as a `curve_checks` panic... rather than as the assertion — catch and
/// label that case distinctly"):
///
/// - `"CursorAssertion"`: the message is one of `oracle.rs`'s own three hardening-check
///   assertions (they all share the literal prefix checked below) — a directly observed
///   cursor-verification failure, not an inference.
/// - `"SuspectedEarlyAdvance"`: the message looks like a `crates/sim/src/curve_checks.rs`
///   shape violation (`"submission shape violation during"`), *and* the fingerprint cursor
///   had advanced only a handful of calls ago (at or under
///   [`FP_EARLY_ADVANCE_CALL_THRESHOLD`]) — consistent with an early/false cursor advance
///   having handed the oracle curve a wrong-for-this-step price that then failed its own
///   shape check, exactly the surfacing path the issue calls out.
/// - `"Other"`: neither of the above — a panic this lane cannot attribute to the
///   fingerprint cursor specifically from the message and call count alone.
fn classify_fingerprint_panic(outcome: &PanicOutcome) -> &'static str {
    if outcome
        .message
        .contains("WHI-1248 fingerprint cursor hardening check")
    {
        "CursorAssertion"
    } else if outcome.message.contains("submission shape violation")
        && matches!(
            outcome.fingerprint_calls_since_advance,
            Some(n) if n <= FP_EARLY_ADVANCE_CALL_THRESHOLD
        )
    {
        "SuspectedEarlyAdvance"
    } else {
        "Other"
    }
}

/// WHI-1248's Fingerprint-mode final evaluation — the two-phase "never silently dropped"
/// architecture the issue requires. Phase 1 ([`oracle::run_batch_fingerprint_checked`]) runs
/// every seed with its own per-seed `catch_unwind` inside a rayon `par_iter`, so one seed's
/// trip never drops any *other* seed's valid result. Phase 2, here, serially re-runs each
/// Phase-1-tripped seed, one at a time, through the pre-existing [`catch_panicking`] /
/// `oracle::run_batch` (the same machinery `run_final_eval_catching_panics` already uses)
/// to recover a full, classified message. A seed that does *not* reproduce its trip on this
/// solo re-run is reported as such and its (now successful) result is folded back in; a
/// seed that reproduces the trip is reported with its classification and excluded from the
/// surviving set the caller pairs against the reference batch — either way, every tripped
/// seed's own fate is named in the returned `Vec<TrippedSeedOutcome>`, never silently
/// absorbed into a smaller `n` with no trace.
fn run_fingerprint_final_eval(
    params: OracleParams,
    configs: &[SimulationConfig],
) -> anyhow::Result<FingerprintFinalEvalOutcome> {
    let (mut oks, tripped_seeds) = oracle::run_batch_fingerprint_checked(params, configs)?;

    let mut tripped_outcomes = Vec::with_capacity(tripped_seeds.len());
    for tripped in tripped_seeds {
        let single = vec![tripped.config.clone()];
        let seed = tripped.seed;
        match catch_panicking(move || oracle::run_batch(params, &single))? {
            Ok((batch, staleness)) => {
                tripped_outcomes.push(TrippedSeedOutcome {
                    seed,
                    label: "RecoveredOnSerialRerun",
                    message: "did not reproduce when re-run serially and alone; its result \
                              from that re-run is included in the paired statistic below"
                        .to_string(),
                });
                if let (Some(result), Some(stale)) = (
                    batch.results.into_iter().next(),
                    staleness.into_iter().next(),
                ) {
                    oks.push((result, stale));
                }
            }
            Err(outcome) => {
                let label = classify_fingerprint_panic(&outcome);
                tripped_outcomes.push(TrippedSeedOutcome {
                    seed,
                    label,
                    message: outcome.message,
                });
            }
        }
    }

    Ok((oks, tripped_outcomes))
}

/// Restores `configs`' own original relative order over a (possibly reordered, by Phase 2
/// appending recovered seeds at the end) set of surviving `(SimResult, StalenessSummary)`
/// pairs — required so the candidate results line up index-for-index with the reference
/// batch (built straight from `configs` in its original order, and itself filtered to the
/// same surviving-seed set via [`filter_batch_to_seeds`]) for `stats::paired_stat`, which
/// pairs strictly by index, not by seed lookup.
fn reorder_oks_to_configs_order(
    configs: &[SimulationConfig],
    oks: Vec<(SimResult, StalenessSummary)>,
) -> (Vec<SimResult>, Vec<StalenessSummary>) {
    let mut by_seed: HashMap<u64, (SimResult, StalenessSummary)> =
        oks.into_iter().map(|(r, s)| (r.seed, (r, s))).collect();
    let mut results = Vec::new();
    let mut staleness = Vec::new();
    for config in configs {
        if let Some((r, s)) = by_seed.remove(&config.seed) {
            results.push(r);
            staleness.push(s);
        }
    }
    (results, staleness)
}

/// Filters a reference [`BatchResult`] down to exactly the surviving seed set, preserving
/// its own existing relative order (which already matches `configs`' original order, since
/// the reference batch is built straight from `configs.clone()`) — the counterpart to
/// [`reorder_oks_to_configs_order`] so both sides of a fingerprint-mode paired comparison
/// are index-aligned subsequences of the very same original seed order.
fn filter_batch_to_seeds(batch: &BatchResult, seeds: &HashSet<u64>) -> BatchResult {
    let results: Vec<SimResult> = batch
        .results
        .iter()
        .filter(|r| seeds.contains(&r.seed))
        .cloned()
        .collect();
    BatchResult::from_results(results)
}

/// WHI-1248: per-sigma-tier slices of the same paired comparison the headline table
/// reports — reuses `regime.rs`'s own tier reconstruction (`Tier::{Low,Mid,High}`, equal-
/// width thirds of `HyperparameterVariance`'s own sampling range, which by construction
/// track `config/bench.toml`'s three `[grid] gbm_sigma_levels`, docs/DESIGN.md §2.3),
/// grouped purely by `Regime.sigma` — deliberately narrower than `regime::slice_paired_stats`'s
/// own full 3-axis (fee x liquidity x sigma) grouping, since this issue's own scope is
/// specifically a claim about the sigma axis alone ("converge at low sigma / fan out at
/// high sigma"), not the full regime.
fn slice_by_sigma_tier(
    base: &SimulationConfig,
    candidate: &[SimResult],
    reference: &[SimResult],
) -> anyhow::Result<Vec<(regime::Tier, stats::PairedStat)>> {
    if candidate.len() != reference.len() {
        anyhow::bail!(
            "sigma-tier slicing requires equal-length batches: candidate={} reference={}",
            candidate.len(),
            reference.len()
        );
    }
    let mut bins: std::collections::BTreeMap<regime::Tier, (Vec<SimResult>, Vec<SimResult>)> =
        std::collections::BTreeMap::new();
    for (i, (c, r)) in candidate.iter().zip(reference.iter()).enumerate() {
        if c.seed != r.seed {
            anyhow::bail!(
                "sigma-tier slicing unpaired at index {i}: candidate seed {} vs reference \
                 seed {}",
                c.seed,
                r.seed
            );
        }
        let tier = regime::classify_seed(base, c.seed).sigma;
        let entry = bins.entry(tier).or_default();
        entry.0.push(c.clone());
        entry.1.push(r.clone());
    }
    bins.into_iter()
        .map(|(tier, (c, r))| Ok((tier, stats::paired_stat(&c, &r)?)))
        .collect()
}

/// Renders [`slice_by_sigma_tier`]'s output as a report section, plus the explicit
/// converge-at-low/fan-out-at-high verdict WHI-1248 requires. `grid_levels`, when available
/// (`config/bench.toml`'s existing `[grid] gbm_sigma_levels`, read-only — no new `[grid]`
/// axis is added), labels each tier with the representative level it roughly corresponds to;
/// purely cosmetic; slicing itself never depends on it. The verdict is operationalized as
/// 95% CI *width* (a real statistic [`stats::PairedStat`] already exposes per bin) rather
/// than a raw per-sim variance, which is not available per bin from the existing stats
/// machinery — stated explicitly here so the operationalization is never mistaken for the
/// only possible one.
fn format_sigma_slices(
    grid_levels: Option<&[f64]>,
    slices: &[(regime::Tier, stats::PairedStat)],
) -> String {
    let mut out = String::new();
    out.push_str("## Per-sigma-tier slices (WHI-1248)\n\n");
    out.push_str(
        "Slices purely by `regime.rs`'s own `sigma` tier (Low/Mid/High thirds of \
         `HyperparameterVariance`'s sampling range, which by construction track \
         `config/bench.toml`'s three `[grid] gbm_sigma_levels`), ignoring the fee/liquidity \
         axes `regime::slice_paired_stats` also splits on — a claim specifically about the \
         sigma axis, not the full regime.\n\n",
    );
    out.push_str("| sigma tier | approx level | n | mean diff | 95% CI |\n|---|---|---|---|---|\n");
    let sorted_levels: Option<Vec<f64>> = grid_levels.map(|l| {
        let mut v = l.to_vec();
        v.sort_by(|a, b| a.partial_cmp(b).expect("gbm_sigma_levels are finite"));
        v
    });
    for (tier, stat) in slices {
        let level_str = match (&sorted_levels, tier) {
            (Some(levels), regime::Tier::Low) if levels.len() == 3 => format!("{:.4}", levels[0]),
            (Some(levels), regime::Tier::Mid) if levels.len() == 3 => format!("{:.4}", levels[1]),
            (Some(levels), regime::Tier::High) if levels.len() == 3 => {
                format!("{:.4}", levels[2])
            }
            _ => "n/a".to_string(),
        };
        out.push_str(&format!(
            "| {:?} | {level_str} | {} | {:.6} | [{:.6}, {:.6}] |\n",
            tier, stat.n, stat.mean_diff, stat.ci_low, stat.ci_high
        ));
    }
    out.push('\n');

    let low_width = slices
        .iter()
        .find(|(t, _)| *t == regime::Tier::Low)
        .map(|(_, s)| s.ci_high - s.ci_low);
    let high_width = slices
        .iter()
        .find(|(t, _)| *t == regime::Tier::High)
        .map(|(_, s)| s.ci_high - s.ci_low);
    let verdict = match (low_width, high_width) {
        (Some(lw), Some(hw)) if hw > lw => format!(
            "**Converge/fan-out verdict:** HELD — the Low-sigma tier's 95% CI width \
             ({lw:.6}) is narrower than the High-sigma tier's ({hw:.6}), consistent with the \
             \"converge at low sigma / fan out at high sigma\" prediction (CI width used as \
             the dispersion proxy, since `stats::PairedStat` does not expose a per-bin raw \
             variance directly)."
        ),
        (Some(lw), Some(hw)) => format!(
            "**Converge/fan-out verdict:** DID NOT HOLD — the Low-sigma tier's 95% CI width \
             ({lw:.6}) is not narrower than the High-sigma tier's ({hw:.6}); the prediction is \
             not borne out by this run (CI width used as the dispersion proxy)."
        ),
        _ => "**Converge/fan-out verdict:** not computable — a Low or High sigma tier bin was \
              empty on this segment's seeds."
            .to_string(),
    };
    out.push_str(&verdict);
    out.push_str("\n\n");
    out
}

/// WHI-1248's closed-form analytic envelope for the `L=0` clairvoyant rung, re-derived for
/// variant (a) per the amendment (still an upper bound, but looser than variant (b)'s own
/// clean derivation): `sum(retail volume_y) x captured spread`, at 100% flow share (assume
/// every unit of retail flow the router could possibly send goes to this pool, not just its
/// actual share against the normalizer) and zero adverse selection (assume every unit fully
/// captures the nominal quoted spread, with no erosion from being picked off by informed
/// flow).
///
/// Under variant (a) (`target_x` pinned to the pair's own fixed `initial_x`, never
/// `reserve_x` itself), retail flow drifts `reserve_x` away from `target_x` over the course
/// of a simulation; `base = v0 + reserve_x - target_x` (`oracle.rs::oracle_swap`) then
/// departs from `v0`, and the curve's own marginal price at that drifted `reserve_x` moves
/// away from the flat `p_oracle * (1 +/- spread)` this envelope assumes. This is a standard
/// AMM inventory-skew effect: as inventory drifts toward one side, the curve's own effective
/// execution price for further same-direction flow degrades toward the oracle price,
/// capturing *less* than the full nominal spread on that flow — it can only ever reduce the
/// realized captured spread relative to this flat-spread idealization, never increase it.
/// The envelope therefore remains a valid upper bound for variant (a) too, just a looser one
/// than variant (b)'s (where `target_x` tracks `reserve_x` exactly, so there is no drift
/// term to begin with).
fn analytic_envelope_l0_upper_bound(configs: &[SimulationConfig], spread_bps: f64) -> f64 {
    if configs.is_empty() {
        return 0.0;
    }
    let spread_fraction = spread_bps / 10_000.0;
    let total: f64 = configs
        .iter()
        .map(|cfg| {
            let expected_retail_volume_y =
                f64::from(cfg.n_steps) * cfg.retail_arrival_rate * cfg.retail_mean_size;
            expected_retail_volume_y * spread_fraction
        })
        .sum();
    total / configs.len() as f64
}

/// Renders the `L=0` simulated-vs-envelope comparison the issue requires as a blocker check:
/// "the simulated L=0 number must sit below this envelope, or it's a blocker."
fn format_envelope_check(l0_avg_edge: f64, envelope: f64) -> String {
    let mut out = String::new();
    out.push_str("## L=0 analytic envelope check (WHI-1248)\n\n");
    out.push_str(&format!(
        "- Simulated `L=0` avg edge: {l0_avg_edge:.6}\n- Closed-form upper envelope: \
         {envelope:.6}\n"
    ));
    if l0_avg_edge <= envelope {
        out.push_str(&format!(
            "- **Sits below the envelope: PASS** ({l0_avg_edge:.6} <= {envelope:.6}).\n\n"
        ));
    } else {
        out.push_str(&format!(
            "- **BLOCKER: the simulated L=0 avg edge exceeds its own closed-form upper \
             envelope** ({l0_avg_edge:.6} > {envelope:.6}) — this is disqualifying per \
             WHI-1248's own acceptance criteria and means either the envelope's derivation or \
             the L=0 measurement itself has a bug that must be found before this number is \
             reported as real.\n\n"
        ));
    }
    out
}

/// Renders the fingerprint hardening-check trip table WHI-1248 requires: "a seed that trips
/// any check must be reported and re-run, never silently dropped from a paired statistic."
fn format_tripped_seeds(tripped: &[TrippedSeedOutcome]) -> String {
    let mut out = String::new();
    out.push_str("## Fingerprint hardening-check trips (WHI-1248)\n\n");
    if tripped.is_empty() {
        out.push_str(
            "Zero seeds tripped any fingerprint hardening check during Phase 1 (per-seed, \
             parallel) of this run.\n\n",
        );
        return out;
    }
    out.push_str(&format!(
        "{} seed(s) tripped a fingerprint hardening check during Phase 1 and were re-run \
         serially, one at a time (Phase 2), to recover a classified message — none was \
         silently dropped from the paired statistic without being named here.\n\n",
        tripped.len()
    ));
    out.push_str("| seed | classification | message |\n|---|---|---|\n");
    for t in tripped {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            t.seed,
            t.label,
            t.message.replace('|', "\\|").replace('\n', " ")
        ));
    }
    out.push('\n');
    out
}

pub fn run(args: CeilingArgs) -> anyhow::Result<()> {
    let lag = validate_cursor_and_lag(args.cursor, args.lag)?;
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
            anyhow::bail!("{ANCHORED_FIT_CONCENTRATION_CONFLICT}");
        }
        if args.spread_bps.is_some() {
            anyhow::bail!("--fit always re-fits spread_bps; do not pass --spread-bps alongside it");
        }
    } else if args.concentration.is_none() || args.spread_bps.is_none() {
        anyhow::bail!("either --fit or both --concentration and --spread-bps must be given");
    }

    let (reference_slug, reference_lib_path) = validate_reference_allowlist(&args.reference)?;
    let variant_slug = args.variant.slug();
    let cursor_mode: CursorMode = args.cursor.into();
    let cursor_slug_str = cursor_slug(args.cursor, lag);
    let stage = format!(
        "ceiling-{variant_slug}-{cursor_slug_str}-{segment_name}-orbic-oracle-vs-{reference_slug}"
    );

    if !args.no_report {
        crate::report::ensure_report_slot_free(Path::new(DEFAULT_REPORT_DIR), &stage)?;
    }

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
        run_fit(
            args.variant,
            args.concentration,
            &screening_configs,
            budget,
            cursor_mode,
            lag,
        )?
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
        cursor_mode,
        fingerprint_lag: lag,
    };

    let run_meta = CeilingRunMeta {
        stage: &stage,
        segment_name,
        n_sims: configs.len(),
        n_steps: base_config.n_steps,
        variant: args.variant,
        reference_slug: &reference_slug,
        cursor_slug: &cursor_slug_str,
    };

    let grid_levels = bench_config.grid().ok().map(|g| g.gbm_sigma_levels.clone());

    match cursor_mode {
        CursorMode::TradeTriggered => {
            let outcome = run_final_eval_catching_panics(params, &configs)?;

            match outcome {
                OracleBatchOutcome::Valid { batch, staleness } => {
                    let paired = stats::paired_stat(&batch.results, &reference_batch.results)?;
                    let staleness_agg = aggregate_staleness(&staleness);
                    let sigma_slices = slice_by_sigma_tier(
                        &base_config,
                        &batch.results,
                        &reference_batch.results,
                    )?;

                    println!(
                        "ceiling `{stage}`: mean edge diff (oracle - {reference_slug}) = \
                         {:.6} (95% CI [{:.6}, {:.6}], n={})",
                        paired.mean_diff, paired.ci_low, paired.ci_high, paired.n
                    );

                    if !args.no_report {
                        let extra = format_sigma_slices(grid_levels.as_deref(), &sigma_slices);
                        let path = write_ceiling_report(
                            run_meta,
                            &fitted,
                            FinalEvalSummary::Valid {
                                paired: &paired,
                                staleness: &staleness_agg,
                            },
                            &extra,
                        )?;
                        println!("wrote {}", path.display());
                    }

                    Ok(())
                }
                OracleBatchOutcome::Invalid(panic) => {
                    eprintln!(
                        "ceiling `{stage}`: the fitted point (concentration={:.4}, \
                         spread_bps={:.4}) panicked during final re-evaluation on \
                         `{segment_name}` — caught, not crashed: {}",
                        fitted.concentration, fitted.spread_bps, panic.message
                    );

                    if !args.no_report {
                        let path = write_ceiling_report(
                            run_meta,
                            &fitted,
                            FinalEvalSummary::Invalid(&panic),
                            "",
                        )?;
                        println!("wrote {} (marked INVALID — see the report)", path.display());
                    }

                    anyhow::bail!(
                        "the fitted point {:?} panicked during final re-evaluation on \
                         `{segment_name}` — valid on `screening`'s seeds does not guarantee \
                         valid on a different, larger seed set (docs/DESIGN.md \
                         §2.4/§2.5/WHI-1213; the Orbic family's own quantization jitter, \
                         WHI-1206, is exactly this). Do not trust this point without \
                         investigating why: {}",
                        (fitted.concentration, fitted.spread_bps),
                        panic.message,
                    );
                }
            }
        }
        CursorMode::Fingerprint => {
            let (oks, tripped) = run_fingerprint_final_eval(params, &configs)?;
            let (results, staleness) = reorder_oks_to_configs_order(&configs, oks);
            let tripped_section = format_tripped_seeds(&tripped);

            if results.is_empty() {
                let synthesized = PanicOutcome {
                    message: format!(
                        "all {} seed(s) tripped a fingerprint hardening check and none \
                         survived Phase 2's serial re-run — see the trip table below for each \
                         one's own classification and message",
                        tripped.len()
                    ),
                    recovered: true,
                    fingerprint_calls_since_advance: None,
                };
                eprintln!(
                    "ceiling `{stage}`: zero surviving seeds under the fingerprint cursor on \
                     `{segment_name}` — see the report"
                );
                if !args.no_report {
                    let path = write_ceiling_report(
                        run_meta,
                        &fitted,
                        FinalEvalSummary::Invalid(&synthesized),
                        &tripped_section,
                    )?;
                    println!("wrote {} (marked INVALID — see the report)", path.display());
                }
                anyhow::bail!(
                    "zero surviving seeds under the fingerprint cursor on `{segment_name}`: {}",
                    synthesized.message
                );
            }

            let surviving: HashSet<u64> = results.iter().map(|r| r.seed).collect();
            let filtered_reference = filter_batch_to_seeds(&reference_batch, &surviving);
            let paired = stats::paired_stat(&results, &filtered_reference.results)?;
            let staleness_agg = aggregate_staleness(&staleness);
            let sigma_slices =
                slice_by_sigma_tier(&base_config, &results, &filtered_reference.results)?;

            let mut extra = tripped_section;
            extra.push_str(&format_sigma_slices(grid_levels.as_deref(), &sigma_slices));
            if lag == 0 {
                let envelope = analytic_envelope_l0_upper_bound(&configs, fitted.spread_bps);
                let l0_avg_edge =
                    results.iter().map(|r| r.submission_edge).sum::<f64>() / results.len() as f64;
                extra.push_str(&format_envelope_check(l0_avg_edge, envelope));
            }

            println!(
                "ceiling `{stage}`: mean edge diff (oracle - {reference_slug}) = {:.6} (95% \
                 CI [{:.6}, {:.6}], n={}) [{} tripped seed(s), see report]",
                paired.mean_diff,
                paired.ci_low,
                paired.ci_high,
                paired.n,
                tripped.len()
            );

            if !args.no_report {
                let path = write_ceiling_report(
                    run_meta,
                    &fitted,
                    FinalEvalSummary::Valid {
                        paired: &paired,
                        staleness: &staleness_agg,
                    },
                    &extra,
                )?;
                println!("wrote {}", path.display());
            }

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_message_recovers_a_str_payload_verbatim() {
        let payload: Box<dyn std::any::Any + Send> = Box::new("boom");
        let (message, recovered) = panic_message(&*payload);
        assert_eq!(message, "boom");
        assert!(recovered);
    }

    #[test]
    fn panic_message_recovers_a_string_payload_verbatim() {
        let payload: Box<dyn std::any::Any + Send> = Box::new("boom".to_string());
        let (message, recovered) = panic_message(&*payload);
        assert_eq!(message, "boom");
        assert!(recovered);
    }

    #[test]
    fn panic_message_synthesizes_a_readable_message_for_a_non_string_payload() {
        // WHI-1248 amendment scope item #4: the original downcast returned `None` here,
        // which `catch_panicking` turned into a fixed, uninformative placeholder. The fix
        // must produce a message that is both non-empty and names the payload's own type
        // rather than a generic string.
        let payload: Box<dyn std::any::Any + Send> = Box::new(42_i32);
        let (message, recovered) = panic_message(&*payload);
        assert!(!recovered);
        assert!(
            message.contains("non-string panic payload") && message.contains("type_id"),
            "expected a readable, type-naming message, got: {message}"
        );
    }

    #[test]
    fn validate_cursor_and_lag_accepts_trade_triggered_with_no_lag() {
        assert_eq!(
            validate_cursor_and_lag(CursorArg::TradeTriggered, None).unwrap(),
            0
        );
    }

    #[test]
    fn validate_cursor_and_lag_rejects_trade_triggered_with_a_lag() {
        let err = validate_cursor_and_lag(CursorArg::TradeTriggered, Some(1)).unwrap_err();
        assert!(err.to_string().contains("only meaningful alongside"));
    }

    #[test]
    fn validate_cursor_and_lag_requires_lag_for_fingerprint() {
        let err = validate_cursor_and_lag(CursorArg::Fingerprint, None).unwrap_err();
        assert!(err.to_string().contains("requires --lag"));
    }

    #[test]
    fn validate_cursor_and_lag_accepts_fingerprint_lag_0_and_1() {
        assert_eq!(
            validate_cursor_and_lag(CursorArg::Fingerprint, Some(0)).unwrap(),
            0
        );
        assert_eq!(
            validate_cursor_and_lag(CursorArg::Fingerprint, Some(1)).unwrap(),
            1
        );
    }

    #[test]
    fn validate_cursor_and_lag_rejects_out_of_scope_lag_values() {
        let err = validate_cursor_and_lag(CursorArg::Fingerprint, Some(5)).unwrap_err();
        assert!(err.to_string().contains("out of scope"));
        let err = validate_cursor_and_lag(CursorArg::Fingerprint, Some(25)).unwrap_err();
        assert!(err.to_string().contains("out of scope"));
    }

    #[test]
    fn classify_fingerprint_panic_recognizes_a_cursor_assertion() {
        let outcome = PanicOutcome {
            message: "WHI-1248 fingerprint cursor hardening check (a): mismatch".to_string(),
            recovered: true,
            fingerprint_calls_since_advance: None,
        };
        assert_eq!(classify_fingerprint_panic(&outcome), "CursorAssertion");
    }

    #[test]
    fn classify_fingerprint_panic_recognizes_a_suspected_early_advance() {
        let outcome = PanicOutcome {
            message: "submission shape violation during buy: price moved".to_string(),
            recovered: true,
            fingerprint_calls_since_advance: Some(3),
        };
        assert_eq!(
            classify_fingerprint_panic(&outcome),
            "SuspectedEarlyAdvance"
        );
    }

    #[test]
    fn classify_fingerprint_panic_falls_back_to_other() {
        let outcome = PanicOutcome {
            message: "attempt to divide by zero".to_string(),
            recovered: true,
            fingerprint_calls_since_advance: None,
        };
        assert_eq!(classify_fingerprint_panic(&outcome), "Other");

        // A shape-violation-looking message with a large or missing call count is not
        // attributed to an early advance either.
        let outcome = PanicOutcome {
            message: "submission shape violation during buy: price moved".to_string(),
            recovered: true,
            fingerprint_calls_since_advance: Some(1_000),
        };
        assert_eq!(classify_fingerprint_panic(&outcome), "Other");
    }

    #[test]
    fn reorder_oks_to_configs_order_restores_original_order_and_drops_missing_seeds() {
        let base = SimulationConfig {
            seed: 1,
            ..SimulationConfig::default()
        };
        let configs: Vec<SimulationConfig> = (1..=3)
            .map(|s| {
                let mut c = base.clone();
                c.seed = s;
                c
            })
            .collect();
        let stale = StalenessSummary {
            n: 1,
            mean: 0.0,
            p50: 0.0,
            p95: 0.0,
            max: 0.0,
        };
        // Seed 2 is missing (as if it never survived); seeds 3 and 1 arrive out of order.
        let oks = vec![
            (
                SimResult {
                    seed: 3,
                    submission_edge: 0.3,
                },
                stale,
            ),
            (
                SimResult {
                    seed: 1,
                    submission_edge: 0.1,
                },
                stale,
            ),
        ];
        let (results, staleness) = reorder_oks_to_configs_order(&configs, oks);
        assert_eq!(
            results.iter().map(|r| r.seed).collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(staleness.len(), 2);
    }

    #[test]
    fn filter_batch_to_seeds_keeps_only_the_given_seeds_in_original_order() {
        let batch = BatchResult::from_results(vec![
            SimResult {
                seed: 1,
                submission_edge: 0.1,
            },
            SimResult {
                seed: 2,
                submission_edge: 0.2,
            },
            SimResult {
                seed: 3,
                submission_edge: 0.3,
            },
        ]);
        let seeds: HashSet<u64> = [1, 3].into_iter().collect();
        let filtered = filter_batch_to_seeds(&batch, &seeds);
        assert_eq!(
            filtered.results.iter().map(|r| r.seed).collect::<Vec<_>>(),
            vec![1, 3]
        );
    }

    #[test]
    fn analytic_envelope_l0_upper_bound_is_zero_for_zero_spread() {
        let cfg = SimulationConfig::default();
        assert_eq!(analytic_envelope_l0_upper_bound(&[cfg], 0.0), 0.0);
    }

    #[test]
    fn analytic_envelope_l0_upper_bound_is_positive_for_positive_spread() {
        let cfg = SimulationConfig::default();
        let envelope = analytic_envelope_l0_upper_bound(&[cfg], 20.0);
        assert!(envelope > 0.0);
    }

    #[test]
    fn format_tripped_seeds_reports_zero_trips_explicitly() {
        let out = format_tripped_seeds(&[]);
        assert!(out.contains("Zero seeds tripped"));
    }

    #[test]
    fn format_tripped_seeds_lists_every_seed_with_its_classification() {
        let tripped = vec![TrippedSeedOutcome {
            seed: 42,
            label: "CursorAssertion",
            message: "boom".to_string(),
        }];
        let out = format_tripped_seeds(&tripped);
        assert!(out.contains("42"));
        assert!(out.contains("CursorAssertion"));
        assert!(out.contains("boom"));
    }
}
