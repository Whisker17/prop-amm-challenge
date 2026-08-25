//! Verbatim Rust port of Uniswap V3 `SqrtPriceMath` (and the one-line
//! `UnsafeMath` helper it depends on).
//!
//! Upstream: Uniswap/v3-core `contracts/libraries/SqrtPriceMath.sol`
//! Commit:   e3589b192d0be27e100cd0daaf6c97204fdb1899 (tag `v1.0.0`)
//! sha256:   ddd62e3a94346248677f30f1ab009ef015e71e4b8696dcca890eeabc9dc6c149
//!
//! Two subtleties are preserved deliberately because they change results:
//!
//! * Upstream compiles at 0.7.6, so bare `*`, `+` and `-` **wrap**, while
//!   `LowGasSafeMath`'s `.add()` and `SafeCast`'s `.toUint160()` **revert**. The
//!   two are mixed within single expressions here, exactly as upstream mixes
//!   them — e.g. `getNextSqrtPriceFromAmount0RoundingUp` truncates with
//!   `uint160(...)` on the `add` path but reverts via `.toUint160()` on the
//!   `remove` path.
//! * The rounding direction of each helper is part of the protocol's safety
//!   argument, not an implementation detail.

use super::signed::{self, I256};
use crate::u256::U256;
use crate::univ3::mul_div;
use crate::univ3::mul_div_rounding_up;

/// `FixedPoint96.RESOLUTION`
pub const RESOLUTION: u32 = 96;
/// `FixedPoint96.Q96`
pub const Q96: U256 = U256::from_u128(1 << 96);

/// `UnsafeMath.divRoundingUp(x, y)` = `add(div(x, y), gt(mod(x, y), 0))`.
///
/// Upstream documents division by zero as unspecified; on the EVM `DIV` and
/// `MOD` by zero both yield zero, so this returns zero, and the `add` wraps.
pub fn div_rounding_up(x: U256, y: U256) -> U256 {
    match x.checked_div_rem(y) {
        Some((quotient, remainder)) => {
            if remainder.is_zero() {
                quotient
            } else {
                quotient.wrapping_add(U256::ONE)
            }
        }
        None => U256::ZERO,
    }
}

fn max_uint160() -> U256 {
    U256::ONE
        .checked_shl(160)
        .expect("2^160 fits")
        .wrapping_sub(U256::ONE)
}

/// `getNextSqrtPriceFromAmount0RoundingUp`
pub fn get_next_sqrt_price_from_amount0_rounding_up(
    sqrt_px96: U256,
    liquidity: u128,
    amount: U256,
    add: bool,
) -> Option<U256> {
    // we short circuit amount == 0 because the result is otherwise not
    // guaranteed to equal the input price
    if amount.is_zero() {
        return Some(sqrt_px96);
    }
    let numerator1 = U256::from_u128(liquidity).checked_shl(RESOLUTION)?;

    if add {
        // if ((product = amount * sqrtPX96) / amount == sqrtPX96)
        let product = amount.wrapping_mul(sqrt_px96);
        if product.checked_div(amount)? == sqrt_px96 {
            let denominator = numerator1.wrapping_add(product);
            if denominator >= numerator1 {
                // always fits in 160 bits (truncating cast upstream)
                return Some(super::bits::to_uint160_truncating(mul_div_rounding_up(
                    numerator1,
                    sqrt_px96,
                    denominator,
                )?));
            }
        }
        // uint160(UnsafeMath.divRoundingUp(numerator1, (numerator1 / sqrtPX96).add(amount)))
        let quotient = numerator1.checked_div(sqrt_px96)?;
        let denominator = quotient.checked_add(amount)?; // LowGasSafeMath.add reverts
        Some(super::bits::to_uint160_truncating(div_rounding_up(
            numerator1,
            denominator,
        )))
    } else {
        // require((product = amount * sqrtPX96) / amount == sqrtPX96 && numerator1 > product)
        let product = amount.wrapping_mul(sqrt_px96);
        if product.checked_div(amount)? != sqrt_px96 || numerator1 <= product {
            return None;
        }
        let denominator = numerator1.wrapping_sub(product);
        // .toUint160() reverts here rather than truncating
        signed::to_uint160(mul_div_rounding_up(numerator1, sqrt_px96, denominator)?)
    }
}

/// `getNextSqrtPriceFromAmount1RoundingDown`
pub fn get_next_sqrt_price_from_amount1_rounding_down(
    sqrt_px96: U256,
    liquidity: u128,
    amount: U256,
    add: bool,
) -> Option<U256> {
    let liquidity_u256 = U256::from_u128(liquidity);
    if add {
        let quotient = if amount <= max_uint160() {
            amount
                .checked_shl(RESOLUTION)?
                .checked_div(liquidity_u256)?
        } else {
            mul_div(amount, Q96, liquidity_u256)?
        };
        // uint256(sqrtPX96).add(quotient).toUint160()
        signed::to_uint160(sqrt_px96.checked_add(quotient)?)
    } else {
        let quotient = if amount <= max_uint160() {
            div_rounding_up(amount.checked_shl(RESOLUTION)?, liquidity_u256)
        } else {
            mul_div_rounding_up(amount, Q96, liquidity_u256)?
        };
        if sqrt_px96 <= quotient {
            return None; // require(sqrtPX96 > quotient)
        }
        // always fits 160 bits
        Some(super::bits::to_uint160_truncating(
            sqrt_px96.wrapping_sub(quotient),
        ))
    }
}

