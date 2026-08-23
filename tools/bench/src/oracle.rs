//! The ceiling lane's host-side "oracle re-anchor" curve (WHI-1247): a concentrated,
//! spread-bearing curve that quotes directly off a replayed GBM fair-price path rather than
//! off its own reserves — the "price re-anchor the arbitrageur cannot front-run" this issue
//! measures. Lives in `tools/bench`, not a `strategies/**` `lib.rs`: it is never compiled to
//! BPF, never submittable, and out of competition end to end (`ceilings/README.md`).
//!
//! Structurally this mirrors `telemetry.rs`'s own pattern exactly (WHI-1247 step 4): a
//! process-global, `Mutex`-guarded set of "batch-constant" statics installed once,
//! single-threaded, before a parallel batch (here: which `OracleVariant`, the fitted
//! `CONCENTRATION`/`SPREAD_BPS`, and — WHI-1248 — which cursor rung and its fixed lag), plus
//! a `thread_local!` block of genuinely per-simulation state (the replayed price path, the
//! trade-triggered cursor, the captured anchor, this simulation's staleness samples, and —
//! WHI-1248 — the exact-step fingerprint targets/cursor) reset around each
//! `engine::run_simulation_native` call. `oracle_swap`/`oracle_after_swap` are bare `fn`
//! pointers (`SwapFn`/`AfterSwapFn` from `crates/executor/src/native.rs` carry no captured
//! state at all), so these statics are the only channel either side of this module has to
//! the other.
//!
//! WHI-1248 adds a second cursor rung alongside WHI-1247's trade-triggered one: an
//! exact-step "fingerprint" cursor that advances only when a `compute_swap` call's own
//! `(side, input_amount)` matches the *real* arbitrageur's own next-step probe,
//! reconstructed host-side by replaying `Pcg64::seed_from_u64(cfg.seed + 2)` through the
//! identical `LogNormal` construction `crates/sim/src/arbitrageur.rs::Arbitrageur::new` uses
//! (`arbitrageur.rs:47-60`). This is the deployment-faithful analogue of a fixed re-anchor
//! lag (`L` steps stale), not a diagnostic replacement for the trade-triggered rung — both
//! rungs coexist, selected per batch via [`OracleParams::cursor_mode`].

use std::cell::{Cell, RefCell};
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;

use prop_amm_executor::{AfterSwapFn, SwapFn};
use prop_amm_shared::config::SimulationConfig;
use prop_amm_shared::instruction::{decode_after_swap, decode_instruction};
use prop_amm_shared::nano::{f64_to_nano, nano_to_f64, NANO_SCALE_F64};
use prop_amm_shared::normalizer;
use prop_amm_shared::result::{BatchResult, SimResult};
use prop_amm_sim::engine;
use prop_amm_sim::price_process::GBMPriceProcess;
use rand::SeedableRng;
use rand_distr::{Distribution, LogNormal};
use rand_pcg::Pcg64;
use rayon::prelude::*;

/// Which of WHI-1247 step 3's two `target_x` variants is installed. `Anchored` (the
/// headline) pins `target_x` to the pair's own starting `initial_x`; `Floating` (the
/// degenerate diagnostic, never a result) re-reads `target_x = reserve_x` on every call, so
/// `base` is identically `v0` and the curve can never earn a directional edge from reserve
/// drift alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OracleVariant {
    Anchored = 0,
    Floating = 1,
}

/// Which oracle-cursor rung is installed — a second batch-constant axis alongside
/// [`OracleVariant`], not a replacement for it: both rungs share the exact same
/// `oracle_swap` pricing math above, differing only in which index of the replayed price
/// path a call reads from and how that index advances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CursorMode {
    /// WHI-1247 step 5's own rung: the cursor is the last *executed* trade's own step,
    /// moved by [`oracle_after_swap`]. Has its own staleness mechanic
    /// ([`StalenessSummary`]); [`OracleParams::fingerprint_lag`] is not read under this mode.
    TradeTriggered = 0,
    /// WHI-1248's own rung: the cursor advances only when a `compute_swap` call's own
    /// `(side, input_amount)` matches the real arbitrageur's own next-step probe — see the
    /// module doc comment. `oracle_swap`'s price lookup then reads
    /// `fingerprint_cursor.saturating_sub(fingerprint_lag)`, so `fingerprint_lag == 0` is the
    /// `L=0` clairvoyant diagnostic rung and `fingerprint_lag == 1` is the `L=1` headline
    /// deployment rung (`L in {5, 25}` is out of scope for this issue).
    Fingerprint = 1,
}

/// This rung's frozen point plus which variant/cursor it was fitted/measured under
/// (WHI-1247 steps 3, 10; WHI-1248) — installed once, batch-constant, before a parallel
/// batch starts.
#[derive(Debug, Clone, Copy)]
pub struct OracleParams {
    pub variant: OracleVariant,
    /// `v0 = target_x * concentration` — how much deeper the virtual curve is than the
    /// pair's real starting reserves (>= 1.0; `1.0` degenerates to an unconcentrated curve).
    pub concentration: f64,
    /// The spread this rung charges around the oracle price, in basis points (this issue's
    /// own new axis — WHI-1247 step 2, recorded as an adaptation in
    /// `ceilings/C-orbic-oracle/NOTES.md`).
    pub spread_bps: f64,
    /// WHI-1248: which cursor rung. Every call site states this explicitly — no `Default`
    /// impl is provided, since silently defaulting to one rung over the other in a struct
    /// literal is exactly the kind of ambiguity this lane's own honesty constraints forbid.
    pub cursor_mode: CursorMode,
    /// The fixed re-anchor lag, in steps, read only when `cursor_mode == Fingerprint` (`0`
    /// for the `L=0` diagnostic rung, `1` for the `L=1` headline rung). Ignored — but still
    /// present, for one uniform struct shape across both cursor modes — under
    /// `TradeTriggered`.
    pub fingerprint_lag: u64,
}

// --- Constants duplicated from crates/sim/src/arbitrageur.rs (private there; WHI-1248's
// fingerprint replay must reconstruct the real arbitrageur's own probe values bit-for-bit,
// which needs these same three constants — kept under an `FP_` prefix so a future
// `arbitrageur.rs` change is easy to grep for here too). ---
const FP_MIN_INPUT: f64 = 0.001;
const FP_MIN_ARB_NOTIONAL_Y: f64 = 0.01;
const FP_MAX_INPUT_AMOUNT: f64 = (u64::MAX as f64 / NANO_SCALE_F64) * 0.999_999;

// --- Batch-constant state (process-global; mirrors telemetry.rs's REAL_AFTER_SWAP/CALL_LOCK) ---

static VARIANT: AtomicU8 = AtomicU8::new(OracleVariant::Anchored as u8);
static CONCENTRATION_BITS: AtomicU64 = AtomicU64::new(0);
static SPREAD_BPS_BITS: AtomicU64 = AtomicU64::new(0);
static CURSOR_MODE_BITS: AtomicU8 = AtomicU8::new(CursorMode::TradeTriggered as u8);
static FINGERPRINT_LAG: AtomicU64 = AtomicU64::new(0);

/// Held for one call's entire install-through-collect duration — guards concurrent *calls*
/// to [`run_batch`]/[`run_batch_fingerprint_checked`] (e.g. two tests in the same binary),
/// not the internal rayon parallelism within one call. Exactly `telemetry.rs`'s own
/// `CALL_LOCK` role, under a name specific to this module's own statics.
static PARAMS_LOCK: Mutex<()> = Mutex::new(());

