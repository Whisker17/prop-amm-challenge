//! Uniswap V2 baseline with the fee set to zero.
//!
//! This is `UniswapV2Library.getAmountOut` from
//! <https://github.com/Uniswap/v2-periphery> `contracts/libraries/UniswapV2Library.sol`,
//! kept in its original shape:
//!
//! ```solidity
//! uint amountInWithFee = amountIn.mul(997);
//! uint numerator = amountInWithFee.mul(reserveOut);
//! uint denominator = reserveIn.mul(1000).add(amountInWithFee);
//! amountOut = numerator / denominator;
//! ```
//!
//! The only change is the fee numerator: `997 -> 1000`, i.e. a zero-fee pool, as
//! the benchmark requires. The `* 1000` scaling and the single truncating
//! division are preserved so the rounding behaviour stays that of the original
//! implementation rather than an algebraically "simplified" variant.

use crate::u256::U256;

/// Fee numerator for a zero-fee pool (`1000 - 0`); upstream uses `997`.
pub const FEE_NUMERATOR: U256 = U256::from_u128(1000);
/// Fee denominator, as upstream.
pub const FEE_DENOMINATOR: U256 = U256::from_u128(1000);

/// `getAmountOut` with the fee set to zero.
///
/// Returns `None` where the Solidity library would revert: non-positive input,
/// empty reserves, or arithmetic overflow.
pub fn get_amount_out(amount_in: U256, reserve_in: U256, reserve_out: U256) -> Option<U256> {
    if amount_in.is_zero() {
        return None; // require(amountIn > 0, "INSUFFICIENT_INPUT_AMOUNT")
    }
    if reserve_in.is_zero() || reserve_out.is_zero() {
        return None; // require(reserveIn > 0 && reserveOut > 0, "INSUFFICIENT_LIQUIDITY")
    }
    let amount_in_with_fee = amount_in.checked_mul(FEE_NUMERATOR)?;
    let numerator = amount_in_with_fee.checked_mul(reserve_out)?;
    let denominator = reserve_in
        .checked_mul(FEE_DENOMINATOR)?
        .checked_add(amount_in_with_fee)?;
    numerator.checked_div(denominator)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WAD: u128 = 1_000_000_000_000_000_000;

    fn wad(units: u128) -> U256 {
        U256::from_u128(units * WAD)
    }

    #[test]
    fn zero_fee_output_matches_the_textbook_constant_product_value() {
        // 10 000 * 1 / (100 + 1) = 99.0099… Y for 1 X in.
        let out = get_amount_out(wad(1), wad(100), wad(10_000)).unwrap();
        assert_eq!(out, U256::from_u128(99_009_900_990_099_009_900));
    }

    #[test]
    fn the_invariant_never_decreases() {
        let (rx, ry) = (wad(100), wad(10_000));
        for units in [1u128, 2, 5, 10, 50] {
            let amount_in = wad(units);
            let out = get_amount_out(amount_in, rx, ry).unwrap();
            let k_before = rx.checked_mul(ry).unwrap();
            let k_after = rx
                .checked_add(amount_in)
                .unwrap()
                .checked_mul(ry.checked_sub(out).unwrap())
                .unwrap();
            assert!(k_after >= k_before, "constant product must not shrink");
        }
    }

    #[test]
    fn output_is_strictly_below_the_output_reserve() {
        let out = get_amount_out(wad(1_000_000), wad(100), wad(10_000)).unwrap();
        assert!(out < wad(10_000));
    }

    #[test]
    fn degenerate_inputs_report_a_revert() {
        assert_eq!(get_amount_out(U256::ZERO, wad(100), wad(10_000)), None);
        assert_eq!(get_amount_out(wad(1), U256::ZERO, wad(10_000)), None);
        assert_eq!(get_amount_out(wad(1), wad(100), U256::ZERO), None);
        assert_eq!(get_amount_out(U256::MAX, wad(100), wad(10_000)), None);
    }
}
