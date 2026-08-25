//! DODO PMM query pipeline and state persistence.
//!
//! [`DodoQueryInput::query_with_fee`] mirrors `test/harness/DodoPmmHarness.sol`
//! (`_query` + `queryWithFee`) from the read-only `mantle-propamm-contracts`
//! repository, i.e. the exact pipeline the 19 golden vectors were produced with:
//!
//! ```text
//! adjustedTarget(state) -> record B0/Q0 -> sellBaseToken | sellQuoteToken
//!   -> lpFeeAmount = mulFloor(gross, lpFeeRate) -> amountOut = gross - fee
//! ```
//!
//! [`DodoPool`] adds the persistence rule from
//! `src/MantlePropAmmPool.sol::swap` / `_evaluateQuote`: reserves always move,
//! but the target is written back **only when `RState` changes**, and then only
//! on the side matching the trade direction (`sellBase -> targetBase`,
//! `sellQuote -> targetQuote`).

use super::decimal_math;
use super::pmm_pricing::{self, PmmState, RState};
use crate::u256::U256;

/// One PMM query, in the same field order as `DodoPmmHarness.QueryInput`.
#[derive(Debug, Clone, Copy)]
pub struct DodoQueryInput {
    pub i: U256,
    pub k: U256,
    pub b: U256,
    pub q: U256,
    pub b0: U256,
    pub q0: U256,
    pub r_state: RState,
    pub sell_base: bool,
    pub amount_in: U256,
    pub lp_fee_rate: U256,
}

/// One PMM result, in the same field order as `DodoPmmHarness.FeeQueryResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DodoQuery {
    pub gross_amount_out: U256,
    pub lp_fee_amount: U256,
    pub amount_out: U256,
    pub new_r: RState,
    pub adjusted_b0: U256,
    pub adjusted_q0: U256,
}

impl DodoQueryInput {
    fn state(&self) -> PmmState {
        PmmState {
            i: self.i,
            k: self.k,
            b: self.b,
            q: self.q,
            b0: self.b0,
            q0: self.q0,
            r: self.r_state,
        }
    }

    /// `DodoPmmHarness._query`
    pub fn query(&self) -> Option<(U256, RState, U256, U256)> {
        let mut state = self.state();
        pmm_pricing::adjusted_target(&mut state)?;
        let adjusted_b0 = state.b0;
        let adjusted_q0 = state.q0;
        let (amount_out, new_r) = if self.sell_base {
            pmm_pricing::sell_base_token(&state, self.amount_in)?
        } else {
            pmm_pricing::sell_quote_token(&state, self.amount_in)?
        };
        Some((amount_out, new_r, adjusted_b0, adjusted_q0))
    }

    /// `DodoPmmHarness.queryWithFee`
    pub fn query_with_fee(&self) -> Option<DodoQuery> {
        let (gross_amount_out, new_r, adjusted_b0, adjusted_q0) = self.query()?;
        let lp_fee_amount = decimal_math::mul_floor(gross_amount_out, self.lp_fee_rate)?;
        let amount_out = gross_amount_out.checked_sub(lp_fee_amount)?;
        Some(DodoQuery {
            gross_amount_out,
            lp_fee_amount,
            amount_out,
            new_r,
            adjusted_b0,
            adjusted_q0,
        })
    }

    /// `DodoPmmHarness.midPrice` (adjusted target first, then `getMidPrice`).
    pub fn mid_price(&self) -> Option<U256> {
        let mut state = self.state();
        pmm_pricing::adjusted_target(&mut state)?;
        pmm_pricing::get_mid_price(&state)
    }

    /// `DodoPmmHarness.adjustedTarget`
    pub fn adjusted_target(&self) -> Option<(U256, U256)> {
        let mut state = self.state();
        pmm_pricing::adjusted_target(&mut state)?;
        Some((state.b0, state.q0))
    }
}

/// A DODO PMM pool with persistent target/`RState`, as held on chain.
///
/// `b`/`q` are *not* stored here: in this benchmark the reserves are owned by
/// the simulation ledger and supplied per call, exactly as the on-chain pool
/// reads its own `_reserveBase`/`_reserveQuote` at quote time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DodoPool {
    pub i: U256,
    pub k: U256,
    pub target_base: U256,
    pub target_quote: U256,
    pub r_state: RState,
    pub lp_fee_rate: U256,
}

