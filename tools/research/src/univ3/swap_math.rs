//! Verbatim Rust port of Uniswap V3 `SwapMath` and `LiquidityMath`.
//!
//! Upstream: Uniswap/v3-core `contracts/libraries/SwapMath.sol`
//! Commit:   e3589b192d0be27e100cd0daaf6c97204fdb1899 (tag `v1.0.0`)
//! sha256:   d6cb9a153be4ea9fb2377ef88641ef7979b5cee6933162f1b732d0289e26e1b6
//! and       `contracts/libraries/LiquidityMath.sol`
//! sha256:   84d20a16d5346f6ec4c12dff4df23dda5d46e52d33f18aaaaac2e9e36ce4a072
//!
//! `computeSwapStep` is the single-tick swap kernel. Its structure matters:
//! the first branch decides *where the price ends up*, and only then are the
//! final `amountIn` / `amountOut` recomputed — except in the two cases where the
//! already-computed value is reused (`max && exactIn` / `max && !exactIn`).
//! Recomputing those would change the rounding, so the reuse is preserved.

use super::signed::I256;
use super::sqrt_price_math::{
    get_amount0_delta, get_amount1_delta, get_next_sqrt_price_from_input,
    get_next_sqrt_price_from_output,
};
use crate::u256::U256;
use crate::univ3::{mul_div, mul_div_rounding_up};

/// One hundred per cent in hundredths of a bip, i.e. `1e6`.
pub const FEE_DENOMINATOR: u32 = 1_000_000;

/// The four return values of `computeSwapStep`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwapStep {
    pub sqrt_ratio_next_x96: U256,
    pub amount_in: U256,
    pub amount_out: U256,
    pub fee_amount: U256,
}

/// `computeSwapStep(sqrtRatioCurrentX96, sqrtRatioTargetX96, liquidity, amountRemaining, feePips)`
pub fn compute_swap_step(
    sqrt_ratio_current_x96: U256,
    sqrt_ratio_target_x96: U256,
    liquidity: u128,
    amount_remaining: I256,
    fee_pips: u32,
) -> Option<SwapStep> {
    let zero_for_one = sqrt_ratio_current_x96 >= sqrt_ratio_target_x96;
    let exact_in = amount_remaining.is_positive_or_zero();

    let mut amount_in = U256::ZERO;
    let mut amount_out = U256::ZERO;
    let sqrt_ratio_next_x96;

    if exact_in {
        let amount_remaining_less_fee = mul_div(
            amount_remaining.to_raw(),
            U256::from_u64((FEE_DENOMINATOR - fee_pips) as u64),
            U256::from_u64(FEE_DENOMINATOR as u64),
        )?;
        amount_in = if zero_for_one {
            get_amount0_delta(
                sqrt_ratio_target_x96,
                sqrt_ratio_current_x96,
                liquidity,
                true,
            )?
        } else {
            get_amount1_delta(
                sqrt_ratio_current_x96,
                sqrt_ratio_target_x96,
                liquidity,
                true,
            )?
        };
        sqrt_ratio_next_x96 = if amount_remaining_less_fee >= amount_in {
            sqrt_ratio_target_x96
        } else {
            get_next_sqrt_price_from_input(
                sqrt_ratio_current_x96,
                liquidity,
                amount_remaining_less_fee,
                zero_for_one,
            )?
        };
    } else {
        amount_out = if zero_for_one {
            get_amount1_delta(
                sqrt_ratio_target_x96,
                sqrt_ratio_current_x96,
                liquidity,
                false,
            )?
        } else {
            get_amount0_delta(
                sqrt_ratio_current_x96,
                sqrt_ratio_target_x96,
                liquidity,
                false,
            )?
        };
        let requested = amount_remaining.magnitude(); // uint256(-amountRemaining)
        sqrt_ratio_next_x96 = if requested >= amount_out {
            sqrt_ratio_target_x96
        } else {
            get_next_sqrt_price_from_output(
                sqrt_ratio_current_x96,
                liquidity,
                requested,
                zero_for_one,
            )?
        };
    }

    let max = sqrt_ratio_target_x96 == sqrt_ratio_next_x96;

    // get the input/output amounts
    if zero_for_one {
        amount_in = if max && exact_in {
            amount_in
        } else {
            get_amount0_delta(sqrt_ratio_next_x96, sqrt_ratio_current_x96, liquidity, true)?
        };
        amount_out = if max && !exact_in {
            amount_out
        } else {
            get_amount1_delta(
                sqrt_ratio_next_x96,
                sqrt_ratio_current_x96,
                liquidity,
                false,
            )?
        };
    } else {
        amount_in = if max && exact_in {
            amount_in
        } else {
            get_amount1_delta(sqrt_ratio_current_x96, sqrt_ratio_next_x96, liquidity, true)?
        };
        amount_out = if max && !exact_in {
            amount_out
        } else {
            get_amount0_delta(
                sqrt_ratio_current_x96,
                sqrt_ratio_next_x96,
                liquidity,
                false,
            )?
        };
    }

    // cap the output amount to not exceed the remaining output amount
    if !exact_in && amount_out > amount_remaining.magnitude() {
        amount_out = amount_remaining.magnitude();
    }

    let fee_amount = if exact_in && sqrt_ratio_next_x96 != sqrt_ratio_target_x96 {
        // we didn't reach the target, so take the remainder of the maximum input as fee
        amount_remaining.to_raw().wrapping_sub(amount_in)
    } else {
        mul_div_rounding_up(
            amount_in,
            U256::from_u64(fee_pips as u64),
            U256::from_u64((FEE_DENOMINATOR - fee_pips) as u64),
        )?
    };

    Some(SwapStep {
        sqrt_ratio_next_x96,
        amount_in,
        amount_out,
        fee_amount,
    })
}

