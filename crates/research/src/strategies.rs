//! The strategy catalogue under comparison, and the parameter pairing table.
//!
//! Every strategy is fee-free and starts from the same capital:
//! `reserveX = 100`, `reserveY = 10 000`, mid price `100`.
//!
//! * DODO PMM: `B = B0 = 100`, `Q = Q0 = 10 000`, `R = ONE`, `lpFeeRate = 0`.
//! * Flashbots prop AMM: `reserveX = targetX = 100`, `reserveY = 10 000`, no fee.
//! * Uniswap V2: `reserveX = 100`, `reserveY = 10 000`, fee `0`.
//!
//! ## Parameter pairing
//!
//! | DODO `K` | Flashbots `concentration` |
//! | --- | --- |
//! | `1e18` | 1 |
//! | `0.5e18` | 2 |
//! | `0.2e18` | 5 |
//! | `0.1e18` | 10 |
//! | `0.05e18` | 20 |
//! | `0.02e18` | 50 |
//! | `0.01e18` | 100 |
//! | `0.001e18` | 1000 |
//!
//! `K ≈ 1 / concentration` matches only the *local* curvature near the
//! equilibrium point. It is not a global equivalence between the two curves, it
//! is not used to modify or substitute any formula, and no claim of curve
//! equivalence is made anywhere in this crate.

use prop_amm_executor::{AfterSwapFn, SwapFn};
use prop_amm_shared::nano::f64_to_nano;

use crate::curves;
use crate::dodo::decimal_math::ONE;
use crate::u256::U256;
use crate::univ3;
use crate::univ3_curve;
use crate::wad::{nano_to_wad, price_to_wad};

/// Which ported curve a strategy uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// DODO V2 PMM (`i` is the oracle-published guide price).
    Dodo,
    /// Flashbots `ExamplePropAmm` (`multX` is the oracle-published price).
    Flashbots,
    /// Uniswap V2 with zero fee (no oracle input by construction).
    UniV2,
    /// Uniswap V3, zero fee, one full-range position. The correctness and sanity
    /// baseline: at zero fee this is constant product on the same capital.
    UniV3FullRange,
    /// Uniswap V3, zero fee, one static concentrated position. A separate
    /// sensitivity arm, never the main baseline: the position is minted once and
    /// **never rebalanced**, so the price can and does leave its range.
    UniV3Concentrated,
}

impl Family {
    pub fn as_str(self) -> &'static str {
        match self {
            Family::Dodo => "dodo",
            Family::Flashbots => "flashbots",
            Family::UniV2 => "univ2",
            Family::UniV3FullRange => "univ3-full-range",
            Family::UniV3Concentrated => "univ3-concentrated",
        }
    }

    /// Whether the curve consumes an oracle price at all.
    pub fn is_oracle_aware(self) -> bool {
        match self {
            Family::Dodo | Family::Flashbots => true,
            Family::UniV2 | Family::UniV3FullRange | Family::UniV3Concentrated => false,
        }
    }

    /// Whether the curve is one of the Uniswap V3 arms, which alone can refuse
    /// an order for lack of room (see [`crate::univ3_curve`]).
    pub fn is_univ3(self) -> bool {
        matches!(self, Family::UniV3FullRange | Family::UniV3Concentrated)
    }
}

/// Which catalogue to run.
///
/// The V3 arms are opt-in so that a run can be compared against the phase-1 and
/// phase-2 results without the strategy list silently changing underneath it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategySet {
    /// DODO, Flashbots and the zero-fee Uniswap V2 anchor.
    Legacy,
    /// Everything in [`StrategySet::Legacy`], plus the two Uniswap V3 arms.
    WithV3,
}

impl StrategySet {
    pub fn as_str(self) -> &'static str {
        match self {
            StrategySet::Legacy => "legacy",
            StrategySet::WithV3 => "with-v3",
        }
    }

    pub fn parse(text: &str) -> Option<StrategySet> {
        match text {
            "legacy" => Some(StrategySet::Legacy),
            "with-v3" | "with_v3" | "withv3" => Some(StrategySet::WithV3),
            _ => None,
        }
    }
}