fn install_params(params: OracleParams) {
    VARIANT.store(params.variant as u8, Ordering::Relaxed);
    CONCENTRATION_BITS.store(params.concentration.to_bits(), Ordering::Relaxed);
    SPREAD_BPS_BITS.store(params.spread_bps.to_bits(), Ordering::Relaxed);
    CURSOR_MODE_BITS.store(params.cursor_mode as u8, Ordering::Relaxed);
    FINGERPRINT_LAG.store(params.fingerprint_lag, Ordering::Relaxed);
}

fn current_variant() -> OracleVariant {
    match VARIANT.load(Ordering::Relaxed) {
        1 => OracleVariant::Floating,
        _ => OracleVariant::Anchored,
    }
}

fn current_concentration() -> f64 {
    f64::from_bits(CONCENTRATION_BITS.load(Ordering::Relaxed))
}

fn current_spread_bps() -> f64 {
    f64::from_bits(SPREAD_BPS_BITS.load(Ordering::Relaxed))
}

fn current_cursor_mode() -> CursorMode {
    match CURSOR_MODE_BITS.load(Ordering::Relaxed) {
        1 => CursorMode::Fingerprint,
        _ => CursorMode::TradeTriggered,
    }
}

fn current_fingerprint_lag() -> u64 {
    FINGERPRINT_LAG.load(Ordering::Relaxed)
}

// --- Per-simulation state (thread_local; mirrors telemetry.rs's own COUNTERS) ---

thread_local! {
    /// This simulation's replayed fair-price path, `price_path[step] == fair_price` at
    /// `engine.rs`'s loop iteration `step` — built once per simulation via
    /// [`seed_thread_local`] from the *same* `GBMPriceProcess::new(...)`/`.step()` sequence
    /// `crates/sim/src/engine.rs::run_sim_inner` draws, so the replay is bit-identical to
    /// what the engine actually saw (Pcg64's stream is fully determined by the seed).
    static PRICE_PATH: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
    /// The oracle index the trade-triggered cursor currently reads from — WHI-1247's own
    /// rung (step 5): the last `step` seen in `after_swap`, so a quote between trades keeps
    /// reading the price as of the last *executed* trade, not the live fair price. Starts at
    /// `0`, which is already the exact right price for the very first quote: `engine.rs`'s
    /// loop draws `fair_price = price.step()` before anything trades at `step == 0`, and
    /// that is exactly `price_path[0]`.
    static CURSOR: RefCell<u64> = const { RefCell::new(0) };
    /// `target_x` for [`OracleVariant::Anchored`] — this simulation's actual starting X
    /// reserve (`cfg.initial_x`), captured once per simulation rather than assumed constant:
    /// `oracle_swap` is a bare `fn(&[u8]) -> u64` with no other channel to the
    /// `SimulationConfig` it's being run under.
    static ANCHOR_X: RefCell<f64> = const { RefCell::new(0.0) };
    /// Steps-since-last-executed-trade, recorded once per `oracle_after_swap` call, read out
    /// and reset by [`take_staleness_summary`] at this simulation's end.
    static STALENESS_SAMPLES: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    /// WHI-1248: this simulation's fingerprint targets, `fp_targets[step] == (buy_probe_nano,
    /// sell_probe_nano)` — the exact nano-quantized `(start_y, start_x)` the real
    /// arbitrageur's own RNG stream would draw as its buy-side / sell-side search's very
    /// first evaluated point against the submission AMM at loop iteration `step`. Built once
    /// per simulation by [`seed_thread_local`], only when [`current_cursor_mode`] is
    /// [`CursorMode::Fingerprint`] (left empty otherwise — checking `.is_empty()` is cheaper
    /// than threading an extra bool through every call site). A step whose own `fair_price`
    /// was non-finite or non-positive gets the sentinel `(0, 0)`, matching the real
    /// arbitrageur's own behavior of skipping its RNG draw (and therefore issuing no probe
    /// at all against the submission AMM) on such a step — `0` can never collide with a real
    /// probe, since every real probe is floored at `FP_MIN_INPUT`/`FP_MIN_ARB_NOTIONAL_Y` and
    /// then nano-quantized, always `> 0`.
    static FP_TARGETS: RefCell<Vec<(u64, u64)>> = const { RefCell::new(Vec::new()) };
    /// The fingerprint cursor itself (WHI-1248) — monotone, advanced only by
    /// [`maybe_advance_fingerprint_cursor`] on a genuine probe match, never read directly by
    /// `oracle_swap`'s pricing math without first subtracting [`current_fingerprint_lag`].
    static FP_CURSOR: RefCell<u64> = const { RefCell::new(0) };
    /// Cleared to `false` on every fingerprint-cursor advance; set `true` the first time a
    /// `side == 1` (sell-X) call is seen since the last advance. Gates a fingerprint match
    /// (hardening check (c), WHI-1248): a step's own buy-side search (many `side == 0`
    /// calls, evaluated in full before that step's own sell-side search begins —
    /// `Arbitrageur::execute_arb`'s `Self::best_candidate(self.plan_arb_buy_x(..),
    /// self.plan_arb_sell_x(..))` evaluates its first argument, the entire buy-side search,
    /// before its second) can coincidentally collide with the *next* step's buy-side probe
    /// under a small-sigma GBM path where adjacent steps' search ranges overlap; requiring at
    /// least one `side == 1` call since the last advance blocks that specific false match,
    /// since a genuine step transition requires this step's own sell-side search (or a later
    /// retail trade) to have run first. Does not protect against every conceivable
    /// coincidental collision (e.g. within a step's own sell-side search evaluating a value
    /// that happens to equal the next step's own sell probe) — only the specific
    /// buy-before-sell asymmetry the issue calls out.
    static SEEN_SIDE1_SINCE_ADVANCE: Cell<bool> = const { Cell::new(false) };
    /// `oracle_swap` calls since the last fingerprint-cursor advance — read at panic time by
    /// [`fingerprint_panic_diagnostics`] to help `commands/ceiling.rs` distinguish a
    /// `curve_checks.rs` panic caused by an early, wrong cursor advance (a small count, near
    /// `BRACKET_MAX_STEPS + GOLDEN_MAX_ITERS`-ish) from a panic reached only after many more
    /// calls (more consistent with a genuine shape violation, not this lane's own cursor
    /// bug).
    static CALLS_SINCE_FP_ADVANCE: Cell<u64> = const { Cell::new(0) };
}

