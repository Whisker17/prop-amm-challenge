//! `bench fuzz` (WHI-1212, docs/DESIGN.md §2.9): a pre-search shape-fuzz gate. `prop-amm
//! validate` (`crates/cli/src/commands/validate.rs`) only probes 10 sizes {0.1..200} at one
//! fixed state (`rx=100, ry=10000`, zeroed storage) — a far weaker check than the
//! ~10^7-instance exercise a real 1000-sim run puts a submission through. This module
//! hammers a candidate much harder, and *before* a frozen search spends paired-seed budget
//! on it, with:
//!
//! - dense sweeps (several grid shapes, from just above the minimum tradable input up to
//!   `MAX_INPUT_AMOUNT`) at each of many synthetic `(reserve_x, reserve_y, storage)` states,
//! - golden-section-shaped sample sets against several synthetic fair prices, matching the
//!   dense-near-the-optimum pattern `crates/sim/src/arbitrageur.rs`'s
//!   `bracket_maximum`/`golden_section_max` and `crates/sim/src/router.rs`'s own alpha-split
//!   golden section actually produce (re-implemented here, not imported — both are private
//!   methods on crate-private search state, and this is a faithful-shape mirror, not a
//!   byte-for-byte port of either's two-stage algorithm; see `docs/DEFERRED_ISSUES.md` for
//!   the residual gap that leaves open),
//! - states drawn from all of `config/bench.toml`'s `[grid]` regime corners, including
//!   states only reachable after a full-length GBM drift (very small and very large spot),
//!   each in both an exact-CPMM-invariant and a randomly jittered, off-invariant variant,
//! - a zeroed and a random-byte storage variant per state (mirrors `validate.rs`'s own
//!   randomized reserve/storage probe — issue WHI-1212's own rationale: "it is where clamp
//!   bugs surface").
//!
//! Every check ultimately runs through `curve_checks::submission_shape_violation` — this
//! module's mirror of `crates/sim/src/curve_checks.rs`'s private function of the same name;
//! see that file's own header comment for why it must be a copy, not an import.

use prop_amm_shared::config::{
    BASELINE_STEPS, GBM_DT, GBM_MU, INITIAL_PRICE, INITIAL_X, INITIAL_Y,
};
use prop_amm_shared::instruction::STORAGE_SIZE;
use prop_amm_shared::nano::NANO_SCALE_F64;
use prop_amm_sim::price_process::GBMPriceProcess;

use crate::config::{FuzzConfig, GridConfig};
use crate::curve_checks::submission_shape_violation;
use crate::fast_compile::LoadedFast;
use crate::grid;

/// Mirrors `crates/sim/src/arbitrageur.rs`'s own `MIN_INPUT`/`crates/sim/src/router.rs`'s
/// `MIN_TRADE_SIZE` — the smallest input either real search ever evaluates.
const MIN_INPUT: f64 = 1e-3;
/// Mirrors `crates/sim/src/arbitrageur.rs::MAX_INPUT_AMOUNT` — the largest input an
/// arbitrageur's search is ever allowed to reach.
const MAX_INPUT_AMOUNT: f64 = (u64::MAX as f64 / NANO_SCALE_F64) * 0.999_999;
/// The golden ratio conjugate itself — not a tunable, a mathematical constant golden-section
/// search requires; mirrored from `crates/sim/src/arbitrageur.rs`/`router.rs` because both
/// use the same value, not because this module's search is a literal port of either (see
/// this module's own header comment and `docs/DEFERRED_ISSUES.md`).
const GOLDEN_RATIO_CONJUGATE: f64 = 0.618_033_988_749_894_8;
/// Cluster-grid power values for the dense sweep's two power-clustered grid shapes (toward
/// the low end and the high end respectively) — an algorithm-shape choice independent of
/// `[fuzz]`'s own sample-*count* tunables, same bucket as `GOLDEN_RATIO_CONJUGATE` above and
/// as upstream's own hardcoded search constants (`arbitrageur.rs::BRACKET_GROWTH`, etc.),
/// not a value this module claims fidelity to any specific upstream number for.
const CLUSTER_GRID_POWER_LOW: f64 = 2.4;
const CLUSTER_GRID_POWER_HIGH: f64 = 0.45;
/// Not a `config/bench.toml` segment and not a decision input — purely an internal RNG seed
/// for exploring a regime's GBM drift range when building fuzz states, disjoint from grid
/// mode's own `GRID_SEED_BASE` (`tools/bench/src/grid.rs`) and from `FUZZ_STATE_SEED_BASE`
/// below only so the three are never confused when reading a seed value in a debugger.
const FUZZ_PRICE_SEED_BASE: u64 = 5_000_000;
/// Seeds the random-byte storage buffer and the off-invariant reserve jitter
/// (`storage_seed`/`jitter_seed_x`/`jitter_seed_y` below) — a distinct base from
/// `FUZZ_PRICE_SEED_BASE` so the two purposes never share a literal seed value for the same
/// cell.
const FUZZ_STATE_SEED_BASE: u64 = 6_000_000;
/// Offset added to `FUZZ_STATE_SEED_BASE` for jitter seeds, keeping that namespace disjoint
/// from the storage-seed namespace below it.
const FUZZ_JITTER_SEED_OFFSET: u64 = 1_000_000;

