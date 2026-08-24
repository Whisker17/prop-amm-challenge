//! Two's-complement `int256` / `int128` semantics for the Uniswap V3 port.
//!
//! Uniswap V3 is compiled at Solidity **0.7.6**, where signed arithmetic wraps
//! and casts truncate. This type therefore stores raw 256 bits and applies the
//! EVM's interpretation, rather than trying to map onto a checked Rust integer.
//!
//! `SafeCast` (`toUint160` / `toInt128` / `toInt256`) is ported here with its
//! exact revert conditions, since those are the only places upstream *does*
//! check.
//!
//! Upstream: Uniswap/v3-core `contracts/libraries/SafeCast.sol`
//! Commit:   e3589b192d0be27e100cd0daaf6c97204fdb1899, sha256
//!           9aed494b56d3dd16b7d6535583ded2cdfb03dc80aaa919347b13d35fd597e8bf

use super::bits;
use crate::u256::U256;

/// A 256-bit two's-complement signed integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct I256(U256);

impl I256 {
    pub const ZERO: I256 = I256(U256::ZERO);

    /// `2^255`, i.e. `type(int256).min`.
    pub fn min_value() -> I256 {
        I256(U256::ONE.checked_shl(255).expect("2^255 fits"))
    }

    /// `2^255 - 1`, i.e. `type(int256).max`.
    pub fn max_value() -> I256 {
        I256(Self::min_value().0.wrapping_sub(U256::ONE))
    }

    #[inline]
    pub fn from_raw(raw: U256) -> I256 {
        I256(raw)
    }

    #[inline]
    pub fn to_raw(self) -> U256 {
        self.0
    }

    pub fn from_i128(value: i128) -> I256 {
        if value >= 0 {
            I256(U256::from_u128(value as u128))
        } else {
            I256(U256::ZERO.wrapping_sub(U256::from_u128(value.unsigned_abs())))
        }
    }

    /// `SafeCast.toInt256(uint256 y)`: `require(y < 2**255)`.
    pub fn to_int256(value: U256) -> Option<I256> {
        if value < Self::min_value().0 {
            Some(I256(value))
        } else {
            None
        }
    }

    #[inline]
    pub fn is_negative(self) -> bool {
        bits::is_negative(self.0)
    }

    #[inline]
    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }

    #[inline]
    pub fn is_positive_or_zero(self) -> bool {
        !self.is_negative()
    }

    /// Magnitude as an unsigned value, i.e. Solidity's `uint256(-x)` for a
    /// negative `x`. For `type(int256).min` this is `2^255`, matching the EVM's
    /// wrapping negation.
    pub fn magnitude(self) -> U256 {
        if self.is_negative() {
            U256::ZERO.wrapping_sub(self.0)
        } else {
            self.0
        }
    }

    /// Wrapping negation, as `-x` behaves in Solidity 0.7.6.
    #[inline]
    pub fn wrapping_neg(self) -> I256 {
        I256(U256::ZERO.wrapping_sub(self.0))
    }

    /// `LowGasSafeMath.add(int256 a, int256 b)`:
    /// `require((z = a + b) >= a == (b >= 0))`.
    pub fn checked_add(self, other: I256) -> Option<I256> {
        let z = I256(self.0.wrapping_add(other.0));
        if (z >= self) == other.is_positive_or_zero() {
            Some(z)
        } else {
            None
        }
    }

    /// `LowGasSafeMath.sub(int256 a, int256 b)`:
    /// `require((z = a - b) <= a == (b >= 0))`.
    pub fn checked_sub(self, other: I256) -> Option<I256> {
        let z = I256(self.0.wrapping_sub(other.0));
        if (z <= self) == other.is_positive_or_zero() {
            Some(z)
        } else {
            None
        }
    }

    #[inline]
    pub fn wrapping_add(self, other: I256) -> I256 {
        I256(self.0.wrapping_add(other.0))
    }

    #[inline]
    pub fn wrapping_sub(self, other: I256) -> I256 {
        I256(self.0.wrapping_sub(other.0))
    }

    /// `SafeCast.toInt128(int256 y)`: `require((z = int128(y)) == y)`.
    pub fn to_int128(self) -> Option<i128> {
        let low = self.0.limbs();
        let truncated = (low[0] as u128) | ((low[1] as u128) << 64);
        let value = truncated as i128;
        if I256::from_i128(value) == self {
            Some(value)
        } else {
            None
        }
    }

    /// Render as a decimal string with a sign, for vectors and reports.
    pub fn to_string_signed(self) -> String {
        if self.is_negative() {
            format!("-{}", self.magnitude())
        } else {
            self.0.to_string()
        }
    }
}

