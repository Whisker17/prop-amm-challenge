use std::cell::RefCell;
use std::sync::atomic::{AtomicPtr, Ordering};

use rayon::prelude::*;

use prop_amm_executor::{AfterSwapFn, SwapFn};
use prop_amm_shared::config::SimulationConfig;
use prop_amm_shared::instruction::decode_after_swap;
use prop_amm_shared::nano::nano_to_f64;
use prop_amm_shared::normalizer;
use prop_amm_shared::result::{BatchResult, SimResult};
use prop_amm_sim::engine;

/// Which AMM a recorder slot observes. `#[repr(usize)]` so it converts straight to an array
/// index, matching `compile.rs`'s own `Slot` convention.
#[derive(Clone, Copy, Debug)]
#[repr(usize)]
enum AmmSlot {
    Submission = 0,
    Normalizer = 1,
}

const N_SLOTS: usize = 2;

/// Per-simulation, per-AMM executed-volume counters (docs/DESIGN.md §2.7 L1 observability).
/// Volume is denominated in Y (the quote token) — the same unit as `submission_edge` — so
/// edge-per-unit-volume is a meaningful ratio: the Y-side amount of a trade is `input_amount`
/// on a buy-X (side 0, Y in) and `output_amount` on a sell-X (side 1, Y out).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VolumeCounters {
    pub trade_count: u64,
    pub volume_y: f64,
}

thread_local! {
    static COUNTERS: RefCell<[VolumeCounters; N_SLOTS]> = RefCell::new([VolumeCounters::default(); N_SLOTS]);
}

/// Resets this thread's counters to zero. Call immediately before driving one simulation, so
/// the read after that simulation reflects exactly it — "counters reset at a known
/// simulation boundary" (the ticket's own phrasing).
fn reset_thread_counters() {
    COUNTERS.with(|c| *c.borrow_mut() = [VolumeCounters::default(); N_SLOTS]);
}

fn take_thread_counters() -> [VolumeCounters; N_SLOTS] {
    COUNTERS.with(|c| {
        let counters = *c.borrow();
        *c.borrow_mut() = [VolumeCounters::default(); N_SLOTS];
        counters
    })
}

// The real `after_swap` to delegate to per slot, if the loaded candidate (or the normalizer)
// has one — `null` means "no real after_swap; the recorder is a pure observer that must not
// touch storage". A plain `AtomicPtr`, not a closure, for the same reason `compile.rs` uses
// one for `LOADED_AFTER_SWAP`: `AfterSwapFn` is a bare `fn` pointer with no room for captured
// state. Set once, single-threaded, before the parallel batch starts; read-only afterwards.
static REAL_AFTER_SWAP: [AtomicPtr<()>; N_SLOTS] = [
    AtomicPtr::new(std::ptr::null_mut()),
    AtomicPtr::new(std::ptr::null_mut()),
];

type FfiAfterSwapFn = AfterSwapFn;

fn install_real(slot: AmmSlot, real: Option<AfterSwapFn>) {
    let ptr = match real {
        Some(f) => f as *mut (),
        None => std::ptr::null_mut(),
    };
    REAL_AFTER_SWAP[slot as usize].store(ptr, Ordering::Relaxed);
}

/// Installs the real `after_swap` (if any) each recorder slot must delegate to. Must run
/// single-threaded, before the parallel batch that uses the recorders — mirrors
/// `compile.rs::load_native`'s own one-time, pre-parallel store into `LOADED_AFTER_SWAP`.
fn install(submission_real: Option<AfterSwapFn>, normalizer_real: Option<AfterSwapFn>) {
    install_real(AmmSlot::Submission, submission_real);
    install_real(AmmSlot::Normalizer, normalizer_real);
}