/// `getNextSqrtPriceFromInput`
pub fn get_next_sqrt_price_from_input(
    sqrt_px96: U256,
    liquidity: u128,
    amount_in: U256,
    zero_for_one: bool,
) -> Option<U256> {
    if sqrt_px96.is_zero() || liquidity == 0 {
        return None; // require(sqrtPX96 > 0); require(liquidity > 0);
    }
    if zero_for_one {
        get_next_sqrt_price_from_amount0_rounding_up(sqrt_px96, liquidity, amount_in, true)
    } else {
        get_next_sqrt_price_from_amount1_rounding_down(sqrt_px96, liquidity, amount_in, true)
    }
}

/// `getNextSqrtPriceFromOutput`
pub fn get_next_sqrt_price_from_output(
    sqrt_px96: U256,
    liquidity: u128,
    amount_out: U256,
    zero_for_one: bool,
) -> Option<U256> {
    if sqrt_px96.is_zero() || liquidity == 0 {
        return None;
    }
    if zero_for_one {
        get_next_sqrt_price_from_amount1_rounding_down(sqrt_px96, liquidity, amount_out, false)
    } else {
        get_next_sqrt_price_from_amount0_rounding_up(sqrt_px96, liquidity, amount_out, false)
    }
}

/// `getAmount0Delta(uint160, uint160, uint128, bool)`
pub fn get_amount0_delta(
    sqrt_ratio_a: U256,
    sqrt_ratio_b: U256,
    liquidity: u128,
    round_up: bool,
) -> Option<U256> {
    let (a, b) = if sqrt_ratio_a > sqrt_ratio_b {
        (sqrt_ratio_b, sqrt_ratio_a)
    } else {
        (sqrt_ratio_a, sqrt_ratio_b)
    };

    let numerator1 = U256::from_u128(liquidity).checked_shl(RESOLUTION)?;
    let numerator2 = b.wrapping_sub(a);

    if a.is_zero() {
        return None; // require(sqrtRatioAX96 > 0)
    }

    if round_up {
        Some(div_rounding_up(
            mul_div_rounding_up(numerator1, numerator2, b)?,
            a,
        ))
    } else {
        mul_div(numerator1, numerator2, b)?.checked_div(a)
    }
}

/// `getAmount1Delta(uint160, uint160, uint128, bool)`
pub fn get_amount1_delta(
    sqrt_ratio_a: U256,
    sqrt_ratio_b: U256,
    liquidity: u128,
    round_up: bool,
) -> Option<U256> {
    let (a, b) = if sqrt_ratio_a > sqrt_ratio_b {
        (sqrt_ratio_b, sqrt_ratio_a)
    } else {
        (sqrt_ratio_a, sqrt_ratio_b)
    };
    let delta = b.wrapping_sub(a);
    if round_up {
        mul_div_rounding_up(U256::from_u128(liquidity), delta, Q96)
    } else {
        mul_div(U256::from_u128(liquidity), delta, Q96)
    }
}

/// `getAmount0Delta(uint160, uint160, int128)` — the signed helper.
pub fn get_amount0_delta_signed(
    sqrt_ratio_a: U256,
    sqrt_ratio_b: U256,
    liquidity: i128,
) -> Option<I256> {
    if liquidity < 0 {
        let magnitude = liquidity.unsigned_abs();
        let amount = get_amount0_delta(sqrt_ratio_a, sqrt_ratio_b, magnitude, false)?;
        Some(I256::to_int256(amount)?.wrapping_neg())
    } else {
        let amount = get_amount0_delta(sqrt_ratio_a, sqrt_ratio_b, liquidity as u128, true)?;
        I256::to_int256(amount)
    }
}

