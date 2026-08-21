//! Verbatim Rust port of the Flashbots `ExamplePropAmm` pricing curve.
//!
//! Upstream: flashbots/priority-update-registry `src/ExamplePropAmm.sol`
//! Commit:   da53117870c7bec96d71caebe1b3f94370aba3d6
//! sha256:   5b22ac480c2e2145fc0dbe005361b447f7c8fbc1d56a72671b4cfe6ceb177ca1
//!
//! The two quote functions are reproduced with the same operand order and the
//! same integer truncation points:
//!
//! ```solidity
//! uint256 v0 = pair.targetX * params.concentration;
//! uint256 K = (v0 * v0 * params.multX) / params.multY;
//! uint256 base = v0 + pair.reserveX - pair.targetX;
//!
//! // X -> Y
//! amountOut = K / base - K / (base + amountXIn);
//! // Y -> X
//! amountOut = base - K / (K / base + amountYIn);
//! ```
//!
//! The curve is natively fee-free upstream and is used here without any fee
//! wrapper. `None` marks the cases where the Solidity call would revert
//! (checked-arithmetic overflow/underflow, or division by zero).

use crate::u256::U256;

/// Pricing parameters published through `PrioUpdateRegistry`, plus the pair's
/// `targetX`. `reserveX`/`reserveY` are supplied per call because they are owned
/// by the simulation ledger, exactly as the contract reads its own storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlashbotsPool {
    /// `params.concentration` — a plain integer in `[1, 2000)` upstream.
    pub concentration: U256,
    /// `params.multX` — price multiplier for token X (the oracle-published WAD).
    pub mult_x: U256,
    /// `params.multY` — price multiplier for token Y (`1e18` in this benchmark).
    pub mult_y: U256,
    /// `pair.targetX` — the market maker's deposited X, unchanged by swaps.
    pub target_x: U256,
}

impl FlashbotsPool {
    /// `v0 = targetX * concentration`
    #[inline]
    pub fn v0(&self) -> Option<U256> {
        self.target_x.checked_mul(self.concentration)
    }

    /// `K = (v0 * v0 * multX) / multY`
    #[inline]
    pub fn k(&self) -> Option<U256> {
        let v0 = self.v0()?;
        v0.checked_mul(v0)?
            .checked_mul(self.mult_x)?
            .checked_div(self.mult_y)
    }

    /// `base = v0 + reserveX - targetX`
    #[inline]
    pub fn base(&self, reserve_x: U256) -> Option<U256> {
        self.v0()?
            .checked_add(reserve_x)?
            .checked_sub(self.target_x)
    }

    /// `_quoteXtoY`: `amountOut = K / base - K / (base + amountXIn)`
    pub fn quote_x_to_y(&self, reserve_x: U256, amount_x_in: U256) -> Option<U256> {
        let k = self.k()?;
        let base = self.base(reserve_x)?;
        k.checked_div(base)?
            .checked_sub(k.checked_div(base.checked_add(amount_x_in)?)?)
    }

    /// `_quoteYtoX`: `amountOut = base - K / (K / base + amountYIn)`
    pub fn quote_y_to_x(&self, reserve_x: U256, amount_y_in: U256) -> Option<U256> {
        let k = self.k()?;
        let base = self.base(reserve_x)?;
        base.checked_sub(
            k.checked_div(k.checked_div(base)?.checked_add(amount_y_in)?)?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WAD: u128 = 1_000_000_000_000_000_000;

    fn wad(units: u128) -> U256 {
        U256::from_u128(units * WAD)
    }

    fn pool(concentration: u128, price_units: u128) -> FlashbotsPool {
        FlashbotsPool {
            concentration: U256::from_u128(concentration),
            mult_x: wad(price_units),
            mult_y: U256::from_u128(WAD),
            target_x: wad(100),
        }
    }

    #[test]
    fn concentration_one_at_target_tracks_zero_fee_constant_product() {
        // At reserveX == targetX the virtual quote reserve `K / base` equals
        // targetX * multX / multY (10 000 Y here), so with concentration = 1 the
        // curve coincides with a zero-fee constant product pool at that point.
        // The two expressions truncate in different places, so they may differ
        // by one wei: `K/base - K/(base+in)` floors twice on a 1e42-scaled K,
        // while `ry*in/(rx+in)` floors once.
        let pool = pool(1, 100);
        let out = pool.quote_x_to_y(wad(100), wad(1)).unwrap();
        let cp = U256::from_u128(10_000 * WAD)
            .checked_mul(wad(1))
            .unwrap()
            .checked_div(U256::from_u128(101 * WAD))
            .unwrap();
        let difference = if out >= cp {
            out.checked_sub(cp).unwrap()
        } else {
            cp.checked_sub(out).unwrap()
        };
        assert!(
            difference <= U256::ONE,
            "expected agreement within one wei: flashbots={out}, constant product={cp}"
        );
    }

    #[test]
    fn higher_concentration_gives_less_slippage() {
        let flat = pool(1, 100).quote_x_to_y(wad(100), wad(1)).unwrap();
        let tight = pool(1000, 100).quote_x_to_y(wad(100), wad(1)).unwrap();
        assert!(tight > flat, "concentration should reduce slippage");
        assert!(tight < wad(100), "output stays below the mid-price notional");
    }

    #[test]
    fn zero_input_on_the_y_side_reverts_like_solidity() {
        // `base - K / (K / base + 0)` underflows whenever `K / base` does not
        // divide K exactly, and is zero otherwise; either way the benchmark
        // never quotes a zero amount.
        let pool = pool(7, 100);
        let quote = pool.quote_y_to_x(wad(100), U256::ZERO);
        assert!(quote.is_none() || quote == Some(U256::ZERO));
    }

    #[test]
    fn overflowing_parameters_report_a_revert() {
        let pool = FlashbotsPool {
            concentration: U256::from_u128(1999),
            mult_x: U256::MAX,
            mult_y: U256::from_u128(WAD),
            target_x: wad(100),
        };
        assert_eq!(pool.k(), None);
        assert_eq!(pool.quote_x_to_y(wad(100), wad(1)), None);
    }
}