const SIDES: [(u8, &str); 2] = [(0, "buy X (input Y)"), (1, "sell X (input X)")];

/// One synthetic, frozen `(reserve_x, reserve_y, storage)` triple to probe.
#[derive(Debug, Clone)]
pub struct FuzzState {
    pub label: String,
    pub reserve_x: f64,
    pub reserve_y: f64,
    pub storage: Vec<u8>,
}

/// The first shape violation `run_fuzz` found, if any.
#[derive(Debug, Clone)]
pub struct Violation {
    pub state_label: String,
    pub side_label: &'static str,
    pub sample_kind: &'static str,
    pub message: String,
}

/// Builds the fuzz states: every regime corner in `grid_config` (`config/bench.toml`'s
/// `[grid]` table — the same 27-cell factorial `bench grid` uses), each contributing the
/// reserves implied by its own initial price and by the extremes of a full-length GBM price
/// path sampled under that regime's own sigma. Each `(cell, price)` pair contributes both
/// the exact-CPMM-invariant reserve pair and a randomly jittered, off-invariant, asymmetric
/// one (issue WHI-1212 asks for "*random* states", and WHI-1206's own reserve-clamp-plateau
/// failure is specifically an off-invariant state) — each in turn paired with a zeroed and a
/// random-byte storage buffer. `norm_fee_bps` (the third grid axis) has no principled effect
/// on a *submission's own* reserves — it only ever governs the normalizer counterpart's
/// curve — so it distinguishes states here only through jitter/storage seeding (every cell
/// still gets its own distinct states; it just isn't the reserve-scale axis liquidity and
/// sigma are).
pub fn build_states(grid_config: &GridConfig, fuzz_config: &FuzzConfig) -> Vec<FuzzState> {
    let mut states = Vec::new();
    for cell in grid::cells(grid_config) {
        let liquidity = INITIAL_X.max(1e-9) * cell.norm_liquidity_mult;
        let liquidity_y = INITIAL_Y.max(1e-9) * cell.norm_liquidity_mult;
        let k = liquidity * liquidity_y;

        let (min_price, max_price) =
            drift_extremes(cell.gbm_sigma, cell.index, fuzz_config.seeds_per_regime);

        let mut prices = vec![INITIAL_PRICE, min_price, max_price];
        prices.retain(|p| p.is_finite() && *p > 0.0);
        prices.dedup_by(|a, b| (*a - *b).abs() < 1e-12);

        for (price_idx, price) in prices.into_iter().enumerate() {
            let on_invariant_x = (k / price).sqrt();
            let on_invariant_y = (k * price).sqrt();
            if !on_invariant_x.is_finite()
                || !on_invariant_y.is_finite()
                || on_invariant_x <= 0.0
                || on_invariant_y <= 0.0
            {
                continue;
            }

            let jitter_x = jitter_factor(jitter_seed_x(cell.index, price_idx));
            let jitter_y = jitter_factor(jitter_seed_y(cell.index, price_idx));
            let reserve_variants = [
                (on_invariant_x, on_invariant_y, "on-invariant"),
                (
                    on_invariant_x * jitter_x,
                    on_invariant_y * jitter_y,
                    "off-invariant jitter",
                ),
            ];

            for (reserve_x, reserve_y, reserve_label) in reserve_variants {
                if !reserve_x.is_finite()
                    || !reserve_y.is_finite()
                    || reserve_x <= 0.0
                    || reserve_y <= 0.0
                {
                    continue;
                }

                for (storage, storage_label) in [
                    (vec![0u8; STORAGE_SIZE], "zero storage"),
                    (
                        random_storage(storage_seed(cell.index, price_idx)),
                        "random storage",
                    ),
                ] {
                    states.push(FuzzState {
                        label: format!(
                            "cell {} ({}) price={price:.6} [{reserve_label}, {storage_label}]",
                            cell.index,
                            cell.label_axes(),
                        ),
                        reserve_x,
                        reserve_y,
                        storage,
                    });
                }
            }
        }
    }
    states
}