/// `getAmount1Delta(uint160, uint160, int128)` — the signed helper.
pub fn get_amount1_delta_signed(
    sqrt_ratio_a: U256,
    sqrt_ratio_b: U256,
    liquidity: i128,
) -> Option<I256> {
    if liquidity < 0 {
        let magnitude = liquidity.unsigned_abs();
        let amount = get_amount1_delta(sqrt_ratio_a, sqrt_ratio_b, magnitude, false)?;
        Some(I256::to_int256(amount)?.wrapping_neg())
    } else {
        let amount = get_amount1_delta(sqrt_ratio_a, sqrt_ratio_b, liquidity as u128, true)?;
        I256::to_int256(amount)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sqrt_price_100() -> U256 {
        U256::from_u128(792_281_625_142_643_375_935_439_503_360)
    }

    const L: u128 = 1_000_000_000_000_000_000_000; // 10^21

    #[test]
    fn div_rounding_up_matches_evm_edge_cases() {
        assert_eq!(
            div_rounding_up(U256::from_u64(7), U256::from_u64(2)),
            U256::from_u64(4)
        );
        assert_eq!(
            div_rounding_up(U256::from_u64(8), U256::from_u64(2)),
            U256::from_u64(4)
        );
        assert_eq!(div_rounding_up(U256::ZERO, U256::from_u64(2)), U256::ZERO);
        // EVM: division by zero yields zero rather than trapping.
        assert_eq!(div_rounding_up(U256::from_u64(5), U256::ZERO), U256::ZERO);
    }

    #[test]
    fn zero_amount_short_circuits_to_the_input_price() {
        let p = sqrt_price_100();
        assert_eq!(
            get_next_sqrt_price_from_amount0_rounding_up(p, L, U256::ZERO, true).unwrap(),
            p
        );
        assert_eq!(
            get_next_sqrt_price_from_amount0_rounding_up(p, L, U256::ZERO, false).unwrap(),
            p
        );
    }

    #[test]
    fn zero_price_or_liquidity_reverts() {
        assert_eq!(
            get_next_sqrt_price_from_input(U256::ZERO, L, U256::from_u64(1), true),
            None
        );
        assert_eq!(
            get_next_sqrt_price_from_input(sqrt_price_100(), 0, U256::from_u64(1), true),
            None
        );
        assert_eq!(
            get_next_sqrt_price_from_output(U256::ZERO, L, U256::from_u64(1), false),
            None
        );
    }

    #[test]
    fn adding_token0_lowers_the_price_and_adding_token1_raises_it() {
        let p = sqrt_price_100();
        let one = U256::from_u128(1_000_000_000_000_000_000);
        let after0 = get_next_sqrt_price_from_input(p, L, one, true).unwrap();
        assert!(after0 < p, "selling token0 must lower the price");
        let hundred = U256::from_u128(100_000_000_000_000_000_000);
        let after1 = get_next_sqrt_price_from_input(p, L, hundred, false).unwrap();
        assert!(after1 > p, "selling token1 must raise the price");
    }

    #[test]
    fn amount_deltas_round_in_the_documented_direction() {
        let p = sqrt_price_100();
        let lower = crate::univ3::tick_math::get_sqrt_ratio_at_tick(46_000).unwrap();
        let up = get_amount0_delta(lower, p, L, true).unwrap();
        let down = get_amount0_delta(lower, p, L, false).unwrap();
        assert!(up >= down);
        assert!(up.wrapping_sub(down) <= U256::ONE);

        let up1 = get_amount1_delta(lower, p, L, true).unwrap();
        let down1 = get_amount1_delta(lower, p, L, false).unwrap();
        assert!(up1 >= down1);
        assert!(up1.wrapping_sub(down1) <= U256::ONE);
    }

    #[test]
    fn delta_is_symmetric_in_its_price_arguments() {
        let p = sqrt_price_100();
        let q = crate::univ3::tick_math::get_sqrt_ratio_at_tick(46_500).unwrap();
        assert_eq!(
            get_amount0_delta(p, q, L, true).unwrap(),
            get_amount0_delta(q, p, L, true).unwrap()
        );
        assert_eq!(
            get_amount1_delta(p, q, L, false).unwrap(),
            get_amount1_delta(q, p, L, false).unwrap()
        );
    }

    #[test]
    fn zero_lower_price_reverts_in_amount0_delta() {
        assert_eq!(
            get_amount0_delta(U256::ZERO, sqrt_price_100(), L, true),
            None
        );
    }

    #[test]
    fn signed_helpers_flip_sign_and_rounding_together() {
        let p = sqrt_price_100();
        let q = crate::univ3::tick_math::get_sqrt_ratio_at_tick(46_500).unwrap();
        let positive = get_amount0_delta_signed(p, q, 1_000_000).unwrap();
        let negative = get_amount0_delta_signed(p, q, -1_000_000).unwrap();
        assert!(positive.is_positive_or_zero());
        assert!(negative.is_negative());
        // Positive rounds up, negative rounds down, so |negative| <= positive.
        assert!(negative.magnitude() <= positive.magnitude());
        assert!(positive.magnitude().wrapping_sub(negative.magnitude()) <= U256::ONE);
    }

    #[test]
    fn full_range_mint_amounts_match_the_pinned_expectation() {
        // The benchmark's opening position: L = 10^21 over the whole tick range
        // at price 100. Mint rounds up on both sides.
        let p = sqrt_price_100();
        let lower = crate::univ3::tick_math::MIN_SQRT_RATIO;
        let upper = crate::univ3::tick_math::MAX_SQRT_RATIO;
        let amount0 = get_amount0_delta(p, upper, L, true).unwrap();
        let amount1 = get_amount1_delta(lower, p, L, true).unwrap();
        assert_eq!(amount0.to_string(), "99999999999999999946");
        assert_eq!(amount1.to_string(), "9999999999999999999946");
    }
}