/// WHI-1248: replays the real arbitrageur's own RNG stream
/// (`Pcg64::seed_from_u64(cfg.seed.wrapping_add(2))`, through the identical `LogNormal`
/// construction `crates/sim/src/arbitrageur.rs::Arbitrageur::new` uses — `arbitrageur.rs:53-58`
/// for `sigma`/`mu_ln`, `arbitrageur.rs:34-38` for `min_buy_input_y`/`min_sell_input_x`) to
/// reconstruct, per step, the exact nano-quantized `(buy_probe, sell_probe)` pair the real
/// arbitrageur's own search would evaluate first that step against the submission AMM
/// specifically (the sole RNG draw in `execute_arb`'s non-normalizer branch —
/// `arbitrageur.rs:62-89`). `path` must already be this simulation's replayed fair-price
/// path (`path[step] == fair_price` at `engine.rs`'s loop iteration `step`) — the same one
/// [`seed_thread_local`] builds for [`PRICE_PATH`].
///
/// Known limitations (both intentional, not bugs — WHI-1248):
///
/// 1. If `fair_price` is ever non-finite or `<= 0.0` at some step, the real arbitrageur's own
///    `execute_arb` returns `None` *before* drawing from the RNG at all (`arbitrageur.rs:63-65`)
///    — no probe is issued against the submission AMM that step. This replay matches that by
///    skipping its own draw and recording the sentinel `(0, 0)`, so the fingerprint cursor
///    can never advance into such a step via a probe match (consistent with the real
///    arbitrageur never producing one); the GBM price process this repo's config space uses
///    never produces such a value in practice, so this is a defensive match, not an observed
///    case.
/// 2. `crates/sim/src/amm.rs::BpfAmm::quote_buy_x`/`quote_sell_x` — the functions that
///    actually invoke `compute_swap` — bail with `0.0` *without* calling `compute_swap` at
///    all when `reserve_x <= MIN_RESERVE || reserve_y <= MIN_RESERVE`. In that narrow,
///    near-zero-reserve state, the real arbitrageur's probe for that step/side never reaches
///    `oracle_swap`, so the fingerprint cursor cannot advance via that mechanism for that
///    step. This is a genuine, if narrow, gap in the "monotone match-next-probe" design; the
///    per-trade and terminal hardening checks below exist in part to surface exactly this
///    kind of stall rather than let it pass silently.
/// 3. **Measured, traced, and load-bearing for this issue's own conclusion (not narrow):**
///    floor-clamping makes the "match against `FP_TARGETS[next]`" design non-injective, and
///    the real arbitrageur's own search *routinely* re-probes the floor value it clamps to
///    regardless of which step is actually in progress — so a step whose own fingerprint
///    target happens to floor-clamp becomes a near-guaranteed false-match trap for every
///    other nearby step's own routine floor probe, not a rare coincidence. Traced end to end
///    on seed `2_000_011` (`validation` segment, `concentration=94.33, spread_bps=102.0`,
///    variant (a), `n_steps=10_000`): step 3201's own sell-side search issued a probe of
///    exactly `f64_to_nano(0.001) = 1_000_000` (`FP_MIN_INPUT`) at
///    `oracle.rs`-call-index 121973, 26 calls after step 3201's own genuine buy-side match —
///    this is `arbitrageur.rs::golden_section_max`'s own first internal evaluation,
///    `objective(left)` where `left == lo`, and `lo` is very often handed back unchanged
///    from `bracket_maximum`'s early-return paths (`if mid_value <= 0.0 { return (lo, mid);
///    }`, `if hi_value <= mid_value || hi >= max_input { return (lo, hi); }` — both return
///    before the `for` loop's own first `lo = mid;` reassignment), so `lo == min_input`, the
///    fixed floor constant (`min_sell_input_x(fair_price) == FP_MIN_INPUT == 0.001` whenever
///    `fair_price > MIN_ARB_NOTIONAL_Y / MIN_INPUT == 10`, true for essentially this whole
///    run — `fair_price[3202] == 116.58`). That probe happened to equal step 3202's *own*
///    fingerprint `sell_target`, which had independently floor-clamped to the same constant
///    (its own `start_y ~= 0.1118` implies `start_x = start_y/fair_price ~= 0.00096 <
///    FP_MIN_INPUT`) — two of the 10,000 steps in this one simulation floor-clamped their
///    sell target to this exact value, confirmed by a direct count over `FP_TARGETS`. The
///    cursor advanced from 3201 to 3202 one call early, and the per-trade hardening check
///    correctly caught the resulting mismatch (`3202` vs. the executed trade's own `3201`)
///    at the very next `after_swap`. [`SEEN_SIDE1_SINCE_ADVANCE`]'s own doc comment already
///    named this exact residual case ("within a step's own sell-side search evaluating a
///    value that happens to equal the next step's own sell probe") as unprotected — measurement
///    shows it is the *dominant* failure mode, not a residual one: a ~50%+ per-seed
///    hardening-check trip rate was measured on `validation`, roughly nine orders of
///    magnitude above this design's own derived expectation (~1e-10 per comparison from
///    treating a match as a random continuous-value collision). The buy side has the exact
///    same structural exposure (`min_buy_input_y() == FP_MIN_ARB_NOTIONAL_Y == 0.01`,
///    unconditionally, at every step, for the identical reason). Because a floor-degenerate
///    probe is deterministic given the RNG stream (not a flaky, re-runnable artifact), and
///    because which seeds trip is correlated with the RNG's own low-draw episodes rather
///    than independent of the outcome being measured, averaging any paired statistic over
///    only the seeds that happened not to trip is a selection-biased estimate, not a smaller-
///    n version of the same estimate — this repo's decision (`ceilings/C-orbic-oracle/NOTES.md`
///    § WHI-1248) is therefore to close the `L=1`/`L=0` fingerprint rungs as a documented
///    negative/method-level result rather than report such a number. The mechanism this
///    replay reconstructs is bit-exact and independently verified against
///    `arbitrageur.rs:53-58`'s clamps; what is unsound is the *matching design itself*
///    ("a probe hitting `FP_TARGETS[next]` uniquely identifies arrival at step `next`") once
///    floor-clamping is in play, because the search algorithm's own routine boundary
///    evaluation and a step's own genuine draw are, at the interface this replay observes,
///    indistinguishable events carrying the identical value.
fn build_fingerprint_targets(cfg: &SimulationConfig, path: &[f64]) -> Vec<(u64, u64)> {
    let sigma = cfg.retail_size_sigma.max(0.01);
    let mu_ln = cfg.retail_mean_size.max(0.01).ln() - 0.5 * sigma * sigma;
    let mut rng = Pcg64::seed_from_u64(cfg.seed.wrapping_add(2));
    let dist = LogNormal::new(mu_ln, sigma).expect("sigma > 0 by construction (max(0.01))");

    let min_buy_input = FP_MIN_INPUT.max(FP_MIN_ARB_NOTIONAL_Y);

    path.iter()
        .map(|&fair_price| {
            if !fair_price.is_finite() || fair_price <= 0.0 {
                // Matches `execute_arb`'s own early return — no RNG draw, no probe.
                return (0, 0);
            }
            let start_y = dist
                .sample(&mut rng)
                .max(FP_MIN_INPUT)
                .max(min_buy_input)
                .min(FP_MAX_INPUT_AMOUNT);
            let min_sell_input = FP_MIN_INPUT.max(FP_MIN_ARB_NOTIONAL_Y / fair_price.max(1e-9));
            let start_x = (start_y / fair_price.max(1e-9))
                .max(min_sell_input)
                .min(FP_MAX_INPUT_AMOUNT);
            (f64_to_nano(start_y), f64_to_nano(start_x))
        })
        .collect()
}

