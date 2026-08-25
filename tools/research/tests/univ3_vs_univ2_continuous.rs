//! Full-range zero-fee Uniswap V3 versus zero-fee Uniswap V2, under continuous
//! state evolution.
//!
//! A single-state spot check found no difference between the two at the
//! benchmark's opening inventory. That is a much weaker statement than "the
//! curves are equivalent", so this file deliberately tries to break it, along
//! the axes that a spot check cannot reach:
//!
//! 1. long random swap sequences, with state carried forward
//! 2. state (`sqrtPriceX96`, `tick`, `liquidity`) updated and re-compared after
//!    every swap
//! 3. round trips in both directions
//! 4. one-wei and reserve-scale inputs
//! 5. prices near `MIN_TICK` and `MAX_TICK`
//! 6. exact-input and exact-output
//!
//! Two questions are kept apart on purpose:
//!
//! * **Formula equivalence** — quote both curves from the *same* state. Any
//!   difference here is a genuine difference between the two formulas.
//! * **Trajectory drift** — let each curve carry its own state forward. A
//!   difference here can also come from the two state representations rounding
//!   differently, and is reported as a measurement rather than asserted to zero.
//!
//! Nothing here is allowed to "fix" a difference by simplifying a formula. Where
//! a divergence exists it is measured and reported.

use prop_amm_research::u256::U256;
use prop_amm_research::univ2;
use prop_amm_research::univ3::{self, pool, tick_math, PoolConfig, PoolState, I256};

const WAD: u128 = 1_000_000_000_000_000_000;
const Q96_SHIFT: u32 = 96;

/// Deterministic full-width sampler, reproducible without an RNG dependency.
///
/// SplitMix64 rather than a truncated LCG: an earlier version returned
/// `state >> 11`, which threw away 11 bits, and then sampled amounts with
/// `span as u64` — silently truncating a `200 * 1e18` ceiling to
/// `15532559262904483840`, roughly a thirteenth of the intended range. Both
/// mistakes shrank the region the sequence explored, so the amounts here are
/// drawn at full `u128` width and the tests assert what they actually covered.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn next_u128(&mut self) -> u128 {
        ((self.next_u64() as u128) << 64) | self.next_u64() as u128
    }

    fn bool(&mut self) -> bool {
        self.next_u64() & 1 == 0
    }

    /// Log-uniform in `[1, max]`: pick a bit width first, then a value of that
    /// width. Without this, a uniform draw over a 1e20 ceiling would put
    /// essentially every sample in the top decade and never exercise dust.
    fn log_uniform(&mut self, max: u128) -> u128 {
        if max <= 1 {
            return 1;
        }
        let width = 128 - max.leading_zeros();
        let bits = 1 + (self.next_u64() as u32) % width;
        let value = 1 + self.next_u128() % (1u128 << bits);
        value.min(max)
    }
}

/// Which decade of the reserve a trade fell in, for coverage accounting.
#[derive(Default, Debug, Clone, Copy)]
struct SizeCoverage {
    dust: usize,   // < 0.01% of the reserve
    small: usize,  // 0.01% .. 1%
    medium: usize, // 1% .. 10%
    large: usize,  // >= 10%
    max_seen: u128,
    min_seen: u128,
}

impl SizeCoverage {
    fn record(&mut self, amount: u128, reserve: u128) {
        if self.min_seen == 0 || amount < self.min_seen {
            self.min_seen = amount;
        }
        if amount > self.max_seen {
            self.max_seen = amount;
        }
        // Compare in permyriad to avoid a float and to keep the boundaries exact.
        let bps = if reserve == 0 {
            0
        } else {
            amount.saturating_mul(10_000) / reserve
        };
        match bps {
            0 => self.dust += 1,
            1..=99 => self.small += 1,
            100..=999 => self.medium += 1,
            _ => self.large += 1,
        }
    }

    fn assert_broad(&self, label: &str) {
        assert!(
            self.dust > 0,
            "{label}: no sub-basis-point trades were executed, so the sampler is not \
             reaching the dust regime (min seen {})",
            self.min_seen
        );
        assert!(self.small > 0, "{label}: no 0.01%-1% trades");
        assert!(self.medium > 0, "{label}: no 1%-10% trades");
        assert!(
            self.large > 0,
            "{label}: no trades at or above 10% of the reserve; the earlier truncated \
             sampler failed exactly here (max seen {})",
            self.max_seen
        );
    }
}

