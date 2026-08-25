//! Verbatim Rust port of DODO `DODOMath`.
//!
//! Upstream: DODOEX/contractV2 `contracts/lib/DODOMath.sol`
//! Commit:   8da3ee1ec50966fca9a2c80d424040c45c0f785e
//! Read via: mantle-propamm-contracts `src/vendor/dodo/DODOMath.sol`
//!
//! Preserved literally: the `k == 0` and `k == ONE` special branches, the
//! integration formula, both quadratic solvers, the operand order of every
//! multiplication and division, the overflow-avoidance branch selected by
//! `_multiplicationDoesNotOverflow`, the `bSig` sign logic, the `V2 > V1`
//! clamp and the Babylonian `_sqrt` loop.
//!
//! `None` is returned exactly where the Solidity code would revert (a failing
//! `require`, checked-arithmetic overflow, or division by zero).

use super::decimal_math::{self as decimal_math, ONE, ONE2};
use crate::u256::U256;

/// `_GeneralIntegrate(V0, V1, V2, i, k)` — rounds down.
///
/// `res = i*delta*(1-k+k(V0^2/V1/V2))`
pub fn general_integrate(v0: U256, v1: U256, v2: U256, i: U256, k: U256) -> Option<U256> {
    if v0.is_zero() {
        return None; // require(V0 > 0, "TARGET_IS_ZERO")
    }
    let fair_amount = i.checked_mul(v1.checked_sub(v2)?)?; // i*delta
    if k.is_zero() {
        return fair_amount.checked_div(ONE);
    }
    let v0v0v1v2 = decimal_math::div_floor(v0.checked_mul(v0)?.checked_div(v1)?, v2)?;
    let penalty = decimal_math::mul_floor(k, v0v0v1v2)?; // k(V0^2/V1/V2)
    ONE.checked_sub(k)?
        .checked_add(penalty)?
        .checked_mul(fair_amount)?
        .checked_div(ONE2)
}

/// `_SolveQuadraticFunctionForTarget(V1, delta, i, k)` — rounds down.
pub fn solve_quadratic_function_for_target(
    v1: U256,
    delta: U256,
    i: U256,
    k: U256,
) -> Option<U256> {
    if v1.is_zero() {
        return Some(U256::ZERO);
    }
    if k.is_zero() {
        return v1.checked_add(decimal_math::mul_floor(i, delta)?);
    }
    // V0 = V1*(1+(sqrt-1)/2k), sqrt = √(1+4kidelta/V1)
    let sqrt_value;
    let ki = U256::from_u64(4).checked_mul(k)?.checked_mul(i)?;
    if ki.is_zero() {
        sqrt_value = ONE;
    } else if multiplication_does_not_overflow(ki, delta)? {
        sqrt_value = sqrt(ki.checked_mul(delta)?.checked_div(v1)?.checked_add(ONE2)?);
    } else {
        sqrt_value = sqrt(ki.checked_div(v1)?.checked_mul(delta)?.checked_add(ONE2)?);
    }
    let premium = decimal_math::div_floor(
        sqrt_value.checked_sub(ONE)?,
        k.checked_mul(U256::from_u64(2))?,
    )?
    .checked_add(ONE)?;
    // V0 is greater than or equal to V1 according to the solution
    decimal_math::mul_floor(v1, premium)
}

/// `_SolveQuadraticFunctionForTrade(V0, V1, delta, i, k)` — rounds down.
///
/// Returns `|Q1 - Q2|`.
pub fn solve_quadratic_function_for_trade(
    v0: U256,
    v1: U256,
    delta: U256,
    i: U256,
    k: U256,
) -> Option<U256> {
    if v0.is_zero() {
        return None; // require(V0 > 0, "TARGET_IS_ZERO")
    }
    if delta.is_zero() {
        return Some(U256::ZERO);
    }

    if k.is_zero() {
        let fair = decimal_math::mul_floor(i, delta)?;
        return Some(if fair > v1 { v1 } else { fair });
    }

    if k == ONE {
        // Q2 = Q1/(1+ideltaBQ1/Q0/Q0); returns Q1*(temp/(1+temp))
        let temp;
        let idelta = i.checked_mul(delta)?;
        if idelta.is_zero() {
            temp = U256::ZERO;
        } else if multiplication_does_not_overflow(idelta, v1)? {
            temp = idelta.checked_mul(v1)?.checked_div(v0.checked_mul(v0)?)?;
        } else {
            temp = delta
                .checked_mul(v1)?
                .checked_div(v0)?
                .checked_mul(i)?
                .checked_div(v0)?;
        }
        return v1.checked_mul(temp)?.checked_div(temp.checked_add(ONE)?);
    }

    // part2 = kQ0^2/Q1 - i*deltaB
    let part2 = k
        .checked_mul(v0)?
        .checked_div(v1)?
        .checked_mul(v0)?
        .checked_add(i.checked_mul(delta)?)?;
    // (1-k)Q1
    let mut b_abs = ONE.checked_sub(k)?.checked_mul(v1)?;

    let b_sig;
    if b_abs >= part2 {
        b_abs = b_abs.checked_sub(part2)?;
        b_sig = false;
    } else {
        b_abs = part2.checked_sub(b_abs)?;
        b_sig = true;
    }
    b_abs = b_abs.checked_div(ONE)?;

    // 4(1-k)kQ0^2
    let mut square_root = decimal_math::mul_floor(
        ONE.checked_sub(k)?.checked_mul(U256::from_u64(4))?,
        decimal_math::mul_floor(k, v0)?.checked_mul(v0)?,
    )?;
    // sqrt(b*b + 4(1-k)kQ0*Q0)
    square_root = sqrt(b_abs.checked_mul(b_abs)?.checked_add(square_root)?);

    let denominator = ONE.checked_sub(k)?.checked_mul(U256::from_u64(2))?; // 2(1-k)
    let numerator = if b_sig {
        square_root.checked_sub(b_abs)?
    } else {
        b_abs.checked_add(square_root)?
    };

    let v2 = decimal_math::div_ceil(numerator, denominator)?;
    if v2 > v1 {
        Some(U256::ZERO)
    } else {
        v1.checked_sub(v2)
    }
}

