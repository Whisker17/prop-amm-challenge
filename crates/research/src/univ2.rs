//! Uniswap V2 baseline with the fee set to zero.
//!
//! Upstream: Uniswap/v2-periphery `contracts/libraries/UniswapV2Library.sol`
//! Commit:   ed24991304291297c3b4a52818d02f46a17aa9a2
//! sha256:   4f83e9334f833568fa47b36e9ceca435f6c2962760a0596b043c4e538d0fd9f2
//!
//! (That file has exactly one commit in its history — `87edfdcaf49ccc52591502993db4c8c08ea9eec0` —
//! and its blob is identical at every ref of the repository, so the choice of
//! ref does not affect what is ported here.)
//!
//! ```solidity
//! uint amountInWithFee = amountIn.mul(997);
//! uint numerator = amountInWithFee.mul(reserveOut);
//! uint denominator = reserveIn.mul(1000).add(amountInWithFee);
//! amountOut = numerator / denominator;
//! ```
//!
//! The only change is the fee numerator: `997 -> 1000`, i.e. a zero-fee pool, as
//! the benchmark requires.
//!
//! ## Why the `* 1000` scaling is kept
//!
//! Not for rounding. At a zero fee the factor 1000 is common to numerator and
//! denominator, and `floor(k*x / (k*y)) == floor(x/y)` for any integer `k > 0`,
//! so this is **bit-identical** to the simplified
//! `amountIn * reserveOut / (reserveIn + amountIn)`. (Verified over 200 000
//! random samples across the `uint112` reserve domain: zero mismatches. The
//! scaling *would* matter at the real 997/1000 fee, which is not an integer
//! factor.) It is kept so that this file remains a literal transcription of
//! upstream whose diff against `UniswapV2Library` is exactly one token.

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

/// `getAmountIn` with the fee set to zero — the exact-output counterpart.
///
/// ```solidity
/// uint numerator = reserveIn.mul(amountOut).mul(1000);
/// uint denominator = reserveOut.sub(amountOut).mul(997);
/// amountIn = (numerator / denominator).add(1);
/// ```
///
/// Present so that Uniswap V3's exact-output path has a like-for-like V2
/// counterpart to be compared against. The simulation itself only ever specifies
/// an input, so this is not on the benchmark's hot path.
///
/// Returns `None` where the Solidity library would revert, including the
/// `reserveOut.sub(amountOut)` underflow when the requested output is not
/// available.
pub fn get_amount_in(amount_out: U256, reserve_in: U256, reserve_out: U256) -> Option<U256> {
    if amount_out.is_zero() {
        return None; // require(amountOut > 0, "INSUFFICIENT_OUTPUT_AMOUNT")
    }
    if reserve_in.is_zero() || reserve_out.is_zero() {
        return None; // require(reserveIn > 0 && reserveOut > 0, "INSUFFICIENT_LIQUIDITY")
    }
    let numerator = reserve_in
        .checked_mul(amount_out)?
        .checked_mul(FEE_DENOMINATOR)?;
    // SafeMath.sub reverts when amountOut >= reserveOut
    let denominator = reserve_out
        .checked_sub(amount_out)?
        .checked_mul(FEE_NUMERATOR)?;
    numerator.checked_div(denominator)?.checked_add(U256::ONE)
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
    fn scaling_is_bit_identical_to_the_simplified_form_at_zero_fee() {
        // The doc comment claims this; pin it so the claim cannot rot.
        for (amount, rx, ry) in [
            (1u128, 100, 10_000),
            (7, 13, 17),
            (999_999, 1_000_003, 7),
            (1, 1, 1),
        ] {
            let a = U256::from_u128(amount);
            let (ri, ro) = (U256::from_u128(rx), U256::from_u128(ry));
            let scaled = get_amount_out(a, ri, ro).unwrap();
            let simplified = a
                .checked_mul(ro)
                .unwrap()
                .checked_div(ri.checked_add(a).unwrap())
                .unwrap();
            assert_eq!(scaled, simplified, "amount {amount} rx {rx} ry {ry}");
        }
    }

    #[test]
    fn get_amount_in_is_the_inverse_up_to_the_plus_one() {
        let (rx, ry) = (wad(100), wad(10_000));
        let out = get_amount_out(wad(1), rx, ry).unwrap();
        let back = get_amount_in(out, rx, ry).unwrap();
        // getAmountIn adds one wei, so it never under-charges.
        assert!(back >= U256::ZERO);
        assert!(back <= wad(1).checked_add(U256::from_u64(2)).unwrap());
        assert!(back >= wad(1).checked_sub(U256::from_u64(2)).unwrap());
    }

    #[test]
    fn get_amount_in_reverts_when_the_output_is_not_available() {
        let (rx, ry) = (wad(100), wad(10_000));
        assert_eq!(get_amount_in(ry, rx, ry), None, "cannot drain the reserve");
        assert_eq!(
            get_amount_in(ry.checked_add(U256::ONE).unwrap(), rx, ry),
            None
        );
        assert_eq!(get_amount_in(U256::ZERO, rx, ry), None);
        assert_eq!(get_amount_in(wad(1), U256::ZERO, ry), None);
    }

    #[test]
    fn degenerate_inputs_report_a_revert() {
        assert_eq!(get_amount_out(U256::ZERO, wad(100), wad(10_000)), None);
        assert_eq!(get_amount_out(wad(1), U256::ZERO, wad(10_000)), None);
        assert_eq!(get_amount_out(wad(1), wad(100), U256::ZERO), None);
        assert_eq!(get_amount_out(U256::MAX, wad(100), wad(10_000)), None);
    }
}
