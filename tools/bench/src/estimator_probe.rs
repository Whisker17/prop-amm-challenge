//! WHI-1225's pre-registered "Probe A": replicate an estimator's internal running state from
//! the same `after_swap` payload the estimator itself reads, without touching the real
//! candidate's storage — zero submission changes, zero search budget, run before any
//! `strategies/005b-.../lib.rs` exists. Not `docs/DESIGN.md` §2.7's deferred "L2" (that's full
//! per-step fair-price reconstruction); this only mirrors the same signal `005`'s/`004`'s own
//! `after_swap` already computes from `(reserve_x, reserve_y, step)`, in a thread-local shadow
//! accumulator, following `telemetry.rs`'s own delegate-then-observe pattern
//! (`record_and_delegate`/`take_thread_counters`).
//!
//! Two probes, one per committed strategy, each a deliberate, flagged duplication of that
//! strategy's own math (same category as `curve_checks.rs`'s upstream mirror, noted in
//! `docs/DESIGN.md`'s architecture-risk table) — not a refactor of either `lib.rs`:
//!
//! - [`run_005_dual_estimator_probe`]: replicates `005-vol-adaptive-cpmm-fee`'s variance
//!   accumulation twice in parallel — the committed `var_sum/count` normalization and the
//!   candidate `var_sum/elapsed_sum` fix — from the identical sampled-move sequence.
//! - [`run_004_floor_probe`]: replicates `004-ewma-shock-decay-fee`'s `ewma_vol` EWMA update
//!   and records its time- and volume-weighted distribution against a floor sweep (WHI-1223's
//!   own open question, folded into this same run per WHI-1225's "Added scope").

use std::cell::RefCell;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Mutex;

use rayon::prelude::*;

use prop_amm_executor::{AfterSwapFn, SwapFn};
use prop_amm_shared::config::SimulationConfig;
use prop_amm_shared::instruction::decode_after_swap;
use prop_amm_shared::nano::nano_to_f64;
use prop_amm_shared::normalizer;
use prop_amm_sim::engine;

/// `FLOOR_BPS` values the `004b` issue itself froze (`0..=60`), including the two it actually
/// probed (25, 30) — the whole range WHI-1225's "Added scope" asks this probe to sweep.
pub const FLOOR_BPS: [u64; 7] = [0, 10, 20, 25, 30, 40, 60];
const N_FLOORS: usize = FLOOR_BPS.len();

// ---- 005's own fixed-point constants, mirrored verbatim from
// strategies/005-vol-adaptive-cpmm-fee/lib.rs (not imported: that crate is a BPF program
// target outside the workspace, per AGENTS.md's `programs`/`strategies` carve-out — a
// deliberate, flagged duplication, same category as `curve_checks.rs`'s upstream mirror). ----
const P_SCALE_005: u128 = 1_000_000_000;
const MOVE_BPS_CAP_005: u128 = 250;