/// Resets this thread's per-simulation state for a fresh call to `engine::run_simulation_native`
/// under `cfg` — the replayed price path, both cursors, captured anchor, staleness samples,
/// and (WHI-1248, only under [`CursorMode::Fingerprint`]) the fingerprint targets. Call
/// immediately before that call, on the same thread, mirroring
/// `telemetry.rs::take_thread_counters`'s own "resets at a known simulation boundary".
pub fn seed_thread_local(cfg: &SimulationConfig) {
    let mut proc = GBMPriceProcess::new(
        cfg.initial_price,
        cfg.gbm_mu,
        cfg.gbm_sigma,
        cfg.gbm_dt,
        cfg.seed,
    );
    let path: Vec<f64> = (0..cfg.n_steps).map(|_| proc.step()).collect();

    if current_cursor_mode() == CursorMode::Fingerprint {
        let targets = build_fingerprint_targets(cfg, &path);
        FP_TARGETS.with(|t| *t.borrow_mut() = targets);
    } else {
        FP_TARGETS.with(|t| t.borrow_mut().clear());
    }
    FP_CURSOR.with(|c| *c.borrow_mut() = 0);
    SEEN_SIDE1_SINCE_ADVANCE.with(|s| s.set(false));
    CALLS_SINCE_FP_ADVANCE.with(|c| c.set(0));

    PRICE_PATH.with(|p| *p.borrow_mut() = path);
    CURSOR.with(|c| *c.borrow_mut() = 0);
    ANCHOR_X.with(|a| *a.borrow_mut() = cfg.initial_x);
    STALENESS_SAMPLES.with(|s| s.borrow_mut().clear());
}

/// This rung's staleness distribution for one simulation (WHI-1247 step 5): mean, median,
/// p95, and max steps-since-last-executed-trade across every trade this simulation routed
/// to the submission AMM, plus the sample count. Collected identically under both cursor
/// modes (WHI-1248) — [`oracle_after_swap`] always updates the trade-triggered cursor and
/// its staleness samples regardless of which cursor is actually driving `oracle_swap`'s own
/// price lookup, so this remains a reportable diagnostic even for a fingerprint-mode run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StalenessSummary {
    pub n: usize,
    pub mean: f64,
    pub p50: f64,
    pub p95: f64,
    pub max: f64,
}

/// `pub(crate)` (not just private) so `commands/ceiling.rs`'s own batch-level aggregation
/// (`aggregate_staleness`) can compute a genuine percentile of the per-simulation summaries
/// — e.g. the 95th percentile of every simulation's own mean staleness — without a third
/// copy of this five-line algorithm. `sorted` must already be sorted ascending; unsorted
/// input silently produces a meaningless answer rather than panicking, exactly like the two
/// existing call sites in this file already relied on.
pub(crate) fn nearest_rank(sorted: &[f64], fraction: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * fraction).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn summarize_staleness(samples: &[u64]) -> StalenessSummary {
    if samples.is_empty() {
        return StalenessSummary {
            n: 0,
            mean: 0.0,
            p50: 0.0,
            p95: 0.0,
            max: 0.0,
        };
    }
    let mut sorted: Vec<f64> = samples.iter().map(|&s| s as f64).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("staleness samples are finite"));
    let n = sorted.len();
    let mean = sorted.iter().sum::<f64>() / n as f64;
    StalenessSummary {
        n,
        mean,
        p50: nearest_rank(&sorted, 0.50),
        p95: nearest_rank(&sorted, 0.95),
        max: *sorted.last().expect("checked non-empty above"),
    }
}

/// Reads out and resets this thread's staleness samples plus the rest of the per-simulation
/// state — call immediately after the matching `engine::run_simulation_native` call
/// [`seed_thread_local`] set up for, on the same thread.
pub fn take_staleness_summary() -> StalenessSummary {
    let samples = STALENESS_SAMPLES.with(|s| std::mem::take(&mut *s.borrow_mut()));
    PRICE_PATH.with(|p| p.borrow_mut().clear());
    CURSOR.with(|c| *c.borrow_mut() = 0);
    summarize_staleness(&samples)
}

/// WHI-1248 hardening check (b): call once, after a simulation completes, on the same thread
/// [`seed_thread_local`] set up for it — under [`CursorMode::Fingerprint`], the fingerprint
/// cursor must have reached the very last step by the time the simulation ends. Panics
/// (rather than returning a `Result`) so it composes with the same `catch_unwind`-based
/// "never silently dropped" machinery as hardening check (a) below and any genuine
/// `curve_checks.rs` shape violation — one uniform failure channel for
/// `commands/ceiling.rs`'s Phase 2 re-run to recover a message from.
pub(crate) fn check_fingerprint_terminal(seed: u64, n_steps: u32) {
    let fp_cursor = FP_CURSOR.with(|c| *c.borrow());
    let expected = u64::from(n_steps).saturating_sub(1);
    assert_eq!(
        fp_cursor, expected,
        "WHI-1248 fingerprint cursor hardening check (b): seed {seed}'s fingerprint cursor \
         ({fp_cursor}) never reached the final step ({expected}) — the cursor stalled before \
         the simulation's own last step ever produced a matching probe"
    );
}

/// WHI-1248: read at panic time (from within `commands/ceiling.rs::catch_panicking`'s own
/// installed hook, on the same, still-panicking thread) to snapshot how many `oracle_swap`
/// calls had occurred since the fingerprint cursor's last advance. `None` whenever
/// [`CursorMode::Fingerprint`] is not the currently-installed mode — harmless to call
/// unconditionally from every `catch_panicking` call site, including the ones that only ever
/// run under [`CursorMode::TradeTriggered`].
pub(crate) fn fingerprint_panic_diagnostics() -> Option<u64> {
    if current_cursor_mode() != CursorMode::Fingerprint {
        return None;
    }
    Some(CALLS_SINCE_FP_ADVANCE.with(|c| c.get()))
}

/// WHI-1248's monotone advance rule, and hardening check (c) (the side-1 gate described on
/// [`SEEN_SIDE1_SINCE_ADVANCE`] above): on a genuine probe match for the *next* step, moves
/// [`FP_CURSOR`] forward by exactly one and resets the side-1 gate/call counter. Must run
/// (and, on a match, mutate `FP_CURSOR`) *before* `oracle_swap`'s own price lookup in the
/// same call — reading the lagged price only after a possible advance is what makes `L=0`
/// mean "this step's own price" and `L=1` mean "one step stale", not one extra step of lag
/// stacked on top of whichever advance this very call just produced.
fn maybe_advance_fingerprint_cursor(side: u8, input_amount: u64) {
    CALLS_SINCE_FP_ADVANCE.with(|c| c.set(c.get().saturating_add(1)));
    if side == 1 {
        SEEN_SIDE1_SINCE_ADVANCE.with(|s| s.set(true));
    }
    if side != 0 && side != 1 {
        return;
    }
    if !SEEN_SIDE1_SINCE_ADVANCE.with(|s| s.get()) {
        return;
    }

    let targets_len = FP_TARGETS.with(|t| t.borrow().len());
    let cursor = FP_CURSOR.with(|c| *c.borrow());
    let next = cursor + 1;
    if next as usize >= targets_len {
        return; // Already at (or past) the final step — no further target to match against.
    }

    let (buy_target, sell_target) = FP_TARGETS.with(|t| t.borrow()[next as usize]);
    let matched = match side {
        0 => input_amount == buy_target,
        1 => input_amount == sell_target,
        _ => unreachable!("side is 0 or 1 here — checked above"),
    };
    if matched {
        FP_CURSOR.with(|c| *c.borrow_mut() = next);
        SEEN_SIDE1_SINCE_ADVANCE.with(|s| s.set(false));
        CALLS_SINCE_FP_ADVANCE.with(|c| c.set(0));
    }
}

