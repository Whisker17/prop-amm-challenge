//! Verbatim Rust port of DODO `DecimalMath`.
//!
//! Upstream: DODOEX/contractV2 `contracts/lib/DecimalMath.sol`
//! Commit:   8da3ee1ec50966fca9a2c80d424040c45c0f785e
//! Read via: mantle-propamm-contracts `src/vendor/dodo/DecimalMath.sol`
//!
//! Operation order, `floor`/`ceil` choices and the `10**18` / `10**36`
//! constants are preserved exactly. `None` means "the Solidity call would have
//! reverted" (arithmetic overflow, or division by zero).

use crate::u256::U256;

/// `10 ** 18`
pub const ONE: U256 = U256::from_u128(1_000_000_000_000_000_000);
/// `10 ** 36`
pub const ONE2: U256 = U256::from_u128(1_000_000_000_000_000_000_000_000_000_000_000_000);

/// `(target * d) / 10**18`
#[inline]
pub fn mul_floor(target: U256, d: U256) -> Option<U256> {
    target.checked_mul(d)?.checked_div(ONE)
}

/// `_divCeil(target * d, 10**18)`
#[inline]
pub fn mul_ceil(target: U256, d: U256) -> Option<U256> {
    div_ceil_raw(target.checked_mul(d)?, ONE)
}

/// `(target * 10**18) / d`
#[inline]
pub fn div_floor(target: U256, d: U256) -> Option<U256> {
    target.checked_mul(ONE)?.checked_div(d)
}

/// `_divCeil(target * 10**18, d)`
#[inline]
pub fn div_ceil(target: U256, d: U256) -> Option<U256> {
    div_ceil_raw(target.checked_mul(ONE)?, d)
}

/// `10**36 / target`
#[inline]
pub fn reciprocal_floor(target: U256) -> Option<U256> {
    ONE2.checked_div(target)
}

/// `_divCeil(10**36, target)`
#[inline]
pub fn reciprocal_ceil(target: U256) -> Option<U256> {
    div_ceil_raw(ONE2, target)
}

/// DODO `_divCeil`: `quotient = a / b; remainder = a - quotient * b; remainder > 0 ? quotient + 1 : quotient`
#[inline]
pub fn div_ceil_raw(a: U256, b: U256) -> Option<U256> {
    let quotient = a.checked_div(b)?;
    let remainder = a.checked_sub(quotient.checked_mul(b)?)?;
    if !remainder.is_zero() {
        quotient.checked_add(U256::ONE)
    } else {
        Some(quotient)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(value: u128) -> U256 {
        U256::from_u128(value)
    }

    #[test]
    fn one_constants_are_powers_of_ten() {
        assert_eq!(ONE.to_string(), "1000000000000000000");
        assert_eq!(ONE2.to_string(), "1000000000000000000000000000000000000");
        assert_eq!(ONE.checked_mul(ONE).unwrap(), ONE2);
    }

    #[test]
    fn floor_and_ceil_differ_only_on_a_remainder() {
        // 1 wei * 0.333… leaves a remainder below one wei: floor truncates to
        // zero, ceil rounds up to one.
        let third = ONE.checked_div(u(3)).unwrap();
        assert_eq!(mul_floor(U256::ONE, third).unwrap(), U256::ZERO);
        assert_eq!(mul_ceil(U256::ONE, third).unwrap(), U256::ONE);
        // Exact division rounds identically both ways.
        assert_eq!(mul_floor(u(4), ONE).unwrap(), u(4));
        assert_eq!(mul_ceil(u(4), ONE).unwrap(), u(4));
        assert_eq!(
            mul_floor(ONE.checked_mul(u(3)).unwrap(), third).unwrap(),
            u(999_999_999_999_999_999)
        );
    }

    #[test]
    fn div_floor_and_div_ceil() {
        assert_eq!(div_floor(u(1), u(3)).unwrap(), u(333_333_333_333_333_333));
        assert_eq!(div_ceil(u(1), u(3)).unwrap(), u(333_333_333_333_333_334));
        assert_eq!(div_floor(u(2), u(1)).unwrap(), ONE.checked_mul(u(2)).unwrap());
    }

    #[test]
    fn reciprocals() {
        assert_eq!(reciprocal_floor(ONE).unwrap(), ONE);
        assert_eq!(reciprocal_floor(u(3)).unwrap(), ONE2.checked_div(u(3)).unwrap());
        assert_eq!(
            reciprocal_ceil(u(3)).unwrap(),
            ONE2.checked_div(u(3)).unwrap().checked_add(U256::ONE).unwrap()
        );
    }

    #[test]
    fn reverts_are_reported_as_none() {
        assert_eq!(mul_floor(U256::MAX, u(2)), None);
        assert_eq!(div_floor(U256::MAX, u(1)), None);
        assert_eq!(reciprocal_floor(U256::ZERO), None);
        assert_eq!(div_ceil_raw(U256::ONE, U256::ZERO), None);
    }
}
