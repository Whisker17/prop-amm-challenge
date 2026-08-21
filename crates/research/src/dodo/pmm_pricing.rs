//! Verbatim Rust port of DODO `PMMPricing`.
//!
//! Upstream: DODOEX/contractV2 `contracts/lib/PMMPricing.sol`
//! Commit:   8da3ee1ec50966fca9a2c80d424040c45c0f785e
//! Read via: mantle-propamm-contracts `src/vendor/dodo/PMMPricing.sol`
//!
//! Preserved literally: the `RState` machine (`ONE` / `ABOVE_ONE` / `BELOW_ONE`),
//! the branch order in `sellBaseToken` / `sellQuoteToken`, the three
//! return-to-one cases (`<`, `==`, `>`), the "important corner case" clamp to
//! `backToOne*`, `adjustedTarget`, `getMidPrice`, and which side of each pair is
//! priced with `reciprocalFloor(i)`.
//!
//! `None` is returned exactly where the Solidity code would revert.

use super::decimal_math::{self as decimal_math, ONE};
use super::dodo_math;
use crate::u256::U256;

/// `RState` from `MantlePropAmmTypes.sol`, numerically identical to upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RState {
    One = 0,
    AboveOne = 1,
    BelowOne = 2,
}

impl RState {
    pub fn from_u8(value: u8) -> Option<RState> {
        match value {
            0 => Some(RState::One),
            1 => Some(RState::AboveOne),
            2 => Some(RState::BelowOne),
            _ => None,
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// `PMMPricing.PMMState`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PmmState {
    pub i: U256,
    pub k: U256,
    pub b: U256,
    pub q: U256,
    pub b0: U256,
    pub q0: U256,
    pub r: RState,
}

// ============ buy & sell ============

pub fn sell_base_token(state: &PmmState, pay_base_amount: U256) -> Option<(U256, RState)> {
    let receive_quote_amount;
    let new_r;
    if state.r == RState::One {
        // case 1: R=1, R falls below one
        receive_quote_amount = r_one_sell_base_token(state, pay_base_amount)?;
        new_r = RState::BelowOne;
    } else if state.r == RState::AboveOne {
        let back_to_one_pay_base = state.b0.checked_sub(state.b)?;
        let back_to_one_receive_quote = state.q.checked_sub(state.q0)?;
        // case 2: R>1, R status depends on trading amount
        if pay_base_amount < back_to_one_pay_base {
            // case 2.1: R status does not change
            let mut amount = r_above_sell_base_token(state, pay_base_amount)?;
            new_r = RState::AboveOne;
            if amount > back_to_one_receive_quote {
                // [Important corner case!] keep spare quote >= 0
                amount = back_to_one_receive_quote;
            }
            receive_quote_amount = amount;
        } else if pay_base_amount == back_to_one_pay_base {
            // case 2.2: R status changes to ONE
            receive_quote_amount = back_to_one_receive_quote;
            new_r = RState::One;
        } else {
            // case 2.3: R status changes to BELOW_ONE
            receive_quote_amount = back_to_one_receive_quote.checked_add(r_one_sell_base_token(
                state,
                pay_base_amount.checked_sub(back_to_one_pay_base)?,
            )?)?;
            new_r = RState::BelowOne;
        }
    } else {
        // case 3: R<1
        receive_quote_amount = r_below_sell_base_token(state, pay_base_amount)?;
        new_r = RState::BelowOne;
    }
    Some((receive_quote_amount, new_r))
}

pub fn sell_quote_token(state: &PmmState, pay_quote_amount: U256) -> Option<(U256, RState)> {
    let receive_base_amount;
    let new_r;
    if state.r == RState::One {
        receive_base_amount = r_one_sell_quote_token(state, pay_quote_amount)?;
        new_r = RState::AboveOne;
    } else if state.r == RState::AboveOne {
        receive_base_amount = r_above_sell_quote_token(state, pay_quote_amount)?;
        new_r = RState::AboveOne;
    } else {
        let back_to_one_pay_quote = state.q0.checked_sub(state.q)?;
        let back_to_one_receive_base = state.b.checked_sub(state.b0)?;
        if pay_quote_amount < back_to_one_pay_quote {
            let mut amount = r_below_sell_quote_token(state, pay_quote_amount)?;
            new_r = RState::BelowOne;
            if amount > back_to_one_receive_base {
                amount = back_to_one_receive_base;
            }
            receive_base_amount = amount;
        } else if pay_quote_amount == back_to_one_pay_quote {
            receive_base_amount = back_to_one_receive_base;
            new_r = RState::One;
        } else {
            receive_base_amount = back_to_one_receive_base.checked_add(r_one_sell_quote_token(
                state,
                pay_quote_amount.checked_sub(back_to_one_pay_quote)?,
            )?)?;
            new_r = RState::AboveOne;
        }
    }
    Some((receive_base_amount, new_r))
}

// ============ R = 1 cases ============

pub fn r_one_sell_base_token(state: &PmmState, pay_base_amount: U256) -> Option<U256> {
    dodo_math::solve_quadratic_function_for_trade(
        state.q0,
        state.q0,
        pay_base_amount,
        state.i,
        state.k,
    )
}

pub fn r_one_sell_quote_token(state: &PmmState, pay_quote_amount: U256) -> Option<U256> {
    dodo_math::solve_quadratic_function_for_trade(
        state.b0,
        state.b0,
        pay_quote_amount,
        decimal_math::reciprocal_floor(state.i)?,
        state.k,
    )
}

// ============ R < 1 cases ============

pub fn r_below_sell_quote_token(state: &PmmState, pay_quote_amount: U256) -> Option<U256> {
    dodo_math::general_integrate(
        state.q0,
        state.q.checked_add(pay_quote_amount)?,
        state.q,
        decimal_math::reciprocal_floor(state.i)?,
        state.k,
    )
}

pub fn r_below_sell_base_token(state: &PmmState, pay_base_amount: U256) -> Option<U256> {
    dodo_math::solve_quadratic_function_for_trade(
        state.q0,
        state.q,
        pay_base_amount,
        state.i,
        state.k,
    )
}

// ============ R > 1 cases ============

pub fn r_above_sell_base_token(state: &PmmState, pay_base_amount: U256) -> Option<U256> {
    dodo_math::general_integrate(
        state.b0,
        state.b.checked_add(pay_base_amount)?,
        state.b,
        state.i,
        state.k,
    )
}

pub fn r_above_sell_quote_token(state: &PmmState, pay_quote_amount: U256) -> Option<U256> {
    dodo_math::solve_quadratic_function_for_trade(
        state.b0,
        state.b,
        pay_quote_amount,
        decimal_math::reciprocal_floor(state.i)?,
        state.k,
    )
}

// ============ Helper functions ============

/// `adjustedTarget(state)` — mutates `B0` or `Q0` in place, exactly as the
/// Solidity library mutates its `memory` struct.
pub fn adjusted_target(state: &mut PmmState) -> Option<()> {
    if state.r == RState::BelowOne {
        state.q0 = dodo_math::solve_quadratic_function_for_target(
            state.q,
            state.b.checked_sub(state.b0)?,
            state.i,
            state.k,
        )?;
    } else if state.r == RState::AboveOne {
        state.b0 = dodo_math::solve_quadratic_function_for_target(
            state.b,
            state.q.checked_sub(state.q0)?,
            decimal_math::reciprocal_floor(state.i)?,
            state.k,
        )?;
    }
    Some(())
}

pub fn get_mid_price(state: &PmmState) -> Option<U256> {
    if state.r == RState::BelowOne {
        let r = decimal_math::div_floor(state.q0.checked_mul(state.q0)?.checked_div(state.q)?, state.q)?;
        let r = ONE
            .checked_sub(state.k)?
            .checked_add(decimal_math::mul_floor(state.k, r)?)?;
        decimal_math::div_floor(state.i, r)
    } else {
        let r = decimal_math::div_floor(state.b0.checked_mul(state.b0)?.checked_div(state.b)?, state.b)?;
        let r = ONE
            .checked_sub(state.k)?
            .checked_add(decimal_math::mul_floor(state.k, r)?)?;
        decimal_math::mul_floor(state.i, r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wad(units: u128) -> U256 {
        ONE.checked_mul(U256::from_u128(units)).unwrap()
    }

    fn balanced(k: U256, i: U256) -> PmmState {
        PmmState {
            i,
            k,
            b: wad(100),
            q: wad(200),
            b0: wad(100),
            q0: wad(200),
            r: RState::One,
        }
    }

    #[test]
    fn r_state_round_trips_through_u8() {
        for state in [RState::One, RState::AboveOne, RState::BelowOne] {
            assert_eq!(RState::from_u8(state.as_u8()), Some(state));
        }
        assert_eq!(RState::from_u8(3), None);
    }

    #[test]
    fn balanced_mid_price_equals_the_guide_price() {
        let state = balanced(U256::from_u128(100_000_000_000_000_000), wad(2));
        assert_eq!(get_mid_price(&state).unwrap(), wad(2));
    }

    #[test]
    fn adjusted_target_is_a_no_op_in_state_one() {
        let mut state = balanced(U256::from_u128(100_000_000_000_000_000), wad(2));
        let before = state;
        adjusted_target(&mut state).unwrap();
        assert_eq!(state, before);
    }

    #[test]
    fn sell_base_from_one_moves_below_one_and_sell_quote_moves_above_one() {
        let state = balanced(U256::from_u128(100_000_000_000_000_000), wad(2));
        let (_, new_r) = sell_base_token(&state, wad(1)).unwrap();
        assert_eq!(new_r, RState::BelowOne);
        let (_, new_r) = sell_quote_token(&state, wad(1)).unwrap();
        assert_eq!(new_r, RState::AboveOne);
    }

    #[test]
    fn exact_back_to_one_amount_returns_to_state_one() {
        // ABOVE_ONE: B < B0, Q > Q0. Paying exactly B0-B returns to ONE.
        let state = PmmState {
            i: wad(2),
            k: U256::from_u128(100_000_000_000_000_000),
            b: wad(90),
            q: wad(220),
            b0: wad(100),
            q0: wad(200),
            r: RState::AboveOne,
        };
        let (amount, new_r) = sell_base_token(&state, wad(10)).unwrap();
        assert_eq!(new_r, RState::One);
        assert_eq!(amount, wad(20));
    }
}