impl DodoPool {
    pub fn input(&self, b: U256, q: U256, sell_base: bool, amount_in: U256) -> DodoQueryInput {
        DodoQueryInput {
            i: self.i,
            k: self.k,
            b,
            q,
            b0: self.target_base,
            q0: self.target_quote,
            r_state: self.r_state,
            sell_base,
            amount_in,
            lp_fee_rate: self.lp_fee_rate,
        }
    }

    /// Quote without touching state.
    pub fn quote(&self, b: U256, q: U256, sell_base: bool, amount_in: U256) -> Option<DodoQuery> {
        self.input(b, q, sell_base, amount_in).query_with_fee()
    }

    /// Persist the post-trade target/`RState`.
    ///
    /// `MantlePropAmmPool.swap`:
    /// ```solidity
    /// if (result.rStateAfter != oldR) {
    ///     if (sellBase) { _targetBase = result.targetBaseAfter; }
    ///     else { _targetQuote = result.targetQuoteAfter; }
    ///     _rState = result.rStateAfter;
    /// }
    /// ```
    /// where `targetBaseAfter`/`targetQuoteAfter` are the *adjusted* targets.
    pub fn commit(&mut self, sell_base: bool, result: &DodoQuery) {
        if result.new_r != self.r_state {
            if sell_base {
                self.target_base = result.adjusted_b0;
            } else {
                self.target_quote = result.adjusted_q0;
            }
            self.r_state = result.new_r;
        }
    }

    pub fn mid_price(&self, b: U256, q: U256) -> Option<U256> {
        self.input(b, q, true, U256::ZERO).mid_price()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dodo::decimal_math::ONE;

    fn wad(units: u128) -> U256 {
        ONE.checked_mul(U256::from_u128(units)).unwrap()
    }

    fn pool() -> DodoPool {
        DodoPool {
            i: wad(100),
            k: ONE,
            target_base: wad(100),
            target_quote: wad(10_000),
            r_state: RState::One,
            lp_fee_rate: U256::ZERO,
        }
    }

    #[test]
    fn commit_is_a_no_op_when_the_r_state_is_unchanged() {
        let mut pool = pool();
        pool.r_state = RState::BelowOne;
        pool.target_base = wad(90);
        let before = pool;
        let result = DodoQuery {
            gross_amount_out: wad(1),
            lp_fee_amount: U256::ZERO,
            amount_out: wad(1),
            new_r: RState::BelowOne,
            adjusted_b0: wad(1234),
            adjusted_q0: wad(4321),
        };
        pool.commit(true, &result);
        assert_eq!(
            pool, before,
            "targets must not move without an R transition"
        );
    }

    #[test]
    fn commit_writes_only_the_direction_matching_target() {
        let mut pool = pool();
        let result = DodoQuery {
            gross_amount_out: wad(1),
            lp_fee_amount: U256::ZERO,
            amount_out: wad(1),
            new_r: RState::BelowOne,
            adjusted_b0: wad(1234),
            adjusted_q0: wad(4321),
        };
        pool.commit(true, &result);
        assert_eq!(pool.target_base, wad(1234), "sellBase writes targetBase");
        assert_eq!(
            pool.target_quote,
            wad(10_000),
            "sellBase leaves targetQuote"
        );
        assert_eq!(pool.r_state, RState::BelowOne);

        let mut pool = self::pool();
        pool.commit(false, &result);
        assert_eq!(pool.target_base, wad(100), "sellQuote leaves targetBase");
        assert_eq!(pool.target_quote, wad(4321), "sellQuote writes targetQuote");
    }

    #[test]
    fn zero_fee_leaves_the_gross_output_untouched() {
        let pool = pool();
        let result = pool
            .quote(wad(100), wad(10_000), true, wad(1))
            .expect("quote");
        assert_eq!(result.lp_fee_amount, U256::ZERO);
        assert_eq!(result.amount_out, result.gross_amount_out);
    }

    #[test]
    fn balanced_mid_price_equals_the_published_guide_price() {
        let pool = pool();
        assert_eq!(pool.mid_price(wad(100), wad(10_000)).unwrap(), wad(100));
    }
}