/// WHI-1247 step 2's Orbic port: `compute_swap` for the oracle-anchored, spread-bearing
/// curve. `side == 0` buys X from the pool (Y in, X out) and is quoted at
/// `p_oracle * (1 + spread)` (the pool sells X dear); `side == 1` sells X to the pool (X in,
/// Y out) and is quoted at `p_oracle * (1 - spread)` (the pool buys X cheap) — the marginal
/// price at `reserve_x == target_x` is `k / v0^2` on both sides, so this is exactly those
/// two target prices.
///
/// Only ever reads `reserve_x` in the pricing math itself (never `reserve_y`) — a
/// deliberately "virtual one-sided" curve whose price is driven purely by `reserve_x`'s
/// deviation from `target_x`, decoupled from the pool's actual Y reserves, consistent with
/// quoting directly off a re-anchored oracle rather than off its own book
/// (`ceilings/C-orbic-oracle/NOTES.md`). `reserve_y` is read only for the degenerate-input
/// guard, mirroring `crates/shared/src/normalizer.rs`'s own convention.
///
/// The upstream Orbic source's `_isTargetYLocked` 5%-move circuit breaker is deliberately
/// not ported (recorded, not silently dropped — `ceilings/C-orbic-oracle/NOTES.md`).
///
/// WHI-1248: under [`CursorMode::Fingerprint`], every call first runs
/// [`maybe_advance_fingerprint_cursor`] — *before* the price lookup below, per that
/// function's own doc comment — then the price lookup itself reads
/// `fingerprint_cursor.saturating_sub(fingerprint_lag)` instead of the trade-triggered
/// [`CURSOR`]. Under [`CursorMode::TradeTriggered`] this whole path is skipped and behavior
/// is byte-for-byte what WHI-1247 shipped.
pub fn oracle_swap(data: &[u8]) -> u64 {
    let (side, input_amount, reserve_x_nano, reserve_y_nano) = decode_instruction(data);
    if reserve_x_nano == 0 || reserve_y_nano == 0 || input_amount == 0 {
        return 0;
    }

    let cursor_mode = current_cursor_mode();
    if cursor_mode == CursorMode::Fingerprint {
        maybe_advance_fingerprint_cursor(side, input_amount);
    }

    let p_oracle = PRICE_PATH.with(|p| {
        let path = p.borrow();
        if path.is_empty() {
            return f64::NAN;
        }
        let cursor = match cursor_mode {
            CursorMode::TradeTriggered => CURSOR.with(|c| *c.borrow()),
            CursorMode::Fingerprint => {
                let fp = FP_CURSOR.with(|c| *c.borrow());
                fp.saturating_sub(current_fingerprint_lag())
            }
        } as usize;
        path[cursor.min(path.len() - 1)]
    });
    if !(p_oracle.is_finite() && p_oracle > 0.0) {
        return 0;
    }

    let concentration = current_concentration();
    if !(concentration.is_finite() && concentration >= 1.0) {
        return 0;
    }
    let spread = current_spread_bps() / 10_000.0;
    if !(spread.is_finite() && (0.0..1.0).contains(&spread)) {
        return 0;
    }

    let reserve_x = nano_to_f64(reserve_x_nano);
    let target_x = match current_variant() {
        OracleVariant::Anchored => ANCHOR_X.with(|a| *a.borrow()),
        OracleVariant::Floating => reserve_x,
    };
    if !(target_x.is_finite() && target_x > 0.0) {
        return 0;
    }

    let v0 = target_x * concentration;
    let base = v0 + reserve_x - target_x;
    if !(base.is_finite() && base > 0.0) {
        return 0;
    }

    let input = nano_to_f64(input_amount);
    // Round-3 review, standards finding #4: `side` used to be matched twice — once for
    // `price`, once for `out` — leaving the second match's `_ => return 0` unreachable,
    // since the first match already returned on anything outside `{0, 1}`. One match, each
    // arm computing its own `price`/`k`/`out` in order, drops the dead arm without changing
    // behavior.
    let out = match side {
        // Buy X from the pool: `dy` (Y) in, `dx` (X) out.
        0 => {
            let price = p_oracle * (1.0 + spread);
            if !(price.is_finite() && price > 0.0) {
                return 0;
            }
            let k = v0 * v0 * price;
            if !(k.is_finite() && k > 0.0) {
                return 0;
            }
            let denom = k / base + input;
            if !(denom.is_finite() && denom > 0.0) {
                return 0;
            }
            base - k / denom
        }
        // Sell X to the pool: `dx` (X) in, `dy` (Y) out.
        1 => {
            let price = p_oracle * (1.0 - spread);
            if !(price.is_finite() && price > 0.0) {
                return 0;
            }
            let k = v0 * v0 * price;
            if !(k.is_finite() && k > 0.0) {
                return 0;
            }
            let denom = base + input;
            if !(denom.is_finite() && denom > 0.0) {
                return 0;
            }
            k / base - k / denom
        }
        _ => return 0,
    };

    if !(out.is_finite() && out > 0.0) {
        return 0;
    }
    f64_to_nano(out)
}

/// WHI-1247 step 5's cursor update: on every executed trade against the submission AMM,
/// records how many steps elapsed since the last executed trade (staleness), then moves the
/// trade-triggered cursor to this trade's own step. Only ever installed as the *submission*
/// AMM's `after_swap` (never the normalizer's) — a trade routed to the normalizer must not
/// move this pool's own oracle cursor.
///
/// Always runs, and always updates [`CURSOR`]/[`STALENESS_SAMPLES`], regardless of
/// [`current_cursor_mode`] — WHI-1248's fingerprint rung still wants this trade-triggered
/// staleness figure reported as a diagnostic. Additionally, under
/// [`CursorMode::Fingerprint`], runs hardening check (a): by the time an executed trade's
/// own `after_swap` fires, the fingerprint cursor must already equal that trade's own
/// reported `step` — a mismatch means a probe match was missed (the cursor lagged behind) or
/// a false match advanced the cursor past where it should be.
pub fn oracle_after_swap(data: &[u8], _storage: &mut [u8]) {
    let (_side, _input_amount, _output_amount, _reserve_x, _reserve_y, step, _storage) =
        decode_after_swap(data);
    CURSOR.with(|c| {
        let mut cursor = c.borrow_mut();
        let staleness = step.saturating_sub(*cursor);
        STALENESS_SAMPLES.with(|s| s.borrow_mut().push(staleness));
        *cursor = step;
    });

    if current_cursor_mode() == CursorMode::Fingerprint {
        let fp_cursor = FP_CURSOR.with(|c| *c.borrow());
        assert_eq!(
            fp_cursor, step,
            "WHI-1248 fingerprint cursor hardening check (a): the fingerprint cursor \
             ({fp_cursor}) must already equal the executed trade's own step ({step}) by the \
             time that trade's after_swap fires — a mismatch means a probe match was missed \
             or a false match advanced the cursor to the wrong step"
        );
    }
}

fn native_pool() -> anyhow::Result<rayon::ThreadPool> {
    Ok(rayon::ThreadPoolBuilder::new()
        .num_threads(rayon::current_num_threads().min(8))
        .build()?)
}