/// One comparable configuration.
#[derive(Debug, Clone)]
pub struct Strategy {
    pub id: String,
    pub family: Family,
    /// Human-readable parameter, e.g. `K=1e18` or `concentration=10`.
    pub parameter: String,
    /// Index into [`pairing_table`], or `None` for parameter-free strategies.
    pub pairing_index: Option<usize>,
    /// DODO curve parameter `K` (zero for other families).
    pub dodo_k: U256,
    /// Flashbots `concentration` (zero for other families).
    pub concentration: U256,
    /// Uniswap V3 position, for the V3 families only.
    pub univ3: Option<UniV3Position>,
}

/// The V3 position a strategy mints once, at the opening price, and never
/// rebalances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UniV3Position {
    /// `None` for the full-range arm.
    pub half_width_ticks: Option<i32>,
    pub liquidity: u128,
    /// Achieved marginal-impact factor relative to full range, as an exact
    /// ratio. `1/1` for the full-range arm.
    pub factor_num: U256,
    pub factor_den: U256,
}

impl UniV3Position {
    /// Achieved factor rendered to six places, for reports and ids.
    pub fn factor_string(&self) -> String {
        univ3::ConcentratedChoice {
            half_width_ticks: self.half_width_ticks.unwrap_or(0),
            liquidity: self.liquidity,
            factor_num: self.factor_num,
            factor_den: self.factor_den,
        }
        .factor_string(6)
    }
}

impl Strategy {
    pub fn swap_fn(&self) -> SwapFn {
        match self.family {
            Family::Dodo => curves::dodo_compute_swap,
            Family::Flashbots => curves::flashbots_compute_swap,
            Family::UniV2 => curves::univ2_compute_swap,
            Family::UniV3FullRange | Family::UniV3Concentrated => univ3_curve::univ3_compute_swap,
        }
    }

    pub fn after_swap_fn(&self) -> Option<AfterSwapFn> {
        match self.family {
            // DODO carries persistent state (target / RState); V3's pool state
            // (sqrtPrice, tick, liquidity) is authoritative for pricing and is
            // committed the same way.
            Family::Dodo => Some(curves::dodo_after_swap),
            Family::UniV3FullRange | Family::UniV3Concentrated => {
                Some(univ3_curve::univ3_after_swap)
            }
            Family::Flashbots | Family::UniV2 => None,
        }
    }

    /// Storage blob for the unified initial state.
    ///
    /// `initial_x` / `initial_y` are quantised exactly the way the simulation
    /// quantises reserves, so the curve's own view of the opening inventory
    /// matches the ledger to the last nano.
    pub fn initial_storage(&self, initial_price: f64, initial_x: f64, initial_y: f64) -> Vec<u8> {
        let price_wad = price_to_wad(initial_price).unwrap_or(U256::ZERO);
        let base_wad = nano_to_wad(f64_to_nano(initial_x));
        let quote_wad = nano_to_wad(f64_to_nano(initial_y));
        match self.family {
            Family::Dodo => curves::dodo_initial_storage(
                price_wad,
                self.dodo_k,
                base_wad,
                quote_wad,
                U256::ZERO, // lpFeeRate is pinned to zero for this benchmark
            ),
            Family::Flashbots => curves::flashbots_initial_storage(
                price_wad,
                ONE, // multY
                self.concentration,
                base_wad, // targetX == the deposited X
            ),
            Family::UniV2 => vec![0u8; prop_amm_shared::instruction::STORAGE_SIZE],
            // The V3 pool is built from its own integer state, not from the
            // ledger's floats: `sqrtPriceX96` is exact at the opening price and
            // the liquidity was chosen to hold exactly this capital. The two
            // agree by construction, and `univ3::tests` checks that against the
            // pinned `getAmount*Delta` rather than against the choice itself.
            Family::UniV3FullRange | Family::UniV3Concentrated => {
                let position = self
                    .univ3
                    .expect("a Uniswap V3 strategy must carry its position");
                let (config, state) = match position.half_width_ticks {
                    None => univ3::full_range_pool(position.liquidity),
                    Some(half_width) => univ3::concentrated_pool(half_width, position.liquidity),
                };
                univ3_curve::initial_storage(&config, &state)
            }
        }
    }
}