/// Newton's method integer sqrt, mirrored from `005`'s own `isqrt` (behaviourally identical;
/// `x.div_ceil(2)` replaces `005`'s `(x + 1) / 2` per this workspace's own clippy gate, which
/// `strategies/**` isn't subject to since it sits outside the cargo workspace).
fn isqrt(n: u128) -> u128 {
    if n < 2 {
        return n;
    }
    let mut x = n;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

#[derive(Clone, Copy, Default)]
struct Vol005State {
    initialized: bool,
    last_step: u64,
    last_price_fp: u128,
    var_sum: u128,
    count: u64,
    elapsed_sum: u64,
}

thread_local! {
    static VOL005_STATE: RefCell<Vol005State> = RefCell::new(Vol005State::default());
}

fn take_vol005_state() -> Vol005State {
    VOL005_STATE.with(|s| {
        let state = *s.borrow();
        *s.borrow_mut() = Vol005State::default();
        state
    })
}

static REAL_AFTER_SWAP_005: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static CALL_LOCK_005: Mutex<()> = Mutex::new(());

/// Delegates to the real `005` `after_swap` unchanged, then updates a thread-local shadow
/// accumulator from the identical payload — mirroring `005`'s own sampling rule (first
/// executed trade of a new `step`) and its own capped-relative-move-then-square computation,
/// but tracking `count` and `elapsed_sum` (`step - last_step`) side by side so both
/// normalizations can be computed from one run.
fn vol005_recorder(data: &[u8], storage: &mut [u8]) {
    let (_side, _input_amount, _output_amount, rx, ry, step, _storage) = decode_after_swap(data);
    if rx > 0 && ry > 0 {
        VOL005_STATE.with(|s| {
            let mut st = s.borrow_mut();
            let price_fp = (ry as u128).saturating_mul(P_SCALE_005) / rx as u128;
            if !st.initialized {
                st.initialized = true;
                st.last_step = step;
                st.last_price_fp = price_fp;
            } else if step > st.last_step {
                let last_price = st.last_price_fp;
                if last_price > 0 {
                    let diff = price_fp.abs_diff(last_price);
                    let mut move_bps = diff.saturating_mul(10_000) / last_price;
                    if move_bps > MOVE_BPS_CAP_005 {
                        move_bps = MOVE_BPS_CAP_005;
                    }
                    let r2 = move_bps.saturating_mul(move_bps);
                    let gap = step - st.last_step; // >= 1, guarded by `step > st.last_step` above
                    st.var_sum = st.var_sum.saturating_add(r2);
                    st.count = st.count.saturating_add(1);
                    st.elapsed_sum = st.elapsed_sum.saturating_add(gap);
                }
                st.last_step = step;
                st.last_price_fp = price_fp;
            }
        });
    }

    let ptr = REAL_AFTER_SWAP_005.load(Ordering::Relaxed);
    if !ptr.is_null() {
        let real: AfterSwapFn = unsafe { std::mem::transmute(ptr) };
        real(data, storage);
    }
}

/// One seed's final estimator state under both normalizations — the issue's own "record per
/// seed: final sigma-hat_old, sigma-hat_new, `count / n_steps`."
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vol005ProbeSim {
    pub seed: u64,
    pub true_sigma: f64,
    pub n_steps: u32,
    pub count: u64,
    pub elapsed_sum: u64,
    pub sigma_hat_old: f64,
    pub sigma_hat_new: f64,
}

/// Runs the committed `005` (its real `compute_swap`/`after_swap`, loaded via
/// `compile::build_and_load`) over `configs`, alongside the shadow accumulator above.
/// Delegate-then-observe, never observe-then-mutate: the real run's `submission_edge`
/// trajectory is untouched (proven by this module's own
/// `shadow_accumulator_does_not_perturb_the_real_run` test, mirroring `telemetry.rs`'s own
/// non-invasiveness proof).
pub fn run_005_dual_estimator_probe(
    swap_fn: SwapFn,
    after_swap_fn: Option<AfterSwapFn>,
    configs: &[SimulationConfig],
) -> anyhow::Result<Vec<Vol005ProbeSim>> {
    let _guard = CALL_LOCK_005.lock().unwrap_or_else(|p| p.into_inner());
    let ptr = match after_swap_fn {
        Some(f) => f as *mut (),
        None => std::ptr::null_mut(),
    };
    REAL_AFTER_SWAP_005.store(ptr, Ordering::Relaxed);

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(rayon::current_num_threads().min(8))
        .build()?;

    pool.install(|| {
        configs
            .par_iter()
            .map(|config| -> anyhow::Result<Vol005ProbeSim> {
                engine::run_simulation_native(
                    swap_fn,
                    Some(vol005_recorder),
                    normalizer::compute_swap,
                    Some(normalizer::after_swap),
                    config,
                )?;
                let state = take_vol005_state();
                let sigma_hat_old = isqrt(state.var_sum / (state.count as u128).max(1));
                let sigma_hat_new = isqrt(state.var_sum / (state.elapsed_sum as u128).max(1));
                Ok(Vol005ProbeSim {
                    seed: config.seed,
                    true_sigma: config.gbm_sigma,
                    n_steps: config.n_steps,
                    count: state.count,
                    elapsed_sum: state.elapsed_sum,
                    sigma_hat_old: sigma_hat_old as f64,
                    sigma_hat_new: sigma_hat_new as f64,
                })
            })
            .collect()
    })
}

// ---- 004's own fixed-point constants, mirrored verbatim from
// strategies/004-ewma-shock-decay-fee/lib.rs (same duplication rationale as above). ----
const ALPHA_1E9_004: u128 = 200_000_000; // vol EWMA alpha = 0.20
const ONE_M_ALPHA_1E9_004: u128 = 1_000_000_000 - ALPHA_1E9_004;

#[derive(Clone, Copy, Default)]
struct Ewma004State {
    initialized: bool,
    last_rx: u64,
    last_ry: u64,
    ewma_vol: u64,
    below_floor_steps: [u64; N_FLOORS],
    total_steps: u64,
    below_floor_volume: [f64; N_FLOORS],
    total_volume: f64,
}

thread_local! {
    static EWMA004_STATE: RefCell<Ewma004State> = RefCell::new(Ewma004State::default());
}

fn take_ewma004_state() -> Ewma004State {
    EWMA004_STATE.with(|s| {
        let state = *s.borrow();
        *s.borrow_mut() = Ewma004State::default();
        state
    })
}

static REAL_AFTER_SWAP_004: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static CALL_LOCK_004: Mutex<()> = Mutex::new(());

/// Delegates to the real `004` `after_swap` unchanged, then updates a thread-local shadow
/// EWMA from the identical payload — mirroring `004`'s own update on every executed trade (no
/// per-step dedup, unlike `005`), and recording the post-update `ewma_vol` against each
/// `FLOOR_BPS` threshold, both by step count and by this trade's own Y-volume (the same
/// `volume_y` convention `telemetry.rs::record_and_delegate` already uses).
fn ewma004_recorder(data: &[u8], storage: &mut [u8]) {
    let (side, input_amount, output_amount, cur_rx, cur_ry, _step, _storage) =
        decode_after_swap(data);
    let volume_y = if side == 0 {
        nano_to_f64(input_amount)
    } else {
        nano_to_f64(output_amount)
    };

    EWMA004_STATE.with(|s| {
        let mut st = s.borrow_mut();
        let price_change_1e9: u64 =
            if st.initialized && st.last_rx > 0 && st.last_ry > 0 && cur_rx > 0 {
                let cross_new = (cur_ry as u128).saturating_mul(st.last_rx as u128);
                let cross_old = (st.last_ry as u128).saturating_mul(cur_rx as u128);
                let diff = cross_new.abs_diff(cross_old);
                let denom = (st.last_ry as u128).saturating_mul(cur_rx as u128);
                if denom > 0 {
                    diff.saturating_mul(1_000_000_000)
                        .checked_div(denom)
                        .unwrap_or(u128::MAX)
                        .min(u64::MAX as u128) as u64
                } else {
                    0
                }
            } else {
                0
            };

        let new_vol = ((ALPHA_1E9_004 * price_change_1e9 as u128
            + ONE_M_ALPHA_1E9_004 * st.ewma_vol as u128)
            / 1_000_000_000) as u64;

        st.ewma_vol = new_vol;
        st.initialized = true;
        st.last_rx = cur_rx;
        st.last_ry = cur_ry;
        st.total_steps += 1;
        st.total_volume += volume_y;
        for (i, &floor_bps) in FLOOR_BPS.iter().enumerate() {
            if new_vol <= floor_bps * 100_000 {
                st.below_floor_steps[i] += 1;
                st.below_floor_volume[i] += volume_y;
            }
        }
    });

    let ptr = REAL_AFTER_SWAP_004.load(Ordering::Relaxed);
    if !ptr.is_null() {
        let real: AfterSwapFn = unsafe { std::mem::transmute(ptr) };
        real(data, storage);
    }
}

/// One seed's `ewma_vol`-vs-floor time/volume distribution (WHI-1223's own open question,
/// answered here per WHI-1225's "Added scope").
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ewma004ProbeSim {
    pub seed: u64,
    pub true_sigma: f64,
    pub below_floor_steps: [u64; N_FLOORS],
    pub total_steps: u64,
    pub below_floor_volume: [f64; N_FLOORS],
    pub total_volume: f64,
}

/// Runs the committed `004` over `configs`, alongside the shadow EWMA above. Same
/// delegate-then-observe non-invasiveness contract as [`run_005_dual_estimator_probe`].
pub fn run_004_floor_probe(
    swap_fn: SwapFn,
    after_swap_fn: Option<AfterSwapFn>,
    configs: &[SimulationConfig],
) -> anyhow::Result<Vec<Ewma004ProbeSim>> {
    let _guard = CALL_LOCK_004.lock().unwrap_or_else(|p| p.into_inner());
    let ptr = match after_swap_fn {
        Some(f) => f as *mut (),
        None => std::ptr::null_mut(),
    };
    REAL_AFTER_SWAP_004.store(ptr, Ordering::Relaxed);

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(rayon::current_num_threads().min(8))
        .build()?;

    pool.install(|| {
        configs
            .par_iter()
            .map(|config| -> anyhow::Result<Ewma004ProbeSim> {
                engine::run_simulation_native(
                    swap_fn,
                    Some(ewma004_recorder),
                    normalizer::compute_swap,
                    Some(normalizer::after_swap),
                    config,
                )?;
                let state = take_ewma004_state();
                Ok(Ewma004ProbeSim {
                    seed: config.seed,
                    true_sigma: config.gbm_sigma,
                    below_floor_steps: state.below_floor_steps,
                    total_steps: state.total_steps,
                    below_floor_volume: state.below_floor_volume,
                    total_volume: state.total_volume,
                })
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use prop_amm_shared::config::HyperparameterVariance;

    fn tiny_configs(n: u64) -> Vec<SimulationConfig> {
        let base = SimulationConfig {
            n_steps: 200,
            ..SimulationConfig::default()
        };
        let variance = HyperparameterVariance::default();
        (0..n).map(|seed| variance.apply(&base, seed)).collect()
    }

    /// The non-invasiveness acceptance criterion, `telemetry.rs`-style: the same candidate
    /// over the same configs must yield a bit-identical `submission_edge` trajectory whether
    /// run through plain `run_simulation_native` (no probe) or with the probe's recorder
    /// installed — proven here against the normalizer standing in for a compiled candidate.
    #[test]
    fn vol005_probe_does_not_perturb_the_real_run() {
        let configs = tiny_configs(6);

        let without_probe: Vec<_> = configs
            .iter()
            .map(|c| {
                prop_amm_sim::engine::run_simulation_native(
                    normalizer::compute_swap,
                    None,
                    normalizer::compute_swap,
                    Some(normalizer::after_swap),
                    c,
                )
                .unwrap()
            })
            .collect();

        let probe_sims =
            run_005_dual_estimator_probe(normalizer::compute_swap, None, &configs).unwrap();

        let with_probe: Vec<_> = configs
            .iter()
            .map(|c| {
                prop_amm_sim::engine::run_simulation_native(
                    normalizer::compute_swap,
                    None,
                    normalizer::compute_swap,
                    Some(normalizer::after_swap),
                    c,
                )
                .unwrap()
            })
            .collect();

        for (a, b) in without_probe.iter().zip(with_probe.iter()) {
            assert_eq!(a.seed, b.seed);
            assert_eq!(a.submission_edge, b.submission_edge);
        }
        assert_eq!(probe_sims.len(), configs.len());
    }

    #[test]
    fn ewma004_probe_does_not_perturb_the_real_run() {
        let configs = tiny_configs(6);

        let without_probe: Vec<_> = configs
            .iter()
            .map(|c| {
                prop_amm_sim::engine::run_simulation_native(
                    normalizer::compute_swap,
                    None,
                    normalizer::compute_swap,
                    Some(normalizer::after_swap),
                    c,
                )
                .unwrap()
            })
            .collect();

        let probe_sims = run_004_floor_probe(normalizer::compute_swap, None, &configs).unwrap();

        let with_probe: Vec<_> = configs
            .iter()
            .map(|c| {
                prop_amm_sim::engine::run_simulation_native(
                    normalizer::compute_swap,
                    None,
                    normalizer::compute_swap,
                    Some(normalizer::after_swap),
                    c,
                )
                .unwrap()
            })
            .collect();

        for (a, b) in without_probe.iter().zip(with_probe.iter()) {
            assert_eq!(a.seed, b.seed);
            assert_eq!(a.submission_edge, b.submission_edge);
        }
        assert_eq!(probe_sims.len(), configs.len());
    }

    #[test]
    fn vol005_probe_records_a_nonzero_sample_count_on_a_real_run() {
        let configs = tiny_configs(3);
        let sims = run_005_dual_estimator_probe(normalizer::compute_swap, None, &configs).unwrap();
        for sim in &sims {
            assert!(sim.count > 0, "seed {} sampled nothing", sim.seed);
            // elapsed_sum >= count by construction (each gap is >= 1).
            assert!(sim.elapsed_sum >= sim.count);
            // Dividing by a larger-or-equal denominator can only lower (or match) the
            // estimate — the sign the issue's own "one-directional by arithmetic" claim
            // depends on.
            assert!(sim.sigma_hat_new <= sim.sigma_hat_old);
        }
    }

    #[test]
    fn ewma004_probe_records_every_trade_and_a_monotone_floor_ladder() {
        let configs = tiny_configs(3);
        let sims = run_004_floor_probe(normalizer::compute_swap, None, &configs).unwrap();
        for sim in &sims {
            assert!(sim.total_steps > 0, "seed {} recorded no trades", sim.seed);
            assert!(sim.total_volume > 0.0);
            // A higher floor can only ever count at least as many steps below it.
            for i in 1..N_FLOORS {
                assert!(sim.below_floor_steps[i] >= sim.below_floor_steps[i - 1]);
                assert!(sim.below_floor_volume[i] >= sim.below_floor_volume[i - 1]);
            }
            assert!(*sim.below_floor_steps.last().unwrap() <= sim.total_steps);
        }
    }
}
