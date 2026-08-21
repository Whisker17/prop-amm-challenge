//! Static probes: equilibrium mid prices and the multi-size quote matrix.
//!
//! Both go through the same adapter path the simulation uses (`BpfAmm` +
//! `compute_swap`), so what is reported here is exactly what the benchmark
//! trades against.

use prop_amm_shared::nano::f64_to_nano;
use prop_amm_sim::amm::BpfAmm;

use crate::dodo::decimal_math::{self, ONE};
use crate::dodo::state::DodoPool;
use crate::dodo::RState;
use crate::flashbots::FlashbotsPool;
use crate::strategies::{Family, Strategy};
use crate::u256::U256;
use crate::wad::{nano_to_wad, price_to_wad};

/// Mid price at a given inventory, as a WAD integer.
///
/// * DODO: `PMMPricing.getMidPrice` after `adjustedTarget` — the contract's own
///   definition.
/// * Flashbots: the marginal price of the curve, `(K / base) / base`, expressed
///   in WAD. At `reserveX == targetX` this is exactly `multX / multY`.
/// * Uniswap V2: `reserveY / reserveX` in WAD.
pub fn mid_price_wad(
    strategy: &Strategy,
    price: f64,
    reserve_x: f64,
    reserve_y: f64,
) -> Option<U256> {
    let price_wad = price_to_wad(price)?;
    let base_wad = nano_to_wad(f64_to_nano(reserve_x));
    let quote_wad = nano_to_wad(f64_to_nano(reserve_y));
    match strategy.family {
        Family::Dodo => {
            let pool = DodoPool {
                i: price_wad,
                k: strategy.dodo_k,
                target_base: base_wad,
                target_quote: quote_wad,
                r_state: RState::One,
                lp_fee_rate: U256::ZERO,
            };
            pool.mid_price(base_wad, quote_wad)
        }
        Family::Flashbots => {
            let pool = FlashbotsPool {
                concentration: strategy.concentration,
                mult_x: price_wad,
                mult_y: ONE,
                target_x: base_wad,
            };
            let k = pool.k()?;
            let base = pool.base(base_wad)?;
            decimal_math::div_floor(k.checked_div(base)?, base)
        }
        Family::UniV2 => decimal_math::div_floor(quote_wad, base_wad),
    }
}

/// One quote-matrix cell.
#[derive(Debug, Clone)]
pub struct QuoteRow {
    pub strategy_id: String,
    pub family: String,
    pub parameter: String,
    /// `buy_x` spends Y to receive X; `sell_x` spends X to receive Y.
    pub side: &'static str,
    /// Input amount in the token being spent.
    pub input: f64,
    /// Output amount in the token being received.
    pub output: f64,
    /// Realised average price of X in Y over the whole order.
    pub average_price: f64,
    /// Signed slippage against the external fair price, in basis points, from
    /// the taker's perspective (negative = the taker paid worse than fair).
    pub slippage_bps: f64,
}

/// Default order sizes: Y notional for buys, X quantity for sells.
pub fn default_buy_sizes_y() -> Vec<f64> {
    vec![0.01, 0.1, 1.0, 10.0, 50.0, 100.0, 500.0, 1_000.0, 2_500.0]
}

pub fn default_sell_sizes_x() -> Vec<f64> {
    vec![0.0001, 0.001, 0.01, 0.1, 0.5, 1.0, 5.0, 10.0, 25.0]
}

fn strategy_amm(strategy: &Strategy, price: f64, reserve_x: f64, reserve_y: f64) -> BpfAmm {
    let mut amm = BpfAmm::new_native(
        strategy.swap_fn(),
        strategy.after_swap_fn(),
        reserve_x,
        reserve_y,
        strategy.id.clone(),
    );
    amm.set_initial_storage(&strategy.initial_storage(price, reserve_x, reserve_y));
    amm
}