/// `(DODO K, Flashbots concentration)` pairs, in the order given by the study.
pub fn pairing_table() -> Vec<(U256, u64)> {
    vec![
        (U256::from_u128(1_000_000_000_000_000_000), 1),
        (U256::from_u128(500_000_000_000_000_000), 2),
        (U256::from_u128(200_000_000_000_000_000), 5),
        (U256::from_u128(100_000_000_000_000_000), 10),
        (U256::from_u128(50_000_000_000_000_000), 20),
        (U256::from_u128(20_000_000_000_000_000), 50),
        (U256::from_u128(10_000_000_000_000_000), 100),
        (U256::from_u128(1_000_000_000_000_000), 1000),
    ]
}

fn k_label(k: U256) -> String {
    // Render K as a multiple of 1e18 without floating point.
    let scaled = k.to_string();
    format!("K={scaled}")
}

/// Every strategy in the comparison: one DODO and one Flashbots entry per
/// pairing row, plus the parameter-free zero-fee Uniswap V2 baseline.
pub fn all_strategies() -> Vec<Strategy> {
    strategies_for(StrategySet::Legacy)
}

/// The catalogue for a given set.
///
/// [`StrategySet::WithV3`] appends, in this order:
///
/// * `univ3-full-range-zero-fee` — one full-range position, the sanity baseline;
/// * `univ3-conc-c{n}` — one static concentrated position per pairing row, whose
///   range was chosen so that its **marginal** price impact at the opening point
///   matches the paired Flashbots `concentration`, holding deposited capital
///   fixed (see [`univ3::concentrated_for_impact_factor`]). The achieved factor
///   is not exactly `n`, because a tick is 1 bp wide; the residual is carried in
///   the strategy's `parameter` string rather than rounded away.
pub fn strategies_for(set: StrategySet) -> Vec<Strategy> {
    let mut strategies = Vec::new();
    for (index, (k, concentration)) in pairing_table().into_iter().enumerate() {
        strategies.push(Strategy {
            id: format!("dodo-k{k}"),
            family: Family::Dodo,
            parameter: k_label(k),
            pairing_index: Some(index),
            dodo_k: k,
            concentration: U256::ZERO,
            univ3: None,
        });
        strategies.push(Strategy {
            id: format!("flashbots-c{concentration}"),
            family: Family::Flashbots,
            parameter: format!("concentration={concentration}"),
            pairing_index: Some(index),
            dodo_k: U256::ZERO,
            concentration: U256::from_u64(concentration),
            univ3: None,
        });
    }
    strategies.push(Strategy {
        id: "univ2-zero-fee".to_string(),
        family: Family::UniV2,
        parameter: "fee=0".to_string(),
        pairing_index: None,
        dodo_k: U256::ZERO,
        concentration: U256::ZERO,
        univ3: None,
    });

    if set == StrategySet::WithV3 {
        let full_range_coefficient = univ3::full_range_coefficient();
        strategies.push(Strategy {
            id: "univ3-full-range-zero-fee".to_string(),
            family: Family::UniV3FullRange,
            parameter: "fee=0, range=full, marginalImpactFactor=1".to_string(),
            // Row 0 of the pairing table is `concentration = 1`, i.e. no
            // concentration at all, which is precisely what a full range is.
            // This arm therefore *is* that row; a separate `univ3-conc-c1` would
            // be the same pool measured twice under two names.
            pairing_index: Some(0),
            dodo_k: U256::ZERO,
            concentration: U256::ONE,
            univ3: Some(UniV3Position {
                half_width_ticks: None,
                liquidity: univ3::FULL_RANGE_LIQUIDITY,
                factor_num: full_range_coefficient,
                factor_den: full_range_coefficient,
            }),
        });

        for (index, (_, concentration)) in pairing_table().into_iter().enumerate() {
            if concentration <= 1 {
                // Served by the full-range arm above.
                continue;
            }
            let Some(choice) =
                univ3::concentrated_for_impact_factor(concentration, univ3::FULL_RANGE_LIQUIDITY)
            else {
                // No representable range reaches this factor. Skipping is a
                // silent gap, so it is announced instead.
                eprintln!(
                    "warning: no Uniswap V3 range matches marginal impact x{concentration}; \
                     that arm is absent from this run"
                );
                continue;
            };
            let position = UniV3Position {
                half_width_ticks: Some(choice.half_width_ticks),
                liquidity: choice.liquidity,
                factor_num: choice.factor_num,
                factor_den: choice.factor_den,
            };
            strategies.push(Strategy {
                id: format!("univ3-conc-c{concentration}"),
                family: Family::UniV3Concentrated,
                parameter: format!(
                    "fee=0, halfWidth={} ticks, marginalImpactFactor={}",
                    choice.half_width_ticks,
                    position.factor_string()
                ),
                pairing_index: Some(index),
                dodo_k: U256::ZERO,
                concentration: U256::from_u64(concentration),
                univ3: Some(position),
            });
        }
    }

    strategies
}