/// `unchecked { (a * b) / a == b }`
///
/// The multiplication wraps (it is inside an `unchecked` block upstream) but the
/// division still reverts when `a == 0`.
pub fn multiplication_does_not_overflow(a: U256, b: U256) -> Option<bool> {
    Some(a.wrapping_mul(b).checked_div(a)? == b)
}

/// DODO `_sqrt`: Babylonian iteration, returns `floor(sqrt(x))`.
///
/// The iteration starts at `x / 2 + 1` and decreases, so `x / z + z` stays below
/// `x / 2 + 3`; the `expect`s below are unreachable, and are written this way so
/// that an unexpected overflow would surface instead of silently wrapping (the
/// upstream file uses Solidity 0.8 checked arithmetic).
pub fn sqrt(x: U256) -> U256 {
    let mut z = x
        .shift_right(1)
        .checked_add(U256::ONE)
        .expect("x / 2 + 1 cannot overflow"); // x / 2 + 1
    let mut y = x;
    while z < y {
        y = z;
        // (x / z + z) / 2
        z = x
            .checked_div(z)
            .expect("z is non-zero")
            .checked_add(z)
            .expect("x / z + z cannot overflow for a decreasing z")
            .shift_right(1);
    }
    y
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(value: u128) -> U256 {
        U256::from_u128(value)
    }

    #[test]
    fn sqrt_matches_floor_sqrt_except_for_the_upstream_x_equals_two_quirk() {
        // `_sqrt` starts at `y = x` with `z = x/2 + 1`, so for x = 2 the loop
        // never runs and the function returns 2 instead of 1. This is upstream
        // behaviour at the pinned commit and is preserved deliberately.
        assert_eq!(sqrt(u(2)), u(2));

        for n in 0u128..2_000 {
            if n == 2 {
                continue;
            }
            let expected = (n as f64).sqrt().floor() as u128;
            assert_eq!(sqrt(u(n)), u(expected), "sqrt({n})");
        }
        assert_eq!(sqrt(ONE2), ONE);
        assert_eq!(sqrt(U256::ZERO), U256::ZERO);
        assert_eq!(sqrt(U256::ONE), U256::ONE);
    }

    #[test]
    fn general_integrate_k_zero_is_the_linear_branch() {
        // k = 0 collapses to i*delta/1e18.
        let out = general_integrate(
            u(100),
            u(60),
            u(50),
            ONE.checked_mul(u(2)).unwrap(),
            U256::ZERO,
        )
        .unwrap();
        assert_eq!(out, u(20));
    }

    #[test]
    fn general_integrate_requires_non_zero_target() {
        assert_eq!(
            general_integrate(U256::ZERO, u(2), u(1), ONE, U256::ZERO),
            None
        );
    }

    #[test]
    fn solve_for_trade_k_zero_clamps_at_v1() {
        let i = ONE.checked_mul(u(10)).unwrap();
        // i*delta = 10 * 5 = 50 > V1 = 30, so the result clamps to V1.
        let out = solve_quadratic_function_for_trade(u(100), u(30), u(5), i, U256::ZERO).unwrap();
        assert_eq!(out, u(30));
    }

    #[test]
    fn solve_for_trade_returns_zero_for_zero_delta() {
        assert_eq!(
            solve_quadratic_function_for_trade(u(100), u(100), U256::ZERO, ONE, ONE).unwrap(),
            U256::ZERO
        );
    }

    #[test]
    fn solve_for_target_k_zero_is_additive() {
        let out = solve_quadratic_function_for_target(
            u(100),
            u(10),
            ONE.checked_mul(u(3)).unwrap(),
            U256::ZERO,
        )
        .unwrap();
        assert_eq!(out, u(130));
    }

    #[test]
    fn solve_for_target_zero_v1_is_zero() {
        assert_eq!(
            solve_quadratic_function_for_target(U256::ZERO, u(10), ONE, ONE).unwrap(),
            U256::ZERO
        );
    }

    #[test]
    fn multiplication_overflow_probe_matches_solidity_semantics() {
        assert_eq!(multiplication_does_not_overflow(u(2), u(3)), Some(true));
        assert_eq!(
            multiplication_does_not_overflow(U256::MAX, u(2)),
            Some(false)
        );
        // a == 0 divides by zero, which reverts even inside `unchecked`.
        assert_eq!(multiplication_does_not_overflow(U256::ZERO, u(3)), None);
    }
}