impl PartialOrd for I256 {
    fn partial_cmp(&self, other: &I256) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for I256 {
    fn cmp(&self, other: &I256) -> std::cmp::Ordering {
        match (self.is_negative(), other.is_negative()) {
            (false, true) => std::cmp::Ordering::Greater,
            (true, false) => std::cmp::Ordering::Less,
            _ => self.0.cmp(&other.0),
        }
    }
}

/// `SafeCast.toUint160(uint256 y)`: `require((z = uint160(y)) == y)`.
pub fn to_uint160(value: U256) -> Option<U256> {
    if bits::to_uint160_truncating(value) == value {
        Some(value)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_is_signed_not_unsigned() {
        let minus_one = I256::from_i128(-1);
        let one = I256::from_i128(1);
        assert!(minus_one < one);
        assert!(minus_one < I256::ZERO);
        assert!(I256::min_value() < I256::max_value());
        // The raw bits of -1 are larger than those of 1; signed order must win.
        assert!(minus_one.to_raw() > one.to_raw());
    }

    #[test]
    fn magnitude_and_negation() {
        assert_eq!(I256::from_i128(-5).magnitude(), U256::from_u64(5));
        assert_eq!(I256::from_i128(5).magnitude(), U256::from_u64(5));
        assert_eq!(I256::ZERO.magnitude(), U256::ZERO);
        // Wrapping negation of int256::min is itself, as on the EVM.
        assert_eq!(I256::min_value().wrapping_neg(), I256::min_value());
        assert_eq!(I256::min_value().magnitude(), I256::min_value().to_raw());
    }

    #[test]
    fn checked_add_and_sub_match_low_gas_safe_math() {
        assert_eq!(
            I256::from_i128(3).checked_add(I256::from_i128(-5)),
            Some(I256::from_i128(-2))
        );
        assert_eq!(I256::max_value().checked_add(I256::from_i128(1)), None);
        assert_eq!(I256::min_value().checked_sub(I256::from_i128(1)), None);
        assert_eq!(
            I256::min_value().checked_add(I256::from_i128(-1)),
            None,
            "adding a negative to int256::min must overflow"
        );
        assert_eq!(
            I256::from_i128(-5).checked_sub(I256::from_i128(-5)),
            Some(I256::ZERO)
        );
    }

    #[test]
    fn to_int256_rejects_values_at_or_above_2_pow_255() {
        assert_eq!(I256::to_int256(U256::from_u64(7)), Some(I256::from_i128(7)));
        assert_eq!(
            I256::to_int256(I256::max_value().to_raw()),
            Some(I256::max_value())
        );
        assert_eq!(I256::to_int256(I256::min_value().to_raw()), None);
        assert_eq!(I256::to_int256(U256::MAX), None);
    }

    #[test]
    fn to_int128_narrows_exactly() {
        assert_eq!(I256::from_i128(i128::MAX).to_int128(), Some(i128::MAX));
        assert_eq!(I256::from_i128(i128::MIN).to_int128(), Some(i128::MIN));
        assert_eq!(I256::from_i128(-1).to_int128(), Some(-1));
        // One past int128::max must fail.
        let too_big = I256::from_i128(i128::MAX)
            .checked_add(I256::from_i128(1))
            .unwrap();
        assert_eq!(too_big.to_int128(), None);
    }

    #[test]
    fn to_uint160_boundary() {
        let max160 = U256::ONE.checked_shl(160).unwrap().wrapping_sub(U256::ONE);
        assert_eq!(to_uint160(max160), Some(max160));
        assert_eq!(to_uint160(max160.wrapping_add(U256::ONE)), None);
        assert_eq!(to_uint160(U256::ZERO), Some(U256::ZERO));
    }

    #[test]
    fn signed_string_rendering() {
        assert_eq!(I256::from_i128(-1234).to_string_signed(), "-1234");
        assert_eq!(I256::from_i128(1234).to_string_signed(), "1234");
        assert_eq!(I256::ZERO.to_string_signed(), "0");
    }
}