/// Quote every strategy at the same inventory over a grid of order sizes.
///
/// Quotes only: no trade is executed, so no strategy state advances.
pub fn quote_matrix(
    strategies: &[Strategy],
    price: f64,
    reserve_x: f64,
    reserve_y: f64,
    buy_sizes_y: &[f64],
    sell_sizes_x: &[f64],
) -> Vec<QuoteRow> {
    let mut rows = Vec::with_capacity(strategies.len() * (buy_sizes_y.len() + sell_sizes_x.len()));
    for strategy in strategies {
        let mut amm = strategy_amm(strategy, price, reserve_x, reserve_y);
        for input_y in buy_sizes_y {
            let output_x = amm.quote_buy_x(*input_y);
            let average_price = if output_x > 0.0 {
                input_y / output_x
            } else {
                f64::NAN
            };
            rows.push(QuoteRow {
                strategy_id: strategy.id.clone(),
                family: strategy.family.as_str().to_string(),
                parameter: strategy.parameter.clone(),
                side: "buy_x",
                input: *input_y,
                output: output_x,
                average_price,
                // The taker pays `average_price` per X; paying more than fair is negative.
                slippage_bps: if average_price.is_finite() && average_price > 0.0 {
                    (price / average_price - 1.0) * 10_000.0
                } else {
                    f64::NAN
                },
            });
        }
        for input_x in sell_sizes_x {
            let output_y = amm.quote_sell_x(*input_x);
            let average_price = if *input_x > 0.0 {
                output_y / input_x
            } else {
                f64::NAN
            };
            rows.push(QuoteRow {
                strategy_id: strategy.id.clone(),
                family: strategy.family.as_str().to_string(),
                parameter: strategy.parameter.clone(),
                side: "sell_x",
                input: *input_x,
                output: output_y,
                average_price,
                // The taker receives `average_price` per X; receiving less than fair is negative.
                slippage_bps: if average_price.is_finite() {
                    (average_price / price - 1.0) * 10_000.0
                } else {
                    f64::NAN
                },
            });
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::{all_strategies, strategy_by_id};

    #[test]
    fn every_strategy_prices_the_equilibrium_point_at_the_oracle_price() {
        let expected = price_to_wad(100.0).unwrap();
        for strategy in all_strategies() {
            let mid = mid_price_wad(&strategy, 100.0, 100.0, 10_000.0)
                .unwrap_or_else(|| panic!("{}: mid price reverted", strategy.id));
            assert_eq!(mid, expected, "{}: mid price", strategy.id);
        }
    }

    #[test]
    fn quote_matrix_outputs_are_positive_and_monotone() {
        let strategies = all_strategies();
        let rows = quote_matrix(
            &strategies,
            100.0,
            100.0,
            10_000.0,
            &default_buy_sizes_y(),
            &default_sell_sizes_x(),
        );
        assert_eq!(
            rows.len(),
            strategies.len() * (default_buy_sizes_y().len() + default_sell_sizes_x().len())
        );

        for strategy in &strategies {
            for side in ["buy_x", "sell_x"] {
                let mut previous_input = f64::NEG_INFINITY;
                let mut previous_output = f64::NEG_INFINITY;
                for row in rows
                    .iter()
                    .filter(|r| r.strategy_id == strategy.id && r.side == side)
                {
                    assert!(
                        row.output > 0.0,
                        "{} {} {}: non-positive output",
                        strategy.id,
                        side,
                        row.input
                    );
                    assert!(row.input > previous_input);
                    assert!(
                        row.output >= previous_output,
                        "{} {}: output fell from {} to {}",
                        strategy.id,
                        side,
                        previous_output,
                        row.output
                    );
                    previous_input = row.input;
                    previous_output = row.output;
                }
            }
        }
    }

    #[test]
    fn takers_never_beat_the_fair_price_at_equilibrium() {
        let strategies = all_strategies();
        let rows = quote_matrix(
            &strategies,
            100.0,
            100.0,
            10_000.0,
            &default_buy_sizes_y(),
            &default_sell_sizes_x(),
        );
        for row in rows {
            assert!(
                row.slippage_bps <= 1e-6,
                "{} {} {}: taker beat fair value by {} bps",
                row.strategy_id,
                row.side,
                row.input,
                row.slippage_bps
            );
        }
    }

    #[test]
    fn concentration_and_k_order_the_curves_as_expected() {
        // Tighter Flashbots concentration and smaller DODO K both mean less
        // slippage for the same order.
        let flat = strategy_by_id("flashbots-c1").unwrap();
        let tight = strategy_by_id("flashbots-c1000").unwrap();
        let rows = quote_matrix(
            &[flat.clone(), tight.clone()],
            100.0,
            100.0,
            10_000.0,
            &[100.0],
            &[1.0],
        );
        let flat_buy = rows
            .iter()
            .find(|r| r.strategy_id == flat.id && r.side == "buy_x")
            .unwrap();
        let tight_buy = rows
            .iter()
            .find(|r| r.strategy_id == tight.id && r.side == "buy_x")
            .unwrap();
        assert!(tight_buy.output > flat_buy.output);

        let k_big = strategy_by_id("dodo-k1000000000000000000").unwrap();
        let k_small = strategy_by_id("dodo-k1000000000000000").unwrap();
        let rows = quote_matrix(
            &[k_big.clone(), k_small.clone()],
            100.0,
            100.0,
            10_000.0,
            &[100.0],
            &[1.0],
        );
        let big = rows
            .iter()
            .find(|r| r.strategy_id == k_big.id && r.side == "buy_x")
            .unwrap();
        let small = rows
            .iter()
            .find(|r| r.strategy_id == k_small.id && r.side == "buy_x")
            .unwrap();
        assert!(small.output > big.output, "smaller K must slip less");
    }
}