fn q96() -> U256 {
    U256::ONE.checked_shl(Q96_SHIFT).unwrap()
}

/// The virtual reserves a full-range position implies, i.e. the `(x, y)` a
/// constant-product pool would hold at the same price with the same liquidity:
/// `x = L * 2^96 / sqrtP`, `y = L * sqrtP / 2^96`.
///
/// Both divisions floor. At the opening state they are exact — `sqrtP = 10·2^96`
/// divides both — but at a general state reached after swapping they are not,
/// and the derived pair is then up to a wei away from the state it came from.
/// That derivation error is *not* a difference between the two curves, so
/// [`virtual_reserves_exact`] reports whether it is present.
fn virtual_reserves(state: &PoolState) -> (U256, U256) {
    let (x, y, _) = virtual_reserves_exact(state);
    (x, y)
}

/// As [`virtual_reserves`], plus whether both divisions were exact.
fn virtual_reserves_exact(state: &PoolState) -> (U256, U256, bool) {
    let liquidity = U256::from_u128(state.liquidity);
    let (x, x_remainder) = liquidity
        .checked_mul(q96())
        .unwrap()
        .checked_div_rem(state.sqrt_price_x96)
        .unwrap();
    let (y, y_remainder) = liquidity
        .checked_mul(state.sqrt_price_x96)
        .unwrap()
        .checked_div_rem(q96())
        .unwrap();
    (x, y, x_remainder.is_zero() && y_remainder.is_zero())
}

fn full_range() -> (PoolConfig, PoolState) {
    univ3::full_range_pool(univ3::FULL_RANGE_LIQUIDITY)
}

fn swap_v3(
    config: &PoolConfig,
    state: PoolState,
    zero_for_one: bool,
    amount: I256,
) -> Option<pool::SwapOutcome> {
    pool::swap(
        config,
        state,
        zero_for_one,
        amount,
        pool::no_limit(zero_for_one),
    )
}

fn abs_diff(a: U256, b: U256) -> U256 {
    if a >= b {
        a.checked_sub(b).unwrap()
    } else {
        b.checked_sub(a).unwrap()
    }
}

#[test]
fn opening_state_quotes_are_identical_in_both_directions() {
    let (config, state) = full_range();
    let (rx, ry) = virtual_reserves(&state);
    assert_eq!(rx.to_string(), "100000000000000000000");
    assert_eq!(ry.to_string(), "10000000000000000000000");

    let mut checked = 0usize;
    for exponent in 0..21u32 {
        let amount = U256::ONE.checked_shl(exponent * 3).unwrap();

        // token0 in
        let v3 = swap_v3(&config, state, true, I256::to_int256(amount).unwrap()).unwrap();
        let v2 = univ2::get_amount_out(amount, rx, ry).unwrap();
        assert_eq!(
            v3.amount1.magnitude(),
            v2,
            "token0 in, amount {amount}: v3 {} vs v2 {v2}",
            v3.amount1.to_string_signed()
        );

        // token1 in
        let v3 = swap_v3(&config, state, false, I256::to_int256(amount).unwrap()).unwrap();
        let v2 = univ2::get_amount_out(amount, ry, rx).unwrap();
        assert_eq!(
            v3.amount0.magnitude(),
            v2,
            "token1 in, amount {amount}: v3 {} vs v2 {v2}",
            v3.amount0.to_string_signed()
        );
        checked += 2;
    }
    assert!(checked >= 40);
}