/// The lane's own rayon-driven batch loop (WHI-1247 step 6): one `engine::run_simulation_native`
/// call per seed, against the fixed normalizer opponent — mirrors `telemetry.rs`'s own loop
/// shape, not `runner::run_batch_native`, so `--self-check` exercises exactly the loop shape
/// the real oracle measurement below uses. `submission_fn`/`submission_after_swap` are
/// whatever the caller wants run as "submission"; no oracle state is touched here, so no
/// lock is needed (used by `--self-check`, which drives `prop_amm_shared::normalizer::compute_swap`
/// as the submission — not the oracle).
pub fn run_batch_native_loop(
    submission_fn: SwapFn,
    submission_after_swap: Option<AfterSwapFn>,
    configs: &[SimulationConfig],
) -> anyhow::Result<BatchResult> {
    let pool = native_pool()?;
    let results: anyhow::Result<Vec<SimResult>> = pool.install(|| {
        configs
            .par_iter()
            .map(|config| {
                engine::run_simulation_native(
                    submission_fn,
                    submission_after_swap,
                    normalizer::compute_swap,
                    Some(normalizer::after_swap),
                    config,
                )
            })
            .collect()
    });
    Ok(BatchResult::from_results(results?))
}

/// The real oracle measurement (WHI-1247 steps 4-6): installs `params` batch-constant, then
/// drives the same rayon loop shape as [`run_batch_native_loop`] with [`oracle_swap`]/
/// [`oracle_after_swap`] as the submission, seeding and draining this module's own
/// thread-local state immediately around each per-seed call so it resets at a known
/// simulation boundary. Returns the batch result alongside each seed's staleness summary, in
/// the same order as `configs`.
///
/// Unchanged by WHI-1248: still one batch-level `catch_unwind` (via the caller's own
/// `run_catching_panics`/`run_final_eval_catching_panics` in `commands/ceiling.rs`) that
/// discards every seed on a single panic. That is fine for this function's two existing
/// callers (the `screening`-segment search/fit loop, and the `TradeTriggered`/`Floating`
/// final evaluation) but is exactly what [`run_batch_fingerprint_checked`] below exists to
/// avoid for the fingerprint rung's own final evaluation, where the issue requires every
/// tripped seed to be individually named and re-run, never silently dropped alongside
/// everyone else's valid results.
pub fn run_batch(
    params: OracleParams,
    configs: &[SimulationConfig],
) -> anyhow::Result<(BatchResult, Vec<StalenessSummary>)> {
    let _guard = PARAMS_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    install_params(params);

    let pool = native_pool()?;
    let pairs: anyhow::Result<Vec<(SimResult, StalenessSummary)>> = pool.install(|| {
        configs
            .par_iter()
            .map(|config| -> anyhow::Result<(SimResult, StalenessSummary)> {
                seed_thread_local(config);
                let result = engine::run_simulation_native(
                    oracle_swap,
                    Some(oracle_after_swap),
                    normalizer::compute_swap,
                    Some(normalizer::after_swap),
                    config,
                )?;
                if params.cursor_mode == CursorMode::Fingerprint {
                    check_fingerprint_terminal(config.seed, config.n_steps);
                }
                let staleness = take_staleness_summary();
                Ok((result, staleness))
            })
            .collect()
    });

    let (results, staleness): (Vec<SimResult>, Vec<StalenessSummary>) = pairs?.into_iter().unzip();
    Ok((BatchResult::from_results(results), staleness))
}

/// WHI-1248: one seed that panicked during [`run_batch_fingerprint_checked`]'s Phase 1 —
/// carries only the seed and its own [`SimulationConfig`] (`commands/ceiling.rs`'s Phase 2
/// re-runs exactly this one seed through the existing, unmodified [`run_batch`] plus its own
/// `catch_panicking` hook, serially and one at a time, to recover a full panic message).
/// Deliberately does *not* carry a message itself: capturing one race-free requires the
/// installed-hook machinery `commands/ceiling.rs::catch_panicking` already owns, and
/// duplicating a second copy of that machinery here (with its own panic-hook
/// install/restore) just to grab a message during the *parallel* Phase 1 pass would defeat
/// the whole point of running Phase 1 with a silent hook for speed.
pub struct FingerprintTrippedSeed {
    pub seed: u64,
    pub config: SimulationConfig,
}

/// Named alias for `run_batch_fingerprint_checked`'s return type — clippy's
/// `type_complexity` lint objects to the bare nested tuple-of-Vecs spelled out inline.
pub type FingerprintBatchOutcome = (
    Vec<(SimResult, StalenessSummary)>,
    Vec<FingerprintTrippedSeed>,
);

