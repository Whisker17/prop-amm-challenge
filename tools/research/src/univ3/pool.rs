//! Verbatim Rust port of the `UniswapV3Pool.swap` loop.
//!
//! Upstream: Uniswap/v3-core `contracts/UniswapV3Pool.sol`
//! Commit:   e3589b192d0be27e100cd0daaf6c97204fdb1899 (tag `v1.0.0`)
//! sha256:   d515775b7f3ffe921dd70aca86b8bad16280fa4c122425d82b4dbea4dc564a7a
//! Licence:  BUSL-1.1 per the file's own SPDX header
//!
//! ## What is ported
//!
//! The pricing state machine: the `while` loop, `nextInitializedTickWithinOneWord`
//! stepping, the `MIN_TICK`/`MAX_TICK` clamp, `computeSwapStep`, the tick
//! transition (`liquidityNet`, negated when moving down), the
//! `tick = tickNext - 1` asymmetry, the `getTickAtSqrtRatio` recompute, and the
//! final `(amount0, amount1)` assembly. Solidity 0.7.6 semantics are preserved
//! throughout: bare `+`/`-` wrap, while `LowGasSafeMath`'s `.add()`/`.sub()` and
//! `SafeCast`'s `.toInt256()` revert.
//!
//! ## What is deliberately NOT ported, and why it cannot change a quote
//!
//! * **Oracle observations** (`observations.observeSingle` / `.write`). They
//!   record cumulative tick and seconds-per-liquidity for external consumers and
//!   feed nothing back into pricing.
//! * **Protocol fee**. `feeProtocol` is zero for a pool whose owner never calls
//!   `setFeeProtocol`, and the benchmark never sets it. With `feeProtocol == 0`
//!   upstream skips the block entirely.
//! * **Token transfers, the swap callback and the `IIA` balance check.** They are
//!   settlement, not pricing.
//! * **Fee growth *outside* per tick** (`Tick.cross`'s bookkeeping). It affects
//!   position fee accounting, never the swap amounts. Global fee growth *is*
//!   tracked here because it is cheap and lets the golden vectors check it.
//!
//! Everything omitted above is settlement or accounting. The golden vectors come
//! from executing the real pool, so any error in that judgement shows up as a
//! failing test rather than a silent divergence.

use super::signed::I256;
use super::swap_math::{add_delta, compute_swap_step};
use super::tick_list::TickList;
use super::tick_math::{self, MAX_SQRT_RATIO, MAX_TICK, MIN_SQRT_RATIO, MIN_TICK};
use crate::u256::U256;
use crate::univ3::mul_div;

/// `FixedPoint128.Q128`
const Q128: U256 = U256::from_limbs([0, 0, 1, 0]);

/// A swap that needs more loop iterations than this is treated as a revert.
///
/// The whole tick domain is `2 * 887272` ticks, i.e. under 6 933 words at
/// `tickSpacing = 1`, so this budget cannot be reached by any swap that stays
/// inside the domain and makes progress. It exists so that a bug cannot hang the
/// simulation; when it triggers the caller records a revert rather than
/// silently returning a truncated quote.
pub const MAX_LOOP_ITERATIONS: u32 = 8_192;

/// Static pool configuration: the fee tier and the position set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolConfig {
    pub tick_spacing: i32,
    pub fee_pips: u32,
    pub ticks: TickList,
}

/// The mutable pricing state (`slot0.sqrtPriceX96`, `slot0.tick`, `liquidity`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolState {
    pub sqrt_price_x96: U256,
    pub tick: i32,
    pub liquidity: u128,
}

/// What a swap produced. `amount0`/`amount1` are signed exactly as the pool
/// returns them: positive is paid to the pool, negative is paid out by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwapOutcome {
    pub amount0: I256,
    pub amount1: I256,
    pub state: PoolState,
    pub crossed_ticks: u32,
    pub fee_growth_global_x128: U256,
    pub iterations: u32,
}

/// The "no price limit" values a caller uses when it wants the swap to run to
/// completion, matching what the periphery passes.
pub fn no_limit(zero_for_one: bool) -> U256 {
    if zero_for_one {
        MIN_SQRT_RATIO.wrapping_add(U256::ONE)
    } else {
        MAX_SQRT_RATIO.wrapping_sub(U256::ONE)
    }
}