#[test]
fn formula_equivalence_survives_long_random_sequences() {
    // Quote both curves from the SAME state at every step.
    //
    // Two buckets are kept apart, because conflating them would let a
    // derivation artefact masquerade as a formula difference:
    //
    // * states where `L·2^96 / sqrtP` and `L·sqrtP / 2^96` are both exact — the
    //   V2 reserves then *are* the V3 state, and the quotes must match to the
    //   wei;
    // * states where they are not — the derived reserves are themselves rounded,
    //   so a small difference is expected and is measured rather than asserted
    //   away.
    let mut worst = U256::ZERO;
    let mut worst_at = String::new();
    let mut exact_states = 0usize;
    let mut inexact_states = 0usize;
    let mut executed = 0usize;
    let mut coverage0 = SizeCoverage::default();
    let mut coverage1 = SizeCoverage::default();

    for seed in [1u64, 7, 42, 1_337, 0xC0FFEE, 0xBEEF, 0xDEAD_BEEF, 2_026] {
        let (config, mut state) = full_range();
        let mut rng = Rng(seed);

        for step in 0..400 {
            let (rx, ry, derivation_is_exact) = virtual_reserves_exact(&state);
            let zero_for_one = rng.bool();
            let (reserve_in, reserve_out) = if zero_for_one { (rx, ry) } else { (ry, rx) };
            let Some(reserve_in_u128) = reserve_in.as_u128() else {
                break;
            };

            // Log-uniform up to a quarter of the input-side reserve, at full
            // u128 width: dust and reserve-scale trades both get sampled.
            let amount_u128 = rng.log_uniform(reserve_in_u128 / 4);
            let amount = U256::from_u128(amount_u128);
            if zero_for_one {
                coverage0.record(amount_u128, reserve_in_u128);
            } else {
                coverage1.record(amount_u128, reserve_in_u128);
            }

            let Some(outcome) = swap_v3(
                &config,
                state,
                zero_for_one,
                I256::to_int256(amount).unwrap(),
            ) else {
                continue;
            };
            let Some(v2) = univ2::get_amount_out(amount, reserve_in, reserve_out) else {
                continue;
            };
            let v3_out = if zero_for_one {
                outcome.amount1.magnitude()
            } else {
                outcome.amount0.magnitude()
            };

            let difference = abs_diff(v3_out, v2);
            if derivation_is_exact {
                exact_states += 1;
                assert_eq!(
                    difference,
                    U256::ZERO,
                    "at an exactly representable state the two curves must agree to the wei: \
                     seed {seed} step {step}, zeroForOne {zero_for_one}, amount {amount}, \
                     v3 {v3_out}, v2 {v2}"
                );
            } else {
                inexact_states += 1;
                if difference > worst {
                    worst = difference;
                    worst_at = format!(
                        "seed {seed} step {step}, zeroForOne {zero_for_one}, amount {amount}, \
                         v3 {v3_out}, v2 {v2}"
                    );
                }
                assert!(
                    v3_out <= v2,
                    "v3 paid out more than the constant product at seed {seed} step {step}: \
                     v3 {v3_out} vs v2 {v2}"
                );
            }

            state = outcome.state;
            executed += 1;
        }
    }

    assert!(
        executed > 2_000,
        "expected most swaps to execute, got {executed}"
    );
    assert!(
        exact_states > 0,
        "the opening states should be exactly representable"
    );
    assert!(
        inexact_states > 0,
        "the sequences should leave those states"
    );
    coverage0.assert_broad("token0 in");
    coverage1.assert_broad("token1 in");

    // A bound on the derivation error, not on a formula difference. Deliberately
    // far above the observed value so a genuine divergence would still trip it.
    assert!(
        worst <= U256::from_u64(1_000_000),
        "difference at inexactly-derived states reached {worst} wei ({worst_at}), \
         which is too large to be the derivation rounding this bucket allows for"
    );
    println!(
        "same-state comparison: {executed} swaps over 8 seeds, {exact_states} exactly \
         representable (all agreed to the wei), {inexact_states} not (max difference {worst} wei)"
    );
    println!("  worst case: {worst_at}");
    println!(
        "  token0-in sizes: dust {} small {} medium {} large {} (min {} max {})",
        coverage0.dust,
        coverage0.small,
        coverage0.medium,
        coverage0.large,
        coverage0.min_seen,
        coverage0.max_seen
    );
    println!(
        "  token1-in sizes: dust {} small {} medium {} large {} (min {} max {})",
        coverage1.dust,
        coverage1.small,
        coverage1.medium,
        coverage1.large,
        coverage1.min_seen,
        coverage1.max_seen
    );
}