/// `random_storage` consumes `STORAGE_SIZE` (1024) consecutive `mix()` inputs starting at
/// its seed, so any two storage seeds this function returns must differ by at least
/// `STORAGE_SIZE` or their byte streams overlap — hence a stride well above it, with a
/// per-cell stride that's itself a multiple of the per-price stride (room for a few more
/// price points per cell than the 3 `build_states` ever produces, with margin).
const SEED_STRIDE_PER_PRICE: u64 = 4_096;
const SEED_STRIDE_PER_CELL: u64 = SEED_STRIDE_PER_PRICE * 8;

fn storage_seed(cell_index: usize, price_idx: usize) -> u64 {
    FUZZ_STATE_SEED_BASE
        + (cell_index as u64) * SEED_STRIDE_PER_CELL
        + (price_idx as u64) * SEED_STRIDE_PER_PRICE
}

fn jitter_seed_x(cell_index: usize, price_idx: usize) -> u64 {
    FUZZ_STATE_SEED_BASE
        + FUZZ_JITTER_SEED_OFFSET
        + (cell_index as u64) * SEED_STRIDE_PER_CELL
        + (price_idx as u64) * SEED_STRIDE_PER_PRICE
}

/// Offset by half a price-stride from `jitter_seed_x` — `jitter_factor` only ever reads a
/// single `mix()` value (unlike `random_storage`'s 1024-wide window), so this only needs to
/// avoid landing on another `(cell, price)`'s exact seed, not a wide non-overlap margin;
/// `jitter_seed_x(c, p).wrapping_add(1)` would have aliased `jitter_seed_x(c, p + 1)` at the
/// old stride of 1 — this offset can't alias any `jitter_seed_x` call for any cell/price.
fn jitter_seed_y(cell_index: usize, price_idx: usize) -> u64 {
    jitter_seed_x(cell_index, price_idx) + SEED_STRIDE_PER_PRICE / 2
}

/// Maps a seed to a multiplicative factor in `[0.4, 2.5]` — wide enough that applying it
/// independently to `reserve_x` and `reserve_y` lands off the CPMM invariant the
/// "on-invariant" state sits on, narrow enough to stay a plausible reserve state.
fn jitter_factor(seed: u64) -> f64 {
    let unit = (mix(seed) % 1_000_000) as f64 / 1_000_000.0;
    0.4 + unit * (2.5 - 0.4)
}

/// The min/max fair price a full-length GBM path reaches under `sigma`, across
/// `seeds`-many independent price paths — the "states only reachable after long GBM drift"
/// the ticket calls out, without needing to actually run the candidate first.
fn drift_extremes(sigma: f64, cell_index: usize, seeds: u64) -> (f64, f64) {
    let mut min_price = INITIAL_PRICE;
    let mut max_price = INITIAL_PRICE;
    for i in 0..seeds {
        let seed = FUZZ_PRICE_SEED_BASE + (cell_index as u64) * 1_000 + i;
        let mut price = GBMPriceProcess::new(INITIAL_PRICE, GBM_MU, sigma, GBM_DT, seed);
        for _ in 0..BASELINE_STEPS {
            let p = price.step();
            if p.is_finite() && p > 0.0 {
                min_price = min_price.min(p);
                max_price = max_price.max(p);
            }
        }
    }
    (min_price, max_price)
}

