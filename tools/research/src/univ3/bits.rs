//! Bit-level helpers the Uniswap V3 port needs.
//!
//! `TickMath.getTickAtSqrtRatio` is written in assembly and manipulates a
//! two's-complement `int256` with bitwise `or` and an arithmetic shift. The EVM
//! has no notion of signedness in `or`/`shl`/`shr`, so the faithful way to port
//! it is to keep the value as raw 256 bits and only interpret the sign at the
//! end — which is exactly what this module provides.
//!
//! These live here rather than in [`crate::u256`] purely to keep the port
//! self-contained; they are ordinary bit operations with no Uniswap-specific
//! behaviour.

use crate::u256::U256;

#[inline]
pub(crate) fn and(a: U256, b: U256) -> U256 {
    let (x, y) = (a.limbs(), b.limbs());
    U256::from_limbs([x[0] & y[0], x[1] & y[1], x[2] & y[2], x[3] & y[3]])
}

#[inline]
pub(crate) fn or(a: U256, b: U256) -> U256 {
    let (x, y) = (a.limbs(), b.limbs());
    U256::from_limbs([x[0] | y[0], x[1] | y[1], x[2] | y[2], x[3] | y[3]])
}

#[inline]
pub(crate) fn not(a: U256) -> U256 {
    let x = a.limbs();
    U256::from_limbs([!x[0], !x[1], !x[2], !x[3]])
}

/// True when the sign bit (bit 255) is set, i.e. the value read as `int256`
/// would be negative.
#[inline]
pub(crate) fn is_negative(a: U256) -> bool {
    a.limbs()[3] & (1u64 << 63) != 0
}

/// EVM `SAR`: arithmetic right shift on the two's-complement interpretation.
pub(crate) fn sar(a: U256, shift: u32) -> U256 {
    if shift == 0 {
        return a;
    }
    let logical = a.shift_right(shift);
    if !is_negative(a) {
        return logical;
    }
    if shift >= 256 {
        return U256::MAX;
    }
    // Fill the vacated high bits with ones: !( (1 << (256 - shift)) - 1 ).
    let ones = not(U256::ONE
        .checked_shl(256 - shift)
        .map(|v| v.wrapping_sub(U256::ONE))
        .unwrap_or(U256::MAX));
    or(logical, ones)
}

/// Truncate to the low 24 bits and sign-extend, i.e. Solidity's `int24(x)`.
pub(crate) fn to_int24(a: U256) -> i32 {
    let low = (a.limbs()[0] & 0x00ff_ffff) as u32;
    if low & 0x0080_0000 != 0 {
        (low | 0xff00_0000) as i32
    } else {
        low as i32
    }
}

/// Truncate to the low 160 bits, i.e. Solidity's `uint160(x)`.
pub(crate) fn to_uint160_truncating(a: U256) -> U256 {
    let x = a.limbs();
    U256::from_limbs([x[0], x[1], x[2] & 0xffff_ffff, 0])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(text: &str) -> U256 {
        U256::from_dec_str(text).unwrap()
    }

    #[test]
    fn bitwise_ops_match_manual_limb_arithmetic() {
        let a = U256::from_u128(0xf0f0_f0f0_f0f0_f0f0_f0f0_f0f0_f0f0_f0f0);
        let b = U256::from_u128(0x0ff0_0ff0_0ff0_0ff0_0ff0_0ff0_0ff0_0ff0);
        assert_eq!(
            and(a, b),
            U256::from_u128(0x00f0_00f0_00f0_00f0_00f0_00f0_00f0_00f0)
        );
        assert_eq!(or(a, U256::ZERO), a);
        assert_eq!(not(not(a)), a);
        assert_eq!(not(U256::ZERO), U256::MAX);
    }

    #[test]
    fn sar_matches_evm_semantics() {
        // Positive values shift logically.
        assert_eq!(sar(U256::from_u64(256), 4), U256::from_u64(16));
        // -1 stays -1 under any arithmetic shift.
        assert_eq!(sar(U256::MAX, 1), U256::MAX);
        assert_eq!(sar(U256::MAX, 255), U256::MAX);
        // -256 >> 4 == -16 in two's complement.
        let minus_256 = U256::ZERO.wrapping_sub(U256::from_u64(256));
        let minus_16 = U256::ZERO.wrapping_sub(U256::from_u64(16));
        assert_eq!(sar(minus_256, 4), minus_16);
        // Rounding is toward negative infinity, like SAR (not like division).
        let minus_1 = U256::MAX;
        assert_eq!(sar(minus_1, 128), U256::MAX);
    }

    #[test]
    fn sign_detection() {
        assert!(!is_negative(U256::ZERO));
        assert!(!is_negative(dec(
            "57896044618658097711785492504343953926634992332820282019728792003956564819967"
        ))); // 2^255 - 1
        assert!(is_negative(dec(
            "57896044618658097711785492504343953926634992332820282019728792003956564819968"
        ))); // 2^255
        assert!(is_negative(U256::MAX));
    }

    #[test]
    fn int24_truncation_sign_extends() {
        assert_eq!(to_int24(U256::from_u64(46054)), 46054);
        assert_eq!(to_int24(U256::from_u64(0)), 0);
        // -887272 in two's complement, truncated to 24 bits, must come back.
        let minus = U256::ZERO.wrapping_sub(U256::from_u64(887_272));
        assert_eq!(to_int24(minus), -887_272);
        // 0x800000 is the int24 sign bit.
        assert_eq!(to_int24(U256::from_u64(0x0080_0000)), -8_388_608);
    }

    #[test]
    fn uint160_truncation_drops_high_bits() {
        assert_eq!(to_uint160_truncating(U256::MAX).bit_len(), 160);
        let small = U256::from_u128(792_281_625_142_643_375_935_439_503_360);
        assert_eq!(to_uint160_truncating(small), small);
    }
}