#[test]
fn independent_trajectories_are_measured_not_assumed() {
    // Now let each curve carry its own state. Divergence here can come from the
    // two state representations, not only from the formulas.
    let mut worst = U256::ZERO;
    let mut worst_at = String::new();
    let mut executed = 0usize;
    let mut coverage = SizeCoverage::default();

    for seed in [3u64, 11, 99, 0xF00D, 0x5EED] {
        let (config, mut v3_state) = full_range();
        let (mut rx, mut ry) = virtual_reserves(&v3_state);
        let mut rng = Rng(seed);

        for step in 0..400 {
            let zero_for_one = rng.bool();
            let (reserve_in, _) = if zero_for_one { (rx, ry) } else { (ry, rx) };
            let Some(reserve_in_u128) = reserve_in.as_u128() else {
                break;
            };
            let amount_u128 = rng.log_uniform(reserve_in_u128 / 4);
            let amount = U256::from_u128(amount_u128);
            coverage.record(amount_u128, reserve_in_u128);

            let Some(outcome) = swap_v3(
                &config,
                v3_state,
                zero_for_one,
                I256::to_int256(amount).unwrap(),
            ) else {
                continue;
            };
            let (v2_in, v2_out_reserve) = if zero_for_one { (rx, ry) } else { (ry, rx) };
            let Some(v2_out) = univ2::get_amount_out(amount, v2_in, v2_out_reserve) else {
                continue;
            };
            let v3_out = if zero_for_one {
                outcome.amount1.magnitude()
            } else {
                outcome.amount0.magnitude()
            };

            let difference = abs_diff(v3_out, v2_out);
            if difference > worst {
                worst = difference;
                worst_at = format!("seed {seed} step {step}, amount {amount}");
            }

            // Advance both ledgers by their own results.
            v3_state = outcome.state;
            if zero_for_one {
                rx = rx.checked_add(amount).unwrap();
                ry = ry.checked_sub(v2_out).unwrap();
            } else {
                ry = ry.checked_add(amount).unwrap();
                rx = rx.checked_sub(v2_out).unwrap();
            }
            executed += 1;
        }
    }

    assert!(
        executed > 1_200,
        "expected most swaps to execute, got {executed}"
    );
    coverage.assert_broad("trajectory");
    assert!(
        worst <= U256::from_u64(1_000_000),
        "trajectory drift reached {worst} wei ({worst_at}), which is far beyond \
         the wei-scale rounding this test exists to bound"
    );
    println!("trajectory drift: {executed} swaps over 5 seeds, max {worst} wei ({worst_at})");
    println!(
        "  sizes: dust {} small {} medium {} large {} (min {} max {})",
        coverage.dust,
        coverage.small,
        coverage.medium,
        coverage.large,
        coverage.min_seen,
        coverage.max_seen
    );
}

#[test]
fn round_trips_never_create_value_in_either_curve() {
    let (config, state) = full_range();
    let (rx, ry) = virtual_reserves(&state);

    for units in [1u128, 10, 1_000, WAD / 1_000, WAD, 5 * WAD] {
        let amount = U256::from_u128(units);

        // V3: sell token0, then sell the proceeds back.
        let first = swap_v3(&config, state, true, I256::to_int256(amount).unwrap()).unwrap();
        let proceeds = first.amount1.magnitude();
        if proceeds.is_zero() {
            continue;
        }
        let back = swap_v3(
            &config,
            first.state,
            false,
            I256::to_int256(proceeds).unwrap(),
        )
        .unwrap();
        assert!(
            back.amount0.magnitude() <= amount,
            "v3 round trip returned more than it took for {units}"
        );

        // V2: the same round trip on the constant product.
        let out = univ2::get_amount_out(amount, rx, ry).unwrap();
        let rx2 = rx.checked_add(amount).unwrap();
        let ry2 = ry.checked_sub(out).unwrap();
        let back2 = univ2::get_amount_out(out, ry2, rx2).unwrap();
        assert!(back2 <= amount, "v2 round trip returned more than it took");
    }
}