/// `UniswapV3Pool.swap(...)`, pricing only.
///
/// Returns `None` where upstream reverts: `'AS'` (zero amount), `'SPL'` (bad
/// price limit), any arithmetic revert inside the maths, or the loop budget.
pub fn swap(
    config: &PoolConfig,
    state: PoolState,
    zero_for_one: bool,
    amount_specified: I256,
    sqrt_price_limit_x96: U256,
) -> Option<SwapOutcome> {
    if amount_specified.is_zero() {
        return None; // require(amountSpecified != 0, 'AS')
    }
    let limit_ok = if zero_for_one {
        sqrt_price_limit_x96 < state.sqrt_price_x96 && sqrt_price_limit_x96 > MIN_SQRT_RATIO
    } else {
        sqrt_price_limit_x96 > state.sqrt_price_x96 && sqrt_price_limit_x96 < MAX_SQRT_RATIO
    };
    if !limit_ok {
        return None; // require(..., 'SPL')
    }

    let exact_input = amount_specified > I256::ZERO;

    let mut amount_specified_remaining = amount_specified;
    let mut amount_calculated = I256::ZERO;
    let mut sqrt_price_x96 = state.sqrt_price_x96;
    let mut tick = state.tick;
    let mut liquidity = state.liquidity;
    let mut fee_growth_global_x128 = U256::ZERO;
    let mut crossed_ticks = 0u32;
    let mut iterations = 0u32;

    while !amount_specified_remaining.is_zero() && sqrt_price_x96 != sqrt_price_limit_x96 {
        iterations += 1;
        if iterations > MAX_LOOP_ITERATIONS {
            return None;
        }

        // A run of steps with no liquidity and no initialized tick left ahead
        // can only walk the price to the limit while contributing nothing to
        // either amount, so it is collapsed. This is an optimisation over the
        // iteration count only: every skipped step has `amountIn == amountOut ==
        // 0`, because both deltas are proportional to `liquidity == 0`.
        //
        // The final tick still has to be whatever the walk would have left
        // behind. Normally the limit is not a tick price, so the last step takes
        // the `getTickAtSqrtRatio` branch. If the limit *is* exactly a tick
        // price, the last step instead takes the `sqrtPrice == sqrtPriceNext`
        // branch and applies the `tickNext - 1` asymmetry, which is reproduced
        // here rather than being quietly off by one.
        if liquidity == 0 && !config.ticks.has_initialized_beyond(tick, zero_for_one) {
            sqrt_price_x96 = sqrt_price_limit_x96;
            let limit_tick = tick_math::get_tick_at_sqrt_ratio(sqrt_price_x96)?;
            let limit_is_a_tick_price =
                tick_math::get_sqrt_ratio_at_tick(limit_tick)? == sqrt_price_x96;
            tick = if limit_is_a_tick_price && zero_for_one {
                limit_tick - 1
            } else {
                limit_tick
            };
            break;
        }

        let sqrt_price_start_x96 = sqrt_price_x96;

        let (mut tick_next, initialized) = config.ticks.next_initialized_tick_within_one_word(
            tick,
            config.tick_spacing,
            zero_for_one,
        );

        // ensure that we do not overshoot the min/max tick
        //
        // Kept as upstream's if/else-if rather than `clamp`: this file is a
        // branch-for-branch port, and `clamp` would also change the behaviour if
        // MIN_TICK > MAX_TICK ever stopped holding (panic instead of MIN_TICK).
        #[allow(clippy::manual_clamp)]
        if tick_next < MIN_TICK {
            tick_next = MIN_TICK;
        } else if tick_next > MAX_TICK {
            tick_next = MAX_TICK;
        }

        let sqrt_price_next_x96 = tick_math::get_sqrt_ratio_at_tick(tick_next)?;

        let target = if (zero_for_one && sqrt_price_next_x96 < sqrt_price_limit_x96)
            || (!zero_for_one && sqrt_price_next_x96 > sqrt_price_limit_x96)
        {
            sqrt_price_limit_x96
        } else {
            sqrt_price_next_x96
        };

        let step = compute_swap_step(
            sqrt_price_x96,
            target,
            liquidity,
            amount_specified_remaining,
            config.fee_pips,
        )?;
        sqrt_price_x96 = step.sqrt_ratio_next_x96;

        // `-=` / `+=` are bare operators upstream (wrapping); `.sub()` / `.add()`
        // are LowGasSafeMath (checked). Both are reproduced as written.
        let step_in_plus_fee = step.amount_in.wrapping_add(step.fee_amount);
        if exact_input {
            amount_specified_remaining =
                amount_specified_remaining.wrapping_sub(I256::to_int256(step_in_plus_fee)?);
            amount_calculated = amount_calculated.checked_sub(I256::to_int256(step.amount_out)?)?;
        } else {
            amount_specified_remaining =
                amount_specified_remaining.wrapping_add(I256::to_int256(step.amount_out)?);
            amount_calculated =
                amount_calculated.checked_add(I256::to_int256(step_in_plus_fee)?)?;
        }

        // protocol fee is off in this benchmark, so upstream's block is skipped

        if liquidity > 0 {
            fee_growth_global_x128 = fee_growth_global_x128.wrapping_add(mul_div(
                step.fee_amount,
                Q128,
                U256::from_u128(liquidity),
            )?);
        }

        if sqrt_price_x96 == sqrt_price_next_x96 {
            if initialized {
                let mut liquidity_net = config.ticks.liquidity_net(tick_next)?;
                // if we're moving leftward, we interpret liquidityNet as the opposite sign
                if zero_for_one {
                    liquidity_net = liquidity_net.wrapping_neg();
                }
                liquidity = add_delta(liquidity, liquidity_net)?;
                crossed_ticks += 1;
            }
            tick = if zero_for_one {
                tick_next - 1
            } else {
                tick_next
            };
        } else if sqrt_price_x96 != sqrt_price_start_x96 {
            tick = tick_math::get_tick_at_sqrt_ratio(sqrt_price_x96)?;
        }
    }

    let consumed = amount_specified.wrapping_sub(amount_specified_remaining);
    let (amount0, amount1) = if zero_for_one == exact_input {
        (consumed, amount_calculated)
    } else {
        (amount_calculated, consumed)
    };

    Some(SwapOutcome {
        amount0,
        amount1,
        state: PoolState {
            sqrt_price_x96,
            tick,
            liquidity,
        },
        crossed_ticks,
        fee_growth_global_x128,
        iterations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sqrt_price_100() -> U256 {
        U256::from_u128(792_281_625_142_643_375_935_439_503_360)
    }

    const L: u128 = 1_000_000_000_000_000_000_000;

    fn full_range() -> (PoolConfig, PoolState) {
        (
            PoolConfig {
                tick_spacing: 1,
                fee_pips: 0,
                ticks: TickList::new([(MIN_TICK, L as i128), (MAX_TICK, -(L as i128))]),
            },
            PoolState {
                sqrt_price_x96: sqrt_price_100(),
                tick: 46_054,
                liquidity: L,
            },
        )
    }

    #[test]
    fn zero_amount_and_bad_limits_revert() {
        let (config, state) = full_range();
        assert_eq!(
            swap(&config, state, true, I256::ZERO, no_limit(true)),
            None,
            "'AS'"
        );
        // limit on the wrong side of the current price
        assert_eq!(
            swap(&config, state, true, I256::from_i128(1), no_limit(false)),
            None,
            "'SPL'"
        );
        assert_eq!(
            swap(&config, state, false, I256::from_i128(1), no_limit(true)),
            None,
            "'SPL'"
        );
    }

    #[test]
    fn selling_token0_lowers_the_price_and_pays_out_token1() {
        let (config, state) = full_range();
        let one = I256::from_i128(1_000_000_000_000_000_000);
        let outcome = swap(&config, state, true, one, no_limit(true)).unwrap();
        assert_eq!(outcome.amount0, one, "the whole input is consumed");
        assert!(outcome.amount1.is_negative(), "token1 is paid out");
        assert!(outcome.state.sqrt_price_x96 < sqrt_price_100());
        assert_eq!(outcome.crossed_ticks, 0, "full range has no interior ticks");
        assert_eq!(outcome.state.liquidity, L, "liquidity is unchanged");
    }

    #[test]
    fn a_one_wei_swap_is_priced_without_panicking() {
        let (config, state) = full_range();
        let outcome = swap(&config, state, true, I256::from_i128(1), no_limit(true)).unwrap();
        // One wei of token0 buys less than one wei of token1, so the output
        // rounds to zero and the price barely moves.
        assert!(outcome.amount1.magnitude() <= U256::from_u64(100));
    }

    #[test]
    fn exact_output_returns_the_requested_amount() {
        let (config, state) = full_range();
        // Ask for exactly 100 token1 out, paying token0.
        let want = I256::from_i128(-100_000_000_000_000_000_000);
        let outcome = swap(&config, state, true, want, no_limit(true)).unwrap();
        assert_eq!(outcome.amount1, want, "exact output is delivered exactly");
        assert!(outcome.amount0.is_positive_or_zero());
        assert!(outcome.amount0.magnitude() > U256::ZERO);
    }

    #[test]
    fn a_concentrated_range_crosses_its_boundary_and_drops_liquidity() {
        let lower = 46_054 - 200;
        let upper = 46_054 + 200;
        let config = PoolConfig {
            tick_spacing: 1,
            fee_pips: 0,
            ticks: TickList::new([(lower, L as i128), (upper, -(L as i128))]),
        };
        let state = PoolState {
            sqrt_price_x96: sqrt_price_100(),
            tick: 46_054,
            liquidity: L,
        };
        // A large sell must walk out of the bottom of the range.
        let big = I256::from_i128(50_000_000_000_000_000_000);
        let outcome = swap(&config, state, true, big, no_limit(true)).unwrap();
        assert_eq!(outcome.crossed_ticks, 1, "exactly one boundary is crossed");
        assert_eq!(outcome.state.liquidity, 0, "the position is left behind");
        assert!(outcome.state.tick < lower);
        // Not all of the input can be used once liquidity is gone.
        assert!(outcome.amount0 < big);
    }

    #[test]
    fn a_price_limit_stops_the_swap_early() {
        let (config, state) = full_range();
        let limit = tick_math::get_sqrt_ratio_at_tick(46_000).unwrap();
        let big = I256::from_i128(100_000_000_000_000_000_000);
        let outcome = swap(&config, state, true, big, limit).unwrap();
        assert_eq!(outcome.state.sqrt_price_x96, limit);
        assert!(
            outcome.amount0 < big,
            "input is left unconsumed at the limit"
        );
    }

    #[test]
    fn multiple_positions_cross_multiple_ticks() {
        let config = PoolConfig {
            tick_spacing: 1,
            fee_pips: 0,
            ticks: TickList::new([
                (46_054 - 100, L as i128),
                (46_054 + 100, -(L as i128)),
                (46_054 - 300, (L / 2) as i128),
                (46_054 + 300, -((L / 2) as i128)),
            ]),
        };
        let state = PoolState {
            sqrt_price_x96: sqrt_price_100(),
            tick: 46_054,
            liquidity: L + L / 2,
        };
        let big = I256::from_i128(80_000_000_000_000_000_000);
        let outcome = swap(&config, state, true, big, no_limit(true)).unwrap();
        assert!(
            outcome.crossed_ticks >= 2,
            "expected several tick crossings"
        );
        assert!(outcome.state.liquidity < L);
    }

    #[test]
    fn round_trip_never_creates_value() {
        let (config, state) = full_range();
        let one = I256::from_i128(1_000_000_000_000_000_000);
        let first = swap(&config, state, true, one, no_limit(true)).unwrap();
        let received = first.amount1.magnitude();
        let back = swap(
            &config,
            first.state,
            false,
            I256::to_int256(received).unwrap(),
            no_limit(false),
        )
        .unwrap();
        // Selling the proceeds straight back must not return more token0 than
        // was put in.
        assert!(back.amount0.magnitude() <= one.magnitude());
    }

    #[test]
    fn the_empty_walk_reproduces_the_tick_asymmetry_at_a_tick_aligned_limit() {
        // A pool whose price sits below its only position: no liquidity, and
        // nothing initialized further down. The walk to the limit trades
        // nothing, so the only thing to get right is the final tick.
        let lower = 46_000;
        let upper = 46_500;
        let config = PoolConfig {
            tick_spacing: 1,
            fee_pips: 0,
            ticks: TickList::new([(lower, L as i128), (upper, -(L as i128))]),
        };
        let start_tick = 45_000;
        let state = PoolState {
            sqrt_price_x96: tick_math::get_sqrt_ratio_at_tick(start_tick).unwrap(),
            tick: start_tick,
            liquidity: 0,
        };

        // Limit exactly on a tick price, moving down: upstream would leave
        // `tick = limitTick - 1`.
        let limit_tick = 44_000;
        let limit = tick_math::get_sqrt_ratio_at_tick(limit_tick).unwrap();
        let outcome = swap(&config, state, true, I256::from_i128(1_000_000), limit).unwrap();
        assert_eq!(outcome.state.sqrt_price_x96, limit);
        assert_eq!(outcome.state.tick, limit_tick - 1);
        assert!(outcome.amount1.is_zero(), "no liquidity means no output");
        assert!(outcome.amount0.is_zero(), "and no input is consumed");

        // A limit that is not a tick price leaves `getTickAtSqrtRatio(limit)`.
        let off_tick_limit = limit.checked_add(U256::ONE).unwrap();
        let outcome = swap(
            &config,
            state,
            true,
            I256::from_i128(1_000_000),
            off_tick_limit,
        )
        .unwrap();
        assert_eq!(
            outcome.state.tick,
            tick_math::get_tick_at_sqrt_ratio(off_tick_limit).unwrap()
        );
    }

    #[test]
    fn the_loop_budget_is_never_reached_by_an_ordinary_swap() {
        let (config, state) = full_range();
        for units in [
            1i128,
            1_000,
            1_000_000_000_000_000_000,
            10_000_000_000_000_000_000,
        ] {
            let outcome =
                swap(&config, state, true, I256::from_i128(units), no_limit(true)).unwrap();
            assert!(
                outcome.iterations < 100,
                "{units} took {} iterations",
                outcome.iterations
            );
        }
    }
}
