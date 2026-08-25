//! The one and only floating-point boundary in this crate.
//!
//! The simulation ledger works in `f64` and in `nano` (1e9) fixed point; the
//! ported curves work in WAD (1e18) integers. This module holds every
//! conversion between the two worlds:
//!
//! * [`price_to_wad`] quantises an oracle price **exactly once**, using the
//!   binary expansion of the `f64` so that no precision is silently invented.
//! * [`nano_to_wad`] is an exact multiplication by 1e9.
//! * [`wad_to_nano`] truncates toward zero, matching how the simulation itself
//!   quantises amounts.
//!
//! Nothing below this boundary touches `f64`.

use crate::u256::U256;

/// `10 ** 18`
pub const WAD: U256 = U256::from_u128(1_000_000_000_000_000_000);
/// `10 ** 9` — the simulation's `nano` scale.
pub const NANO: U256 = U256::from_u128(1_000_000_000);
/// Multiplier between the two scales (`1e18 / 1e9`).
pub const NANO_TO_WAD: U256 = U256::from_u128(1_000_000_000);

/// Quantise an oracle price into a WAD integer, exactly once.
///
/// The result is `floor(price * 10^18)` computed on the exact binary value of
/// `price` (an `f64` is `mantissa * 2^exponent`), never by multiplying in
/// floating point. Returns `None` for non-finite or non-positive prices and for
/// prices too large to fit a `uint256`.
pub fn price_to_wad(price: f64) -> Option<U256> {
    if !price.is_finite() || price <= 0.0 {
        return None;
    }
    let bits = price.to_bits();
    let raw_exponent = ((bits >> 52) & 0x7ff) as i32;
    let raw_mantissa = bits & ((1u64 << 52) - 1);
    // Subnormals have no implicit leading one and a fixed exponent.
    let (mantissa, exponent) = if raw_exponent == 0 {
        (raw_mantissa, -1074i32)
    } else {
        (raw_mantissa | (1u64 << 52), raw_exponent - 1075)
    };

    let scaled = U256::from_u64(mantissa).checked_mul(WAD)?;
    if exponent >= 0 {
        scaled.checked_shl(exponent as u32)
    } else {
        // Truncating shift == floor for non-negative values.
        Some(scaled.shift_right((-exponent) as u32))
    }
}

/// Exact conversion from the simulation's `nano` fixed point to WAD.
#[inline]
pub fn nano_to_wad(value: u64) -> U256 {
    U256::from_u64(value)
        .checked_mul(NANO_TO_WAD)
        .expect("u64 * 1e9 always fits in 256 bits")
}

/// Truncating conversion from WAD back to the simulation's `nano` fixed point.
///
/// Saturates at `u64::MAX`, which the simulation already treats as an
/// out-of-range quote.
#[inline]
pub fn wad_to_nano(value: U256) -> u64 {
    let (quotient, _) = value.div_rem_small(1_000_000_000);
    quotient.as_u64().unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dyadic_prices_quantise_exactly() {
        assert_eq!(
            price_to_wad(100.0).unwrap(),
            U256::from_u128(100_000_000_000_000_000_000)
        );
        assert_eq!(price_to_wad(1.0).unwrap(), WAD);
        assert_eq!(
            price_to_wad(0.5).unwrap(),
            U256::from_u128(500_000_000_000_000_000)
        );
        assert_eq!(
            price_to_wad(1024.0).unwrap(),
            WAD.checked_mul(U256::from_u64(1024)).unwrap()
        );
    }

    #[test]
    fn non_dyadic_prices_floor_the_exact_binary_value() {
        // 0.1_f64 is slightly above one tenth, so the exact floor carries the
        // representation error instead of hiding it.
        assert_eq!(
            price_to_wad(0.1).unwrap(),
            U256::from_u128(100_000_000_000_000_005)
        );
        // 100.7 is likewise not dyadic; the quantisation is deterministic.
        let value = price_to_wad(100.7).unwrap();
        assert!(value > U256::from_u128(100_699_999_999_999_990_000));
        assert!(value < U256::from_u128(100_700_000_000_000_010_000));
    }

    #[test]
    fn rejects_non_positive_and_non_finite_prices() {
        assert_eq!(price_to_wad(0.0), None);
        assert_eq!(price_to_wad(-1.0), None);
        assert_eq!(price_to_wad(f64::NAN), None);
        assert_eq!(price_to_wad(f64::INFINITY), None);
    }

    #[test]
    fn subnormal_prices_do_not_panic() {
        assert_eq!(price_to_wad(f64::from_bits(1)).unwrap(), U256::ZERO);
    }

    #[test]
    fn nano_round_trip_is_exact_on_nano_multiples() {
        for nano in [0u64, 1, 1_000_000_000, 100_000_000_000, u64::MAX] {
            assert_eq!(wad_to_nano(nano_to_wad(nano)), nano);
        }
    }

    #[test]
    fn wad_to_nano_truncates() {
        assert_eq!(wad_to_nano(U256::from_u64(999_999_999)), 0);
        assert_eq!(wad_to_nano(U256::from_u64(1_999_999_999)), 1);
        assert_eq!(wad_to_nano(U256::MAX), u64::MAX);
    }

    #[test]
    fn quantisation_is_deterministic_across_repeated_calls() {
        let price = 137.913_571_357_913_57_f64;
        let first = price_to_wad(price).unwrap();
        for _ in 0..8 {
            assert_eq!(price_to_wad(price).unwrap(), first);
        }
    }
}