/// A `mix()`-hashed pseudo-random buffer — the same xorshift-multiply mix
/// `crates/cli/src/commands/validate.rs` already uses for its own randomized
/// reserve/storage probe.
fn random_storage(seed: u64) -> Vec<u8> {
    (0..STORAGE_SIZE)
        .map(|i| (mix(seed.wrapping_add(i as u64)) & 0xFF) as u8)
        .collect()
}

#[inline]
fn mix(mut z: u64) -> u64 {
    z ^= z >> 30;
    z = z.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z ^= z >> 27;
    z = z.wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Runs the full fuzz sweep — dense sweeps plus golden-section-shaped sample sets, both
/// sides, every state — and returns the first violation found, or `None` if the candidate
/// is clean across all of it.
pub fn run_fuzz(
    loaded: &LoadedFast,
    states: &[FuzzState],
    fuzz_config: &FuzzConfig,
) -> Option<Violation> {
    // (sample kind label, min input, max input) for the two dense sweeps — a table instead
    // of two near-identical `if let` blocks below.
    let dense_sweeps: [(&str, f64, f64); 2] = [
        (
            "dense sweep (moderate range)",
            MIN_INPUT,
            fuzz_config.moderate_max_input,
        ),
        (
            "dense sweep (up to MAX_INPUT_AMOUNT)",
            MIN_INPUT,
            MAX_INPUT_AMOUNT,
        ),
    ];

    for state in states {
        for (side, side_label) in SIDES {
            let mut quote = |input: f64| {
                loaded.quote(
                    side,
                    input,
                    state.reserve_x,
                    state.reserve_y,
                    &state.storage,
                )
            };

            for (sample_kind, min_input, max_input) in dense_sweeps {
                if let Some(message) = dense_sweep_violation(
                    &mut quote,
                    min_input,
                    max_input,
                    fuzz_config.dense_sweep_points,
                ) {
                    return Some(Violation {
                        state_label: state.label.clone(),
                        side_label,
                        sample_kind,
                        message,
                    });
                }
            }

            let spot = state.reserve_y / state.reserve_x.max(1e-12);
            if let Some(message) = golden_section_violation(
                &mut quote,
                side,
                spot,
                MIN_INPUT,
                MAX_INPUT_AMOUNT,
                &fuzz_config.golden_price_multipliers,
                fuzz_config.golden_max_iters,
            ) {
                return Some(Violation {
                    state_label: state.label.clone(),
                    side_label,
                    sample_kind: "golden-section-shaped sample set",
                    message,
                });
            }
        }
    }
    None
}

/// Several grid shapes spanning `[min_input, max_input]` — linear, geometric, and two
/// power-clustered variants (toward the low end and the high end) — in the same spirit as
/// `crates/sim/src/curve_checks.rs`'s own test module's grid-shape helpers, but this is a
/// separate, purpose-built production sweep generator for `bench fuzz`, not a fidelity-bound
/// mirror of those test-only helpers — its constants are free to differ from theirs.
fn dense_sweep_grids(min_input: f64, max_input: f64, n: usize) -> Vec<Vec<f64>> {
    vec![
        linear_grid(min_input, max_input, n),
        geometric_grid(min_input, max_input, n),
        clustered_grid(min_input, max_input, n, CLUSTER_GRID_POWER_LOW),
        clustered_grid(min_input, max_input, n, CLUSTER_GRID_POWER_HIGH),
    ]
}

fn linear_grid(min_input: f64, max_input: f64, n: usize) -> Vec<f64> {
    let start = min_input * 1.01;
    let span = (max_input - start).max(1e-9);
    (0..n)
        .map(|i| {
            let t = i as f64 / (n.saturating_sub(1).max(1)) as f64;
            start + t * span
        })
        .collect()
}

fn geometric_grid(min_input: f64, max_input: f64, n: usize) -> Vec<f64> {
    let start = min_input * 1.01;
    let ratio = (max_input / start)
        .max(1.0)
        .powf(1.0 / (n.saturating_sub(1).max(1)) as f64);
    (0..n).map(|i| start * ratio.powf(i as f64)).collect()
}

fn clustered_grid(min_input: f64, max_input: f64, n: usize, power: f64) -> Vec<f64> {
    let start = min_input * 1.01;
    let span = (max_input - start).max(1e-9);
    (0..n)
        .map(|i| {
            let t = i as f64 / (n.saturating_sub(1).max(1)) as f64;
            start + t.powf(power) * span
        })
        .collect()
}

/// Quotes every point of every dense-sweep grid and checks each grid's points as its own
/// batch (a real violation shows up within a single grid; merging unrelated grids together
/// would only blur which sweep actually found it).
fn dense_sweep_violation<F: FnMut(f64) -> f64>(
    quote: &mut F,
    min_input: f64,
    max_input: f64,
    n: usize,
) -> Option<String> {
    for grid in dense_sweep_grids(min_input, max_input, n) {
        let points: Vec<(f64, f64)> = grid.iter().map(|&input| (input, quote(input))).collect();
        if let Some(message) = submission_shape_violation(&points, min_input) {
            return Some(message);
        }
    }
    None
}

/// Runs a golden-section maximization of a profit-like objective over `[min_input,
/// max_input]`, returning every `(input, output)` pair evaluated along the way — the same
/// dense-near-the-optimum sampling shape a real arbitrage or router split search produces.
fn golden_section_sample<F: FnMut(f64) -> f64>(
    quote: &mut F,
    side: u8,
    fair_price: f64,
    min_input: f64,
    max_input: f64,
    max_iters: usize,
) -> Vec<(f64, f64)> {
    let score = |input: f64, output: f64| -> f64 {
        if side == 0 {
            output * fair_price - input
        } else {
            output - input * fair_price
        }
    };

    let left0 = min_input.max(0.0);
    let right0 = max_input.max(left0 * 1.000_001).max(MIN_INPUT);
    let mut points = Vec::with_capacity(max_iters + 4);

    let out_left = quote(left0);
    points.push((left0, out_left));
    let out_right = quote(right0);
    points.push((right0, out_right));

    let mut left = left0;
    let mut right = right0;
    let mut x1 = right - GOLDEN_RATIO_CONJUGATE * (right - left);
    let mut x2 = left + GOLDEN_RATIO_CONJUGATE * (right - left);
    let out1 = quote(x1);
    let out2 = quote(x2);
    points.push((x1, out1));
    points.push((x2, out2));
    let mut f1 = score(x1, out1);
    let mut f2 = score(x2, out2);

    for _ in 0..max_iters {
        if f1 < f2 {
            left = x1;
            x1 = x2;
            f1 = f2;
            x2 = left + GOLDEN_RATIO_CONJUGATE * (right - left);
            let out = quote(x2);
            points.push((x2, out));
            f2 = score(x2, out);
        } else {
            right = x2;
            x2 = x1;
            f2 = f1;
            x1 = right - GOLDEN_RATIO_CONJUGATE * (right - left);
            let out = quote(x1);
            points.push((x1, out));
            f1 = score(x1, out);
        }
    }

    points
}

/// Runs `golden_section_sample` against several fair prices, relative to `spot` — mispricings
/// an arbitrageur would actually chase — and checks each run's points as its own batch.
fn golden_section_violation<F: FnMut(f64) -> f64>(
    quote: &mut F,
    side: u8,
    spot: f64,
    min_input: f64,
    max_input: f64,
    multipliers: &[f64],
    max_iters: usize,
) -> Option<String> {
    for &multiplier in multipliers {
        let fair_price = (spot * multiplier).max(1e-12);
        let points =
            golden_section_sample(quote, side, fair_price, min_input, max_input, max_iters);
        if let Some(message) = submission_shape_violation(&points, min_input) {
            return Some(message);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GridConfig;

    fn sample_grid_config() -> GridConfig {
        GridConfig {
            norm_fee_bps_levels: vec![30, 55, 80],
            norm_liquidity_mult_levels: vec![0.4, 1.0, 2.0],
            gbm_sigma_levels: vec![1e-4, 1e-3, 7e-3],
            seeds_per_cell: 40,
        }
    }

    fn sample_fuzz_config() -> FuzzConfig {
        FuzzConfig {
            dense_sweep_points: 40,
            seeds_per_regime: 2,
            golden_price_multipliers: vec![0.5, 1.0, 2.0],
            moderate_max_input: 50_000.0,
            golden_max_iters: 20,
        }
    }

    #[test]
    fn build_states_covers_every_cell_with_finite_positive_reserves() {
        let grid_config = sample_grid_config();
        let fuzz_config = sample_fuzz_config();
        let states = build_states(&grid_config, &fuzz_config);

        let n_cells = grid_config.norm_fee_bps_levels.len()
            * grid_config.norm_liquidity_mult_levels.len()
            * grid_config.gbm_sigma_levels.len();
        // Every cell contributes at least its initial-price state (min/max drift can
        // collapse onto the same price and get deduped), each in an on-invariant and an
        // off-invariant-jitter reserve variant, each in two storage variants.
        assert!(states.len() >= n_cells * 4);

        for state in &states {
            assert!(state.reserve_x.is_finite() && state.reserve_x > 0.0);
            assert!(state.reserve_y.is_finite() && state.reserve_y > 0.0);
            assert_eq!(state.storage.len(), STORAGE_SIZE);
        }
    }

    #[test]
    fn build_states_includes_both_storage_variants() {
        let states = build_states(&sample_grid_config(), &sample_fuzz_config());
        assert!(states.iter().any(|s| s.storage.iter().all(|&b| b == 0)));
        assert!(states.iter().any(|s| s.storage.iter().any(|&b| b != 0)));
    }

    #[test]
    fn build_states_includes_an_off_invariant_jittered_reserve_pair() {
        let grid_config = sample_grid_config();
        let states = build_states(&grid_config, &sample_fuzz_config());
        let cell = grid::cells(&grid_config)[0];
        let k = (INITIAL_X * cell.norm_liquidity_mult) * (INITIAL_Y * cell.norm_liquidity_mult);
        let on_invariant = (k / INITIAL_PRICE).sqrt();

        // At least one state at the initial price departs from the exact CPMM invariant —
        // "many random states", not just the two deterministic on-invariant reserve scales.
        assert!(states
            .iter()
            .any(|s| (s.reserve_x - on_invariant).abs() > 1e-6));
    }

    #[test]
    fn jitter_seed_distinguishes_cells_that_differ_only_in_fee() {
        // norm_fee_bps has no effect on a submission's own reserves (it only governs the
        // normalizer counterpart), so two cells differing only in fee would otherwise be
        // indistinguishable at the reserve level — `cell.index` is what still gives every
        // one of the 27 grid cells its own distinct jittered reserve state.
        let seed_a = jitter_seed_x(0, 0);
        let seed_b = jitter_seed_x(1, 0);
        assert_ne!(seed_a, seed_b);
        assert!((jitter_factor(seed_a) - jitter_factor(seed_b)).abs() > 1e-9);
    }

    #[test]
    fn jitter_seed_x_and_y_never_alias_each_other_or_a_neighboring_price_index() {
        // Regression guard: an earlier version derived jitter_seed_y as
        // `jitter_seed_x(c, p).wrapping_add(1)`, which at the old stride-1 spacing equaled
        // `jitter_seed_x(c, p + 1)` exactly.
        let mut seeds = std::collections::HashSet::new();
        for cell_index in 0..27usize {
            for price_idx in 0..3usize {
                assert!(seeds.insert(jitter_seed_x(cell_index, price_idx)));
                assert!(seeds.insert(jitter_seed_y(cell_index, price_idx)));
            }
        }
    }

    #[test]
    fn storage_seed_windows_never_overlap() {
        // Regression guard: an earlier version used a stride of 100 for both cell and price
        // index, far below `STORAGE_SIZE` (1024) — `random_storage`'s 1024-value window from
        // two different (cell, price) pairs would then overlap almost entirely.
        let mut windows = Vec::new();
        for cell_index in 0..27usize {
            for price_idx in 0..3usize {
                let start = storage_seed(cell_index, price_idx);
                windows.push((start, start + STORAGE_SIZE as u64 - 1));
            }
        }
        windows.sort();
        for pair in windows.windows(2) {
            let (_, end_a) = pair[0];
            let (start_b, _) = pair[1];
            assert!(
                end_a < start_b,
                "storage seed windows overlap: {:?} vs {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn drift_extremes_widen_around_the_initial_price_for_nonzero_sigma() {
        let (min_price, max_price) = drift_extremes(0.01, 0, 4);
        assert!(min_price < INITIAL_PRICE);
        assert!(max_price > INITIAL_PRICE);
    }

    #[test]
    fn dense_sweep_accepts_a_concave_curve() {
        let mut quote = |x: f64| (1.0 + x).ln();
        assert!(dense_sweep_violation(&mut quote, MIN_INPUT, 1_000.0, 100).is_none());
    }

    #[test]
    fn dense_sweep_finds_an_inverted_kink_a_ten_point_probe_would_miss() {
        // Straight up to x=500 (validate.rs's own probe tops out at 200), then a *steeper*
        // slope afterwards — WHI-1207's failure shape.
        let mut quote = |x: f64| {
            if x <= 500.0 {
                x * 2.0
            } else {
                1_000.0 + (x - 500.0) * 5.0
            }
        };
        let message = dense_sweep_violation(&mut quote, MIN_INPUT, 2_000.0, 200)
            .expect("dense sweep should catch the inverted kink");
        assert!(
            message.contains("concavity"),
            "unexpected message: {message}"
        );
    }

    #[test]
    fn golden_section_sample_produces_a_dense_near_optimum_point_set() {
        let mut quote = |x: f64| (1.0 + x).ln();
        let max_iters = 20;
        let points = golden_section_sample(&mut quote, 0, 1.0, MIN_INPUT, 1_000.0, max_iters);
        assert!(points.len() >= max_iters);
        assert!(points.iter().all(|(x, _)| x.is_finite() && *x >= 0.0));
    }

    #[test]
    fn golden_section_violation_catches_non_monotone_curve() {
        let mut quote = |x: f64| {
            if x < 5.0 {
                x
            } else {
                10.0 - x
            }
        };
        let message = golden_section_violation(&mut quote, 0, 1.0, MIN_INPUT, 20.0, &[1.0], 20)
            .expect("expected violation");
        assert!(
            message.contains("monotonicity"),
            "unexpected message: {message}"
        );
    }

    /// End-to-end acceptance check (WHI-1212): `run_fuzz` against the real committed
    /// `strategies/001-cpmm-fee` point reports zero violations, and against the
    /// deliberately broken `tools/bench/tests/fixtures/broken_inverted_kink` fixture it
    /// reports one, naming the offending input pair. Both scenarios share **one** test
    /// function on purpose: `fast_compile::compile_and_load_fast` loads a candidate into a
    /// process-global static (`LOADED_SWAP`/`LOADED_AFTER_SWAP`, one slot total — see that
    /// module's own doc comment), so two separate `#[test]` functions calling it would race
    /// under `cargo test`'s default parallel execution; running both loads sequentially in
    /// one function sidesteps that entirely.
    #[test]
    fn run_fuzz_end_to_end_against_a_real_candidate_and_a_known_broken_fixture() {
        use std::path::Path;

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let grid_config = sample_grid_config();
        let fuzz_config = sample_fuzz_config();
        let states = build_states(&grid_config, &fuzz_config);

        let load = |relative_path: &str| {
            let source = std::fs::read_to_string(repo_root.join(relative_path))
                .unwrap_or_else(|e| panic!("failed to read {relative_path}: {e}"));
            let safe_source = crate::fast_compile::make_safe_source(&source)
                .unwrap_or_else(|e| panic!("failed to prepare {relative_path}: {e}"));
            crate::fast_compile::compile_and_load_fast(&safe_source)
                .unwrap_or_else(|e| panic!("failed to build {relative_path}: {e}"))
        };

        let cpmm_fee = load("strategies/001-cpmm-fee/lib.rs");
        assert!(
            run_fuzz(&cpmm_fee, &states, &fuzz_config).is_none(),
            "committed 001-cpmm-fee point should report zero shape violations"
        );

        let broken = load("tools/bench/tests/fixtures/broken_inverted_kink/lib.rs");
        let violation = run_fuzz(&broken, &states, &fuzz_config)
            .expect("the deliberately broken fixture should report a shape violation");
        assert!(
            violation.message.contains("concavity"),
            "unexpected violation message: {}",
            violation.message
        );
        assert!(!violation.state_label.is_empty());
    }
}
