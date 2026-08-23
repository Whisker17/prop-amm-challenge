//! The ceiling lane's host-side "oracle re-anchor" curve (WHI-1247): a concentrated,
//! spread-bearing curve that quotes directly off a replayed GBM fair-price path rather than
//! off its own reserves — the "price re-anchor the arbitrageur cannot front-run" this issue
//! measures. Lives in `tools/bench`, not a `strategies/**` `lib.rs`: it is never compiled to
//! BPF, never submittable, and out of competition end to end (`ceilings/README.md`).
//!
//! Structurally this mirrors `telemetry.rs`'s own pattern exactly (WHI-1247 step 4): a
//! process-global, `Mutex`-guarded set of "batch-constant" statics installed once,
//! single-threaded, before a parallel batch (here: which `OracleVariant`, and the fitted
//! `CONCENTRATION`/`SPREAD_BPS`), plus a `thread_local!` block of genuinely per-simulation
//! state (the replayed price path, the trade-triggered cursor, the captured anchor, and this
//! simulation's staleness samples) reset around each `engine::run_simulation_native` call.
//! `oracle_swap`/`oracle_after_swap` are bare `fn` pointers (`SwapFn`/`AfterSwapFn` from
//! `crates/executor/src/native.rs` carry no captured state at all), so these statics are the
//! only channel either side of this module has to the other.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;

use prop_amm_executor::{AfterSwapFn, SwapFn};
use prop_amm_shared::config::SimulationConfig;
use prop_amm_shared::instruction::{decode_after_swap, decode_instruction};
use prop_amm_shared::nano::{f64_to_nano, nano_to_f64};
use prop_amm_shared::normalizer;
use prop_amm_shared::result::{BatchResult, SimResult};
use prop_amm_sim::engine;
use prop_amm_sim::price_process::GBMPriceProcess;
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

/// This rung's frozen point plus which variant it was fitted/measured under (WHI-1247 steps
/// 3, 10) — installed once, batch-constant, before a parallel batch starts.
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
}

// --- Batch-constant state (process-global; mirrors telemetry.rs's REAL_AFTER_SWAP/CALL_LOCK) ---

static VARIANT: AtomicU8 = AtomicU8::new(OracleVariant::Anchored as u8);
static CONCENTRATION_BITS: AtomicU64 = AtomicU64::new(0);
static SPREAD_BPS_BITS: AtomicU64 = AtomicU64::new(0);

/// Held for one call's entire install-through-collect duration — guards concurrent *calls*
/// to [`run_batch`] (e.g. two tests in the same binary), not the internal rayon parallelism
/// within one call. Exactly `telemetry.rs`'s own `CALL_LOCK` role, under a name specific to
/// this module's own statics.
static PARAMS_LOCK: Mutex<()> = Mutex::new(());

fn install_params(params: OracleParams) {
    VARIANT.store(params.variant as u8, Ordering::Relaxed);
    CONCENTRATION_BITS.store(params.concentration.to_bits(), Ordering::Relaxed);
    SPREAD_BPS_BITS.store(params.spread_bps.to_bits(), Ordering::Relaxed);
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
}

/// Resets this thread's per-simulation state for a fresh call to `engine::run_simulation_native`
/// under `cfg` — the replayed price path, cursor, captured anchor, and staleness samples.
/// Call immediately before that call, on the same thread, mirroring
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
    PRICE_PATH.with(|p| *p.borrow_mut() = path);
    CURSOR.with(|c| *c.borrow_mut() = 0);
    ANCHOR_X.with(|a| *a.borrow_mut() = cfg.initial_x);
    STALENESS_SAMPLES.with(|s| s.borrow_mut().clear());
}

/// This rung's staleness distribution for one simulation (WHI-1247 step 5): mean, median,
/// p95, and max steps-since-last-executed-trade across every trade this simulation routed
/// to the submission AMM, plus the sample count.
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
pub fn oracle_swap(data: &[u8]) -> u64 {
    let (side, input_amount, reserve_x_nano, reserve_y_nano) = decode_instruction(data);
    if reserve_x_nano == 0 || reserve_y_nano == 0 || input_amount == 0 {
        return 0;
    }

    let p_oracle = PRICE_PATH.with(|p| {
        let path = p.borrow();
        if path.is_empty() {
            return f64::NAN;
        }
        let cursor = CURSOR.with(|c| *c.borrow()) as usize;
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

    let price = match side {
        0 => p_oracle * (1.0 + spread),
        1 => p_oracle * (1.0 - spread),
        _ => return 0,
    };
    if !(price.is_finite() && price > 0.0) {
        return 0;
    }

    let k = v0 * v0 * price;
    if !(k.is_finite() && k > 0.0) {
        return 0;
    }

    let input = nano_to_f64(input_amount);
    let out = match side {
        // Buy X from the pool: `dy` (Y) in, `dx` (X) out.
        0 => {
            let denom = k / base + input;
            if !(denom.is_finite() && denom > 0.0) {
                return 0;
            }
            base - k / denom
        }
        // Sell X to the pool: `dx` (X) in, `dy` (Y) out.
        1 => {
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
/// cursor to this trade's own step. Only ever installed as the *submission* AMM's
/// `after_swap` (never the normalizer's) — a trade routed to the normalizer must not move
/// this pool's own oracle cursor.
pub fn oracle_after_swap(data: &[u8], _storage: &mut [u8]) {
    let (_side, _input_amount, _output_amount, _reserve_x, _reserve_y, step, _storage) =
        decode_after_swap(data);
    CURSOR.with(|c| {
        let mut cursor = c.borrow_mut();
        let staleness = step.saturating_sub(*cursor);
        STALENESS_SAMPLES.with(|s| s.borrow_mut().push(staleness));
        *cursor = step;
    });
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
                let staleness = take_staleness_summary();
                Ok((result, staleness))
            })
            .collect()
    });

    let (results, staleness): (Vec<SimResult>, Vec<StalenessSummary>) = pairs?.into_iter().unzip();
    Ok((BatchResult::from_results(results), staleness))
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

    #[test]
    fn oracle_swap_returns_zero_on_degenerate_input() {
        seed_thread_local(&SimulationConfig::default());
        install_params(OracleParams {
            variant: OracleVariant::Anchored,
            concentration: 10.0,
            spread_bps: 20.0,
        });
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
        let cfg = SimulationConfig::default();
        seed_thread_local(&cfg);
        let p_oracle = PRICE_PATH.with(|p| p.borrow()[0]);
        install_params(OracleParams {
            variant: OracleVariant::Anchored,
            concentration: 50.0,
            spread_bps: 100.0, // 1%
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
        let params = OracleParams {
            variant: OracleVariant::Anchored,
            concentration: 20.0,
            spread_bps: 10.0,
        };
        let (batch, staleness) = run_batch(params, &configs).unwrap();
        assert_eq!(batch.n_sims(), 5);
        assert_eq!(staleness.len(), 5);
        for (result, config) in batch.results.iter().zip(configs.iter()) {
            assert_eq!(result.seed, config.seed);
        }
    }
}