/// Records the Y-side volume of this trade, then delegates unchanged to the real
/// `after_swap` for `slot`, if one was installed — a pure pass-through. This is the entire
/// non-invasiveness contract (docs/DESIGN.md §2.7): the wrapper must not alter `storage`
/// beyond what the real `after_swap` would have done on its own, and must run when there is
/// no real `after_swap` too (a stateless candidate still trades volume worth recording).
fn record_and_delegate(slot: AmmSlot, data: &[u8], storage: &mut [u8]) {
    let (side, input_amount, output_amount, _rx, _ry, _step, _storage) = decode_after_swap(data);
    let volume_y = if side == 0 {
        nano_to_f64(input_amount)
    } else {
        nano_to_f64(output_amount)
    };

    COUNTERS.with(|c| {
        let mut counters = c.borrow_mut();
        let entry = &mut counters[slot as usize];
        entry.trade_count += 1;
        entry.volume_y += volume_y;
    });

    let ptr = REAL_AFTER_SWAP[slot as usize].load(Ordering::Relaxed);
    if !ptr.is_null() {
        let real: FfiAfterSwapFn = unsafe { std::mem::transmute(ptr) };
        real(data, storage);
    }
}

fn submission_recorder(data: &[u8], storage: &mut [u8]) {
    record_and_delegate(AmmSlot::Submission, data, storage)
}

fn normalizer_recorder(data: &[u8], storage: &mut [u8]) {
    record_and_delegate(AmmSlot::Normalizer, data, storage)
}

/// One simulation's L1 observability record (docs/DESIGN.md §2.7): the graded edge, plus
/// each AMM's recorded trade count and Y-volume.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct L1Sim {
    pub seed: u64,
    pub submission_edge: f64,
    pub submission_trades: u64,
    pub submission_volume: f64,
    pub normalizer_trades: u64,
    pub normalizer_volume: f64,
}

/// Our share of total executed Y-volume — `None` when neither AMM traded (both volumes are
/// zero), rather than a division producing `NaN`. When defined, always in `[0, 1]`: both
/// inputs are sums of non-negative amounts.
pub fn flow_share(submission_volume: f64, normalizer_volume: f64) -> Option<f64> {
    let total = submission_volume + normalizer_volume;
    if total <= 0.0 {
        None
    } else {
        Some(submission_volume / total)
    }
}

/// Captured spread per unit of our own flow — `None` when we recorded no volume (division by
/// zero would otherwise silently produce `inf`/`NaN`).
pub fn edge_per_volume(submission_edge: f64, submission_volume: f64) -> Option<f64> {
    if submission_volume <= 0.0 {
        None
    } else {
        Some(submission_edge / submission_volume)
    }
}

/// Runs `configs` against `submission_fn`/`submission_after_swap` and the fixed normalizer
/// opponent (matching `compile.rs::LoadedNative::run_batch`'s own hardcoding), recording L1
/// telemetry for both AMMs. Drives `engine::run_simulation_native` directly, one config per
/// rayon task, resetting/reading the thread-local counters immediately around each call so
/// they reset at a known simulation boundary (docs/DESIGN.md §2.7) rather than accumulating
/// across an entire batch the way `runner::run_batch_native` would offer no way to inspect
/// per-simulation.
pub fn run_batch_native_with_l1(
    submission_fn: SwapFn,
    submission_after_swap: Option<AfterSwapFn>,
    configs: Vec<SimulationConfig>,
) -> anyhow::Result<(BatchResult, Vec<L1Sim>)> {
    install(submission_after_swap, Some(normalizer::after_swap));

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(rayon::current_num_threads().min(8))
        .build()?;

    let pairs: Result<Vec<(SimResult, L1Sim)>, anyhow::Error> = pool.install(|| {
        configs
            .par_iter()
            .map(|config| -> anyhow::Result<(SimResult, L1Sim)> {
                reset_thread_counters();
                let result = engine::run_simulation_native(
                    submission_fn,
                    Some(submission_recorder),
                    normalizer::compute_swap,
                    Some(normalizer_recorder),
                    config,
                )?;
                let counters = take_thread_counters();
                let l1 = L1Sim {
                    seed: result.seed,
                    submission_edge: result.submission_edge,
                    submission_trades: counters[AmmSlot::Submission as usize].trade_count,
                    submission_volume: counters[AmmSlot::Submission as usize].volume_y,
                    normalizer_trades: counters[AmmSlot::Normalizer as usize].trade_count,
                    normalizer_volume: counters[AmmSlot::Normalizer as usize].volume_y,
                };
                Ok((result, l1))
            })
            .collect()
    });

    let (results, l1_sims): (Vec<SimResult>, Vec<L1Sim>) = pairs?.into_iter().unzip();
    Ok((BatchResult::from_results(results), l1_sims))
}