/// `LiquidityMath.addDelta(uint128 x, int128 y)`
///
/// ```solidity
/// if (y < 0) { require((z = x - uint128(-y)) < x, 'LS'); }
/// else       { require((z = x + uint128(y)) >= x, 'LA'); }
/// ```
pub fn add_delta(x: u128, y: i128) -> Option<u128> {
    if y < 0 {
        let z = x.wrapping_sub(y.unsigned_abs());
        if z < x {
            Some(z)
        } else {
            None // 'LS'
        }
    } else {
        let z = x.wrapping_add(y as u128);
        if z >= x {
            Some(z)
        } else {
            None // 'LA'
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::univ3::tick_math;

    fn sqrt_price_100() -> U256 {
        U256::from_u128(792_281_625_142_643_375_935_439_503_360)
    }

    const L: u128 = 1_000_000_000_000_000_000_000;

    #[test]
    fn add_delta_matches_the_require_conditions() {
        assert_eq!(add_delta(10, 5), Some(15));
        assert_eq!(add_delta(10, -5), Some(5));
        assert_eq!(add_delta(0, -1), None, "'LS' underflow");
        assert_eq!(add_delta(u128::MAX, 1), None, "'LA' overflow");
        // Adding zero must succeed (z >= x holds with equality).
        assert_eq!(add_delta(7, 0), Some(7));
        // Subtracting zero must FAIL upstream, because z < x is strict.
        assert_eq!(
            add_delta(7, -0),
            Some(7),
            "-0 is not negative in Rust, so this takes the add branch, as it does in Solidity"
        );
    }

    #[test]
    fn zero_fee_step_takes_no_fee() {
        let target = tick_math::get_sqrt_ratio_at_tick(46_000).unwrap();
        let step = compute_swap_step(
            sqrt_price_100(),
            target,
            L,
            I256::from_i128(1_000_000_000_000_000_000),
            0,
        )
        .unwrap();
        assert_eq!(step.fee_amount, U256::ZERO);
        assert!(step.amount_in > U256::ZERO);
        assert!(step.amount_out > U256::ZERO);
    }

    #[test]
    fn exact_input_that_cannot_reach_the_target_stops_short_of_it() {
        let target = tick_math::get_sqrt_ratio_at_tick(40_000).unwrap();
        let step = compute_swap_step(
            sqrt_price_100(),
            target,
            L,
            I256::from_i128(1_000_000_000_000_000), // 0.001 token0
            0,
        )
        .unwrap();
        assert_ne!(step.sqrt_ratio_next_x96, target);
        assert!(step.sqrt_ratio_next_x96 < sqrt_price_100());
        // The whole input is consumed when the target is not reached.
        assert_eq!(step.amount_in, U256::from_u128(1_000_000_000_000_000));
    }

    #[test]
    fn exact_input_that_overshoots_the_target_is_capped_at_it() {
        let target = tick_math::get_sqrt_ratio_at_tick(46_053).unwrap();
        let step = compute_swap_step(
            sqrt_price_100(),
            target,
            L,
            I256::from_i128(1_000_000_000_000_000_000_000), // far more than needed
            0,
        )
        .unwrap();
        assert_eq!(step.sqrt_ratio_next_x96, target);
    }

    #[test]
    fn exact_output_is_capped_at_the_requested_amount() {
        let target = tick_math::get_sqrt_ratio_at_tick(46_000).unwrap();
        let requested = I256::from_i128(-1_000_000_000_000_000_000); // want 1 token1 out
        let step = compute_swap_step(sqrt_price_100(), target, L, requested, 0).unwrap();
        assert!(step.amount_out <= requested.magnitude());
    }

    #[test]
    fn a_nonzero_fee_is_charged_on_the_input() {
        let target = tick_math::get_sqrt_ratio_at_tick(46_053).unwrap();
        let amount = I256::from_i128(1_000_000_000_000_000_000_000);
        let zero_fee = compute_swap_step(sqrt_price_100(), target, L, amount, 0).unwrap();
        let with_fee = compute_swap_step(sqrt_price_100(), target, L, amount, 3_000).unwrap();
        assert_eq!(zero_fee.fee_amount, U256::ZERO);
        assert!(with_fee.fee_amount > U256::ZERO);
        // Reaching the target costs the same input either way; the fee is extra.
        assert_eq!(with_fee.amount_in, zero_fee.amount_in);
    }

    #[test]
    fn zero_liquidity_moves_straight_to_the_target_without_trading() {
        let target = tick_math::get_sqrt_ratio_at_tick(46_000).unwrap();
        let step = compute_swap_step(
            sqrt_price_100(),
            target,
            0,
            I256::from_i128(1_000_000_000_000_000_000),
            0,
        )
        .unwrap();
        assert_eq!(step.sqrt_ratio_next_x96, target);
        assert_eq!(step.amount_in, U256::ZERO);
        assert_eq!(step.amount_out, U256::ZERO);
    }
}