#[test]
fn extreme_inputs_are_handled_at_both_ends() {
    let (config, state) = full_range();
    let (rx, ry) = virtual_reserves(&state);

    // One wei in each direction.
    let one = I256::from_i128(1);
    let a = swap_v3(&config, state, true, one).unwrap();
    let b = swap_v3(&config, state, false, one).unwrap();
    assert_eq!(
        a.amount1.magnitude(),
        univ2::get_amount_out(U256::ONE, rx, ry).unwrap()
    );
    assert_eq!(
        b.amount0.magnitude(),
        univ2::get_amount_out(U256::ONE, ry, rx).unwrap()
    );

    // An input far larger than the reserves. Both curves must stay solvent:
    // V2 by construction, V3 because the price is bounded by MIN/MAX_SQRT_RATIO.
    let huge = U256::from_u128(1_000_000 * WAD);
    let v3 = swap_v3(&config, state, true, I256::to_int256(huge).unwrap()).unwrap();
    assert!(
        v3.amount1.magnitude() < ry,
        "v3 paid out {} of a {ry} reserve",
        v3.amount1.magnitude()
    );
    let v2 = univ2::get_amount_out(huge, rx, ry).unwrap();
    assert!(v2 < ry);
    // Both approach, but never reach, the full reserve.
    assert!(abs_diff(v3.amount1.magnitude(), v2) <= U256::from_u64(1_000_000));
}

#[test]
fn prices_near_the_tick_domain_bounds_still_quote() {
    // Start close to MIN_TICK and to MAX_TICK and confirm the port neither
    // panics nor silently reverts.
    for tick in [
        tick_math::MIN_TICK + 10,
        tick_math::MIN_TICK + 1_000,
        tick_math::MAX_TICK - 1_000,
        tick_math::MAX_TICK - 10,
    ] {
        let sqrt_price = tick_math::get_sqrt_ratio_at_tick(tick).unwrap();
        let (config, _) = full_range();
        let state = PoolState {
            sqrt_price_x96: sqrt_price,
            tick,
            liquidity: univ3::FULL_RANGE_LIQUIDITY,
        };
        let amount = I256::from_i128(WAD as i128);

        // Whichever direction still has room must produce a quote.
        let down = swap_v3(&config, state, true, amount);
        let up = swap_v3(&config, state, false, amount);
        assert!(
            down.is_some() || up.is_some(),
            "no direction quoted at tick {tick}"
        );
        for outcome in [down, up].into_iter().flatten() {
            assert!(
                outcome.state.sqrt_price_x96 >= tick_math::MIN_SQRT_RATIO
                    && outcome.state.sqrt_price_x96 < tick_math::MAX_SQRT_RATIO,
                "price left the domain at tick {tick}"
            );
        }
    }
}

#[test]
fn exact_output_matches_between_the_curves() {
    let (config, state) = full_range();
    let (rx, ry) = virtual_reserves(&state);

    for units in [1u128, 1_000, WAD / 100, WAD, 50 * WAD] {
        let wanted = U256::from_u128(units);

        // V3: ask for exactly `wanted` token1 out, paying token0.
        let requested = I256::to_int256(wanted).unwrap().wrapping_neg();
        let v3 = swap_v3(&config, state, true, requested).unwrap();
        assert_eq!(
            v3.amount1.magnitude(),
            wanted,
            "v3 must deliver the exact output for {units}"
        );
        let v3_in = v3.amount0.magnitude();

        // V2: the exact-output counterpart, which rounds up by one wei.
        let v2_in = univ2::get_amount_in(wanted, rx, ry).unwrap();

        // V3 rounds the input up too, so the two must agree within the one wei
        // that getAmountIn adds unconditionally.
        assert!(
            abs_diff(v3_in, v2_in) <= U256::from_u64(2),
            "exact-output input differed: v3 {v3_in} vs v2 {v2_in} for {units}"
        );
    }
}

#[test]
fn exact_input_and_exact_output_are_consistent_within_v3() {
    let (config, state) = full_range();
    for units in [1_000u128, WAD / 100, WAD, 10 * WAD] {
        let amount = U256::from_u128(units);
        let forward = swap_v3(&config, state, true, I256::to_int256(amount).unwrap()).unwrap();
        let produced = forward.amount1.magnitude();
        if produced.is_zero() {
            continue;
        }
        // Asking for exactly what the exact-input swap produced must cost no
        // more than that swap paid.
        let reverse = swap_v3(
            &config,
            state,
            true,
            I256::to_int256(produced).unwrap().wrapping_neg(),
        )
        .unwrap();
        assert_eq!(reverse.amount1.magnitude(), produced);
        assert!(
            reverse.amount0.magnitude() <= amount,
            "exact-output cost {} exceeded the exact-input payment {amount}",
            reverse.amount0.magnitude()
        );
    }
}