#[cfg(test)]
mod tests {
    use super::*;
    use prop_amm_shared::config::HyperparameterVariance;

    fn tiny_configs(n: u64) -> Vec<SimulationConfig> {
        let base = SimulationConfig {
            n_steps: 50,
            ..SimulationConfig::default()
        };
        let variance = HyperparameterVariance::default();
        (0..n).map(|seed| variance.apply(&base, seed)).collect()
    }

    /// The non-invasiveness acceptance criterion, proven without any real dylib compile: the
    /// same swap fn over the same configs must yield a bit-identical `total_edge` whether
    /// driven through the plain `runner::run_batch_native` (no telemetry) or through
    /// `run_batch_native_with_l1` (telemetry on). A synthetic swap fn stands in for a
    /// compiled candidate — the property being tested is the recorder's pass-through
    /// behaviour, which doesn't depend on which curve is loaded.
    #[test]
    fn telemetry_is_bit_identical_to_no_telemetry() {
        let configs = tiny_configs(8);

        let without_telemetry = prop_amm_sim::runner::run_batch_native(
            normalizer::compute_swap,
            None,
            normalizer::compute_swap,
            Some(normalizer::after_swap),
            configs.clone(),
            None,
        )
        .unwrap();

        let (with_telemetry, l1_sims) =
            run_batch_native_with_l1(normalizer::compute_swap, None, configs).unwrap();

        assert_eq!(without_telemetry.total_edge, with_telemetry.total_edge);
        assert_eq!(
            without_telemetry.results.len(),
            with_telemetry.results.len()
        );
        for (a, b) in without_telemetry
            .results
            .iter()
            .zip(with_telemetry.results.iter())
        {
            assert_eq!(a.seed, b.seed);
            assert_eq!(a.submission_edge, b.submission_edge);
        }
        assert_eq!(l1_sims.len(), with_telemetry.results.len());
    }

    #[test]
    fn telemetry_records_trades_on_both_slots_when_symmetric() {
        let configs = tiny_configs(4);
        let (_batch, l1_sims) =
            run_batch_native_with_l1(normalizer::compute_swap, None, configs).unwrap();

        for sim in &l1_sims {
            assert!(
                sim.submission_trades > 0,
                "seed {} traded nothing",
                sim.seed
            );
            assert!(
                sim.normalizer_trades > 0,
                "seed {} traded nothing",
                sim.seed
            );
            assert!(sim.submission_volume > 0.0);
            assert!(sim.normalizer_volume > 0.0);
        }
    }

    #[test]
    fn flow_share_is_none_when_no_volume() {
        assert_eq!(flow_share(0.0, 0.0), None);
    }

    #[test]
    fn flow_share_is_bounded_in_zero_one() {
        assert_eq!(flow_share(3.0, 1.0), Some(0.75));
        assert_eq!(flow_share(0.0, 5.0), Some(0.0));
        assert_eq!(flow_share(5.0, 0.0), Some(1.0));
    }

    #[test]
    fn edge_per_volume_is_none_when_no_volume() {
        assert_eq!(edge_per_volume(10.0, 0.0), None);
    }

    #[test]
    fn edge_per_volume_divides_edge_by_volume() {
        assert_eq!(edge_per_volume(10.0, 2.0), Some(5.0));
    }
}