/// Phase 1 of WHI-1248's "never silently dropped" final-evaluation architecture: every seed
/// gets its *own* `catch_unwind`, on the same rayon worker thread that ran it, so one seed's
/// panic (a hardening-check assertion, or an early-fingerprint-advance-induced
/// `curve_checks.rs` panic) never destroys the other seeds' valid results — unlike
/// [`run_batch`]'s existing single, batch-level `catch_unwind` (which stays correct and
/// unchanged for its own two callers). Requires `params.cursor_mode ==
/// CursorMode::Fingerprint` (this is WHI-1248's own per-seed hardening path, not a
/// general-purpose replacement for `run_batch`); bails immediately otherwise.
///
/// Suppresses the default panic hook's stderr print for the duration — a hardening-check
/// assertion tripping (or a suspected early-fingerprint-advance `curve_checks.rs` panic) is
/// expected control flow during this phase, not a crash; the previous hook is always
/// restored before returning, on every path. `commands/ceiling.rs`'s own `catch_panicking`,
/// used by the caller's Phase 2, installs its *own* message-capturing hook per re-run
/// afterward — the two never run concurrently, since Phase 1 fully completes (and restores
/// the default hook) before this function returns.
///
/// Returns every seed that completed cleanly (its [`SimResult`] and trade-triggered-cursor
/// [`StalenessSummary`] — collected identically to [`run_batch`], since [`oracle_after_swap`]
/// always updates both regardless of which cursor prices off) plus every seed that panicked
/// (as a [`FingerprintTrippedSeed`]) — both, always, filtered out of `configs`' own order,
/// never one silently substituted for the other. A genuine (non-panic) `anyhow::Error` from
/// `engine::run_simulation_native` itself — a different failure class than a hardening-check
/// assertion or a shape-violation panic — is treated as a hard error and returned
/// immediately via `?`, not folded into the tripped-seed list: there is no established
/// "re-run and recover a message" story for a non-panic error the way there is for a panic,
/// so it surfaces loudly and immediately instead.
pub fn run_batch_fingerprint_checked(
    params: OracleParams,
    configs: &[SimulationConfig],
) -> anyhow::Result<FingerprintBatchOutcome> {
    if params.cursor_mode != CursorMode::Fingerprint {
        anyhow::bail!(
            "run_batch_fingerprint_checked requires OracleParams::cursor_mode == \
             CursorMode::Fingerprint — this is WHI-1248's own per-seed hardening path for the \
             fingerprint rung's final evaluation, not a general-purpose replacement for \
             run_batch"
        );
    }

    let _guard = PARAMS_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    install_params(params);

    let pool = native_pool()?;

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_info| {
        // Deliberately silent — see the function doc comment above.
    }));

    enum RawOutcome {
        Ok(SimResult, StalenessSummary),
        Tripped,
    }

    let raw_result: anyhow::Result<Vec<RawOutcome>> = pool.install(|| {
        configs
            .par_iter()
            .map(|config| -> anyhow::Result<RawOutcome> {
                let attempt = std::panic::catch_unwind(AssertUnwindSafe(
                    || -> anyhow::Result<(SimResult, StalenessSummary)> {
                        seed_thread_local(config);
                        let result = engine::run_simulation_native(
                            oracle_swap,
                            Some(oracle_after_swap),
                            normalizer::compute_swap,
                            Some(normalizer::after_swap),
                            config,
                        )?;
                        check_fingerprint_terminal(config.seed, config.n_steps);
                        let staleness = take_staleness_summary();
                        Ok((result, staleness))
                    },
                ));
                match attempt {
                    Ok(Ok((result, staleness))) => Ok(RawOutcome::Ok(result, staleness)),
                    Ok(Err(e)) => Err(e), // Genuine, non-panic error — propagate immediately.
                    Err(_payload) => Ok(RawOutcome::Tripped),
                }
            })
            .collect()
    });

    std::panic::set_hook(previous_hook);
    let raw = raw_result?;

    let mut oks = Vec::new();
    let mut tripped = Vec::new();
    for (config, outcome) in configs.iter().zip(raw) {
        match outcome {
            RawOutcome::Ok(result, staleness) => oks.push((result, staleness)),
            RawOutcome::Tripped => tripped.push(FingerprintTrippedSeed {
                seed: config.seed,
                config: config.clone(),
            }),
        }
    }

    Ok((oks, tripped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use prop_amm_shared::config::HyperparameterVariance;
    use prop_amm_shared::instruction::{encode_after_swap, encode_swap_instruction};

    fn tiny_configs(n: u64, seed_base: u64) -> Vec<SimulationConfig> {
        let base = SimulationConfig {
            n_steps: 50,
            ..SimulationConfig::default()
        };
        let variance = HyperparameterVariance::default();
        (0..n)
            .map(|i| variance.apply(&base, seed_base + i))
            .collect()
    }

    fn trade_triggered_params(concentration: f64, spread_bps: f64) -> OracleParams {
        OracleParams {
            variant: OracleVariant::Anchored,
            concentration,
            spread_bps,
            cursor_mode: CursorMode::TradeTriggered,
            fingerprint_lag: 0,
        }
    }

    #[test]
    fn oracle_swap_returns_zero_on_degenerate_input() {
        let _guard = PARAMS_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        seed_thread_local(&SimulationConfig::default());
        install_params(trade_triggered_params(10.0, 20.0));
        let zero_reserve = encode_swap_instruction(0, 1_000_000_000, 0, 1_000_000_000, &[]);
        assert_eq!(oracle_swap(&zero_reserve), 0);
        let zero_input = encode_swap_instruction(0, 0, 1_000_000_000, 1_000_000_000, &[]);
        assert_eq!(oracle_swap(&zero_input), 0);
    }

    /// At `reserve_x == target_x` (the anchored variant's starting point), the curve's own
    /// marginal price is `k / v0^2` on both sides by construction — a tiny probe trade on
    /// each side should therefore price close to `p_oracle * (1 +/- spread)`, in the
    /// direction the spread guard dictates (buy dear, sell cheap).
    #[test]
    fn marginal_price_at_target_matches_oracle_price_with_spread() {
        let _guard = PARAMS_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let cfg = SimulationConfig::default();
        seed_thread_local(&cfg);
        let p_oracle = PRICE_PATH.with(|p| p.borrow()[0]);
        install_params(OracleParams {
            variant: OracleVariant::Anchored,
            concentration: 50.0,
            spread_bps: 100.0, // 1%
            cursor_mode: CursorMode::TradeTriggered,
            fingerprint_lag: 0,
        });

        let reserve_x_nano = f64_to_nano(cfg.initial_x);
        let reserve_y_nano = f64_to_nano(cfg.initial_y);
        let tiny_input = f64_to_nano(0.01); // marginal relative to v0*price, but with enough
                                            // nano-scale resolution that f64_to_nano/nano_to_f64 rounding at the output doesn't
                                            // dominate the comparison (a smaller probe like 1e-6 real units quantizes the output
                                            // to a handful of nano-units and swamps the signal being tested).

        // side 0: buy X from pool (Y in, X out) -> priced at p_oracle * 1.01.
        let buy = encode_swap_instruction(0, tiny_input, reserve_x_nano, reserve_y_nano, &[]);
        let x_out = nano_to_f64(oracle_swap(&buy));
        let y_in = nano_to_f64(tiny_input);
        let implied_buy_price = y_in / x_out;
        assert!(
            (implied_buy_price - p_oracle * 1.01).abs() / (p_oracle * 1.01) < 1e-3,
            "implied buy price {implied_buy_price} should be close to {}",
            p_oracle * 1.01
        );

        // side 1: sell X to pool (X in, Y out) -> priced at p_oracle * 0.99.
        let sell = encode_swap_instruction(1, tiny_input, reserve_x_nano, reserve_y_nano, &[]);
        let y_out = nano_to_f64(oracle_swap(&sell));
        let x_in = nano_to_f64(tiny_input);
        let implied_sell_price = y_out / x_in;
        assert!(
            (implied_sell_price - p_oracle * 0.99).abs() / (p_oracle * 0.99) < 1e-3,
            "implied sell price {implied_sell_price} should be close to {}",
            p_oracle * 0.99
        );
    }

    #[test]
    fn floating_variant_has_base_identically_v0_at_any_reserve() {
        let _guard = PARAMS_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Floating always re-reads target_x = reserve_x, so base = v0 identically —
        // degenerate by construction. Provable indirectly: whatever reserve_x is quoted at,
        // side 0 and side 1 marginal prices both sit exactly at p_oracle * (1 +/- spread),
        // never drifting with reserve_x the way the anchored variant would.
        let cfg = SimulationConfig::default();
        seed_thread_local(&cfg);
        let p_oracle = PRICE_PATH.with(|p| p.borrow()[0]);
        install_params(OracleParams {
            variant: OracleVariant::Floating,
            concentration: 10.0,
            spread_bps: 50.0,
            cursor_mode: CursorMode::TradeTriggered,
            fingerprint_lag: 0,
        });

        for reserve_x in [10.0, 100.0, 10_000.0] {
            let reserve_x_nano = f64_to_nano(reserve_x);
            let reserve_y_nano = f64_to_nano(10_000.0);
            let tiny_input = f64_to_nano(0.01);
            let buy = encode_swap_instruction(0, tiny_input, reserve_x_nano, reserve_y_nano, &[]);
            let x_out = nano_to_f64(oracle_swap(&buy));
            let implied = nano_to_f64(tiny_input) / x_out;
            assert!(
                (implied - p_oracle * 1.005).abs() / (p_oracle * 1.005) < 1e-3,
                "reserve_x={reserve_x}: implied {implied} vs {}",
                p_oracle * 1.005
            );
        }
    }

    #[test]
    fn oracle_after_swap_records_staleness_from_cursor_gap() {
        let _guard = PARAMS_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        install_params(trade_triggered_params(10.0, 20.0));
        seed_thread_local(&SimulationConfig::default());
        let trade_at = |step: u64| encode_after_swap(0, 1, 1, 1, 1, step, &[]);
        oracle_after_swap(&trade_at(3), &mut []);
        oracle_after_swap(&trade_at(10), &mut []);
        oracle_after_swap(&trade_at(11), &mut []);
        let summary = take_staleness_summary();
        assert_eq!(summary.n, 3);
        // staleness samples were [3, 7, 1] (first trade's staleness is its own step, since
        // cursor starts at 0).
        assert_eq!(summary.max, 7.0);
    }

    #[test]
    fn take_staleness_summary_resets_per_simulation_state() {
        let _guard = PARAMS_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        install_params(trade_triggered_params(10.0, 20.0));
        seed_thread_local(&SimulationConfig::default());
        oracle_after_swap(&encode_after_swap(0, 1, 1, 1, 1, 5, &[]), &mut []);
        let first = take_staleness_summary();
        assert_eq!(first.n, 1);

        // Nothing recorded between the two summaries -> the second is empty, not a leftover
        // from the first (the whole point of resetting at a known simulation boundary).
        seed_thread_local(&SimulationConfig::default());
        let second = take_staleness_summary();
        assert_eq!(second.n, 0);
    }

    #[test]
    fn run_batch_native_loop_matches_normalizer_reference_math() {
        // With the submission fn set to the same fixed normalizer opponent, both AMMs run
        // an identical curve over identical starting reserves -- this is the exact shape
        // `--self-check` relies on to validate the lane's own loop.
        let configs = tiny_configs(4, 700_000_000);
        let batch = run_batch_native_loop(
            normalizer::compute_swap,
            Some(normalizer::after_swap),
            &configs,
        )
        .unwrap();
        assert_eq!(batch.n_sims(), 4);
    }

    #[test]
    fn run_batch_produces_one_staleness_summary_per_seed_in_order() {
        let configs = tiny_configs(5, 710_000_000);
        let params = trade_triggered_params(20.0, 10.0);
        let (batch, staleness) = run_batch(params, &configs).unwrap();
        assert_eq!(batch.n_sims(), 5);
        assert_eq!(staleness.len(), 5);
        for (result, config) in batch.results.iter().zip(configs.iter()) {
            assert_eq!(result.seed, config.seed);
        }
    }

    /// WHI-1248: proves the fingerprint cursor actually tracks the real arbitrageur's own
    /// probe sequence over a real (if tiny) simulation, ending at the last step with zero
    /// hardening-check trips — the base case every other fingerprint test builds on.
    #[test]
    fn fingerprint_cursor_tracks_a_real_simulation_with_no_trips() {
        let configs = tiny_configs(6, 720_000_000);
        let params = OracleParams {
            variant: OracleVariant::Anchored,
            concentration: 20.0,
            spread_bps: 10.0,
            cursor_mode: CursorMode::Fingerprint,
            fingerprint_lag: 1,
        };
        let (oks, tripped) = run_batch_fingerprint_checked(params, &configs).unwrap();
        assert!(
            tripped.is_empty(),
            "expected zero fingerprint hardening-check trips over a real simulation, got: {:?}",
            tripped.iter().map(|t| t.seed).collect::<Vec<_>>()
        );
        assert_eq!(oks.len(), configs.len());
    }

    /// WHI-1248 acceptance criterion: a test proving the per-trade assertion (hardening
    /// check (a)) actually fires on a corrupted cursor. Rather than corrupt private state
    /// from outside this module (not exposed, by design), this drives `oracle_after_swap`
    /// directly with a `step` engineered to disagree with wherever `FP_CURSOR` actually is —
    /// exactly the mismatch the assertion exists to catch.
    #[test]
    #[should_panic(expected = "WHI-1248 fingerprint cursor hardening check (a)")]
    fn per_trade_assertion_fires_on_a_corrupted_cursor() {
        let _guard = PARAMS_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        install_params(OracleParams {
            variant: OracleVariant::Anchored,
            concentration: 10.0,
            spread_bps: 20.0,
            cursor_mode: CursorMode::Fingerprint,
            fingerprint_lag: 0,
        });
        seed_thread_local(&SimulationConfig::default());
        // FP_CURSOR starts at 0; report a trade at step 999, which it cannot possibly equal.
        oracle_after_swap(&encode_after_swap(0, 1, 1, 1, 1, 999, &[]), &mut []);
    }

    /// WHI-1248 acceptance criterion: a test proving the terminal assertion (hardening check
    /// (b)) fires when a simulation ends with the cursor short of the final step.
    #[test]
    #[should_panic(expected = "WHI-1248 fingerprint cursor hardening check (b)")]
    fn terminal_assertion_fires_when_cursor_never_reaches_the_final_step() {
        let _guard = PARAMS_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        install_params(OracleParams {
            variant: OracleVariant::Anchored,
            concentration: 10.0,
            spread_bps: 20.0,
            cursor_mode: CursorMode::Fingerprint,
            fingerprint_lag: 0,
        });
        let cfg = SimulationConfig {
            n_steps: 50,
            ..SimulationConfig::default()
        };
        seed_thread_local(&cfg);
        // No probes were ever issued, so FP_CURSOR is still 0, not 49.
        check_fingerprint_terminal(cfg.seed, cfg.n_steps);
    }

    /// WHI-1248: the side-1 gate (hardening check (c)) must block a same-step, side-0-only
    /// coincidental collision with the *next* step's own buy target from advancing the
    /// cursor early — proven directly against [`maybe_advance_fingerprint_cursor`] with a
    /// synthetic target table, independent of whether a real GBM path ever produces such a
    /// collision in practice.
    #[test]
    fn side1_gate_blocks_a_same_step_buy_side_collision_with_the_next_targets_buy_probe() {
        let _guard = PARAMS_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        install_params(OracleParams {
            variant: OracleVariant::Anchored,
            concentration: 10.0,
            spread_bps: 20.0,
            cursor_mode: CursorMode::Fingerprint,
            fingerprint_lag: 0,
        });
        FP_TARGETS.with(|t| *t.borrow_mut() = vec![(111, 222), (333, 444), (555, 666)]);
        FP_CURSOR.with(|c| *c.borrow_mut() = 0);
        SEEN_SIDE1_SINCE_ADVANCE.with(|s| s.set(false));
        CALLS_SINCE_FP_ADVANCE.with(|c| c.set(0));

        // A side-0 call carrying the *next* step's own buy probe, before any side-1 call has
        // been seen since the last advance — must be blocked.
        maybe_advance_fingerprint_cursor(0, 333);
        assert_eq!(
            FP_CURSOR.with(|c| *c.borrow()),
            0,
            "cursor must not advance on a side-0 match before any side-1 call was seen"
        );

        // Now a side-1 call (this step's own sell probe) is seen — the gate opens.
        maybe_advance_fingerprint_cursor(1, 222);
        // The very next side-0 call carrying the next step's buy probe now correctly advances.
        maybe_advance_fingerprint_cursor(0, 333);
        assert_eq!(FP_CURSOR.with(|c| *c.borrow()), 1);
    }

    /// WHI-1248: [`fingerprint_panic_diagnostics`] must return `None` outside
    /// [`CursorMode::Fingerprint`] (harmless to call from every `catch_panicking` site,
    /// including trade-triggered-only ones) and `Some` inside it.
    #[test]
    fn fingerprint_panic_diagnostics_is_mode_gated() {
        let _guard = PARAMS_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        install_params(trade_triggered_params(10.0, 20.0));
        assert_eq!(fingerprint_panic_diagnostics(), None);

        install_params(OracleParams {
            variant: OracleVariant::Anchored,
            concentration: 10.0,
            spread_bps: 20.0,
            cursor_mode: CursorMode::Fingerprint,
            fingerprint_lag: 0,
        });
        CALLS_SINCE_FP_ADVANCE.with(|c| c.set(7));
        assert_eq!(fingerprint_panic_diagnostics(), Some(7));
        CALLS_SINCE_FP_ADVANCE.with(|c| c.set(0));
    }
}