/// Look up a strategy by id, across every set.
pub fn strategy_by_id(id: &str) -> Option<Strategy> {
    strategies_for(StrategySet::WithV3)
        .into_iter()
        .find(|s| s.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_table_matches_the_study_table() {
        let table = pairing_table();
        assert_eq!(table.len(), 8);
        let concentrations: Vec<u64> = table.iter().map(|(_, c)| *c).collect();
        assert_eq!(concentrations, vec![1, 2, 5, 10, 20, 50, 100, 1000]);
        let expected_k = [
            "1000000000000000000",
            "500000000000000000",
            "200000000000000000",
            "100000000000000000",
            "50000000000000000",
            "20000000000000000",
            "10000000000000000",
            "1000000000000000",
        ];
        for (row, expected) in table.iter().zip(expected_k) {
            assert_eq!(row.0.to_string(), expected);
        }
    }

    #[test]
    fn catalogue_has_one_dodo_and_one_flashbots_entry_per_row_plus_univ2() {
        let strategies = all_strategies();
        assert_eq!(strategies.len(), 8 * 2 + 1);
        assert_eq!(
            strategies
                .iter()
                .filter(|s| s.family == Family::Dodo)
                .count(),
            8
        );
        assert_eq!(
            strategies
                .iter()
                .filter(|s| s.family == Family::Flashbots)
                .count(),
            8
        );
        assert_eq!(
            strategies
                .iter()
                .filter(|s| s.family == Family::UniV2)
                .count(),
            1
        );
        for strategy in &strategies {
            assert!(
                strategy_by_id(&strategy.id).is_some(),
                "{} lookup",
                strategy.id
            );
        }
    }

    #[test]
    fn only_dodo_registers_an_after_swap_hook() {
        for strategy in all_strategies() {
            let has_hook = strategy.after_swap_fn().is_some();
            assert_eq!(has_hook, strategy.family == Family::Dodo, "{}", strategy.id);
        }
    }

    #[test]
    fn initial_storage_encodes_the_unified_opening_state() {
        let dodo = strategy_by_id("dodo-k1000000000000000000").unwrap();
        let storage = dodo.initial_storage(100.0, 100.0, 10_000.0);
        assert_eq!(
            U256::from_le_bytes(&storage[curves::dodo_storage::I..curves::dodo_storage::I + 32]),
            U256::from_u128(100_000_000_000_000_000_000)
        );
        assert_eq!(
            U256::from_le_bytes(
                &storage
                    [curves::dodo_storage::TARGET_QUOTE..curves::dodo_storage::TARGET_QUOTE + 32]
            ),
            U256::from_u128(10_000).checked_mul(ONE).unwrap()
        );
        assert_eq!(storage[curves::dodo_storage::R_STATE], 0, "R starts at ONE");
        assert_eq!(
            U256::from_le_bytes(
                &storage[curves::dodo_storage::LP_FEE_RATE..curves::dodo_storage::LP_FEE_RATE + 32]
            ),
            U256::ZERO,
            "lpFeeRate is pinned to zero"
        );

        let flashbots = strategy_by_id("flashbots-c1").unwrap();
        let storage = flashbots.initial_storage(100.0, 100.0, 10_000.0);
        assert_eq!(
            U256::from_le_bytes(
                &storage[curves::flashbots_storage::MULT_Y..curves::flashbots_storage::MULT_Y + 32]
            ),
            ONE,
            "multY is pinned to 1e18"
        );
        assert_eq!(
            U256::from_le_bytes(
                &storage
                    [curves::flashbots_storage::TARGET_X..curves::flashbots_storage::TARGET_X + 32]
            ),
            U256::from_u128(100).checked_mul(ONE).unwrap()
        );
    }
}
