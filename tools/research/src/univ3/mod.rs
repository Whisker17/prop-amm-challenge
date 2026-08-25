//! Uniswap V3, ported from the pinned official sources.
//!
//! Upstream repository: <https://github.com/Uniswap/v3-core>
//! Pinned commit: `e3589b192d0be27e100cd0daaf6c97204fdb1899` (tag `v1.0.0`)
//! Compiles upstream at Solidity **0.7.6**, where arithmetic wraps unless a
//! `LowGasSafeMath` / `SafeCast` helper is used. That distinction is load-bearing
//! and is preserved call site by call site.
//!
//! | Upstream file | sha256 | Ported to |
//! | --- | --- | --- |
//! | `contracts/libraries/TickMath.sol` | `83cf64b2ca84001effd16e007b49bac5359143b6c3132bfe42907b2426a0c5f5` | [`tick_math`] |
//! | `contracts/libraries/SqrtPriceMath.sol` | `ddd62e3a94346248677f30f1ab009ef015e71e4b8696dcca890eeabc9dc6c149` | [`sqrt_price_math`] |
//! | `contracts/libraries/SwapMath.sol` | `d6cb9a153be4ea9fb2377ef88641ef7979b5cee6933162f1b732d0289e26e1b6` | [`swap_math`] |
//! | `contracts/libraries/LiquidityMath.sol` | `84d20a16d5346f6ec4c12dff4df23dda5d46e52d33f18aaaaac2e9e36ce4a072` | [`swap_math::add_delta`] |
//! | `contracts/libraries/TickBitmap.sol` | `bd7a17c5134f0718eb7d856ddfc58d8347d32a8f661bed53aa3ad17c9aea09ba` | [`tick_list`] |
//! | `contracts/libraries/SafeCast.sol` | `9aed494b56d3dd16b7d6535583ded2cdfb03dc80aaa919347b13d35fd597e8bf` | [`signed`] |
//! | `contracts/libraries/UnsafeMath.sol` | `4d02353eb503e3111e25bd50104ac9b279f99e88d848e455262a3fbeb55c50e7` | [`sqrt_price_math::div_rounding_up`] |
//! | `contracts/UniswapV3Pool.sol` | `d515775b7f3ffe921dd70aca86b8bad16280fa4c122425d82b4dbea4dc564a7a` | [`pool`] |
//!
//! `contracts/libraries/FullMath.sol`
//! (`54087aee268a6938a85a408d7b14481b5c2c956c21508d5583f1bf48ec6d69ba`) is ported
//! into [`crate::u256`] instead, because `mulDiv` is a general 512-bit primitive
//! rather than a Uniswap-specific one, and because the other curves may want it.
//!
//! Licence note: the pool and factory carry BUSL-1.1 SPDX headers and the maths
//! libraries carry GPL-2.0-or-later. No upstream source is vendored into this
//! repository; the pinned tree is only read from a temporary checkout to produce
//! golden vectors, and the sha256 values above are what tie this port to it.

pub mod bits;
pub mod pool;
pub mod signed;
pub mod sqrt_price_math;
pub mod swap_math;
pub mod tick_list;
pub mod tick_math;

// `FullMath.mulDiv` / `mulDivRoundingUp` live in the shared integer module.
pub(crate) use crate::u256::{mul_div, mul_div_rounding_up};

pub use pool::{no_limit, PoolConfig, PoolState, SwapOutcome};
pub use signed::I256;
pub use tick_list::{TickInfo, TickList};

use crate::u256::U256;

/// The benchmark's opening sqrt price: `10 * 2^96`.
///
/// This is exact — `sqrt(100) = 10` is an integer, so the Q64.96 representation
/// of price 100 loses nothing. The pool is initialised at this value directly
/// rather than at the tick-aligned price of tick 46054, which would start the
/// V3 pool 0.0044 bps away from every other curve for no reason.
pub const OPENING_SQRT_PRICE_X96: U256 = U256::from_u128(792_281_625_142_643_375_935_439_503_360);

/// The tick the pool derives from [`OPENING_SQRT_PRICE_X96`].
pub const OPENING_TICK: i32 = 46_054;

/// Liquidity for a full-range position holding 100 token0 and 10 000 token1 at
/// price 100: `L = sqrt(100e18 * 10000e18) = 10^21`.
pub const FULL_RANGE_LIQUIDITY: u128 = 1_000_000_000_000_000_000_000;

/// Build the benchmark's full-range configuration and opening state.
pub fn full_range_pool(liquidity: u128) -> (PoolConfig, PoolState) {
    (
        PoolConfig {
            tick_spacing: 1,
            fee_pips: 0,
            ticks: TickList::new([
                (tick_math::MIN_TICK, liquidity as i128),
                (tick_math::MAX_TICK, -(liquidity as i128)),
            ]),
        },
        PoolState {
            sqrt_price_x96: OPENING_SQRT_PRICE_X96,
            tick: OPENING_TICK,
            liquidity,
        },
    )
}

/// Build a symmetric concentrated configuration around the opening tick.
///
/// The position is static: it is minted once and never rebalanced, which is the
/// point of the sensitivity experiment. Once the price leaves `[lower, upper]`
/// the pool holds a single asset and quotes zero on one side.
pub fn concentrated_pool(half_width_ticks: i32, liquidity: u128) -> (PoolConfig, PoolState) {
    let lower = OPENING_TICK - half_width_ticks;
    let upper = OPENING_TICK + half_width_ticks;
    (
        PoolConfig {
            tick_spacing: 1,
            fee_pips: 0,
            ticks: TickList::new([(lower, liquidity as i128), (upper, -(liquidity as i128))]),
        },
        PoolState {
            sqrt_price_x96: OPENING_SQRT_PRICE_X96,
            tick: OPENING_TICK,
            liquidity,
        },
    )
}

/// Deposited value per unit of liquidity, denominated in token1 and scaled by
/// `2^96`, for a position spanning `[sqrt_lower, sqrt_upper]` at `sqrt_price`.
///
/// Derived from the pinned `getAmount0Delta` / `getAmount1Delta`:
///
/// ```text
/// amount1        = L * (s - s_l) / 2^96
/// amount0        = L * 2^96 * (s_u - s) / (s * s_u)
/// amount0 * P    = L * (s - s^2 / s_u) / 2^96        with P = s^2 / 2^192
/// value          = L * (2s - s_l - s^2/s_u) / 2^96
/// ```
///
/// so this returns `2s - s_l - s^2/s_u`. Integer throughout; `s^2/s_u` floors,
/// the same way the pool's own `mulDiv` does, and the same rounding is applied
/// to every range so the comparison between them stays exact.
pub fn capital_coefficient(sqrt_lower: U256, sqrt_price: U256, sqrt_upper: U256) -> Option<U256> {
    if sqrt_lower >= sqrt_price || sqrt_upper <= sqrt_price {
        return None;
    }
    let quote_leg = sqrt_price.checked_sub(sqrt_lower)?;
    let base_leg = sqrt_price.checked_sub(mul_div(sqrt_price, sqrt_price, sqrt_upper)?)?;
    quote_leg.checked_add(base_leg)
}

/// The full-range position's capital coefficient, at the pinned domain bounds.
///
/// `MIN_SQRT_RATIO` and `MAX_SQRT_RATIO` are finite, so this is very slightly
/// below the ideal `2s`; using the real bound rather than the ideal keeps the
/// concentrated ratios below honest instead of flattering.
pub fn full_range_coefficient() -> U256 {
    capital_coefficient(
        tick_math::MIN_SQRT_RATIO,
        OPENING_SQRT_PRICE_X96,
        tick_math::MAX_SQRT_RATIO,
    )
    .expect("the opening price is strictly inside the tick domain")
}

/// A concentrated position chosen to match a target **marginal** price impact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConcentratedChoice {
    /// Ticks either side of [`OPENING_TICK`].
    pub half_width_ticks: i32,
    /// Liquidity that holds the same deposited capital as the full-range pool.
    pub liquidity: u128,
    /// Achieved impact factor as an exact ratio `num / den`. Equal to
    /// `L_concentrated / L_full_range`, which is exactly the factor by which the
    /// marginal price impact is reduced.
    pub factor_num: U256,
    pub factor_den: U256,
}

impl ConcentratedChoice {
    /// The achieved factor, rendered with `decimals` places by integer division.
    pub fn factor_string(&self, decimals: u32) -> String {
        let scale = U256::from_u128(10u128.pow(decimals));
        let scaled = mul_div(self.factor_num, scale, self.factor_den).unwrap_or(U256::ZERO);
        let text = scaled.to_string();
        let width = decimals as usize;
        if text.len() <= width {
            format!("0.{:0>width$}", text, width = width)
        } else {
            let split = text.len() - width;
            format!("{}.{}", &text[..split], &text[split..])
        }
    }
}

/// Pick the symmetric tick range whose **marginal** price impact at the opening
/// point is closest to `1 / target` of the full-range pool's, holding deposited
/// capital fixed.
///
/// Why marginal rather than the impact of some named order size: an order-sized
/// match depends on the size chosen, and would silently encode a view about
/// which trade matters. The marginal impact is a property of the curve at the
/// point where every strategy starts, so it is the one quantity the DODO `K`,
/// the Flashbots `concentration` and a V3 range can all be read off at.
///
/// Locally every one of these curves behaves like constant product on its
/// virtual reserves, where `d(ln P)/d(amount0) = -2 * s / L`. Impact is therefore
/// inversely proportional to `L`, and matching impact means matching `L` for
/// equal capital — which is what this searches for. `target == 1` returns the
/// full-range position unchanged.
///
/// Monotone in the half width (a narrower range concentrates the same capital
/// into more liquidity), so a binary search finds the boundary and the two
/// neighbours are compared exactly, by cross-multiplication, to pick the nearer.
pub fn concentrated_for_impact_factor(
    target: u64,
    full_liquidity: u128,
) -> Option<ConcentratedChoice> {
    let coef_full = full_range_coefficient();
    if target <= 1 {
        return Some(ConcentratedChoice {
            half_width_ticks: tick_math::MAX_TICK - OPENING_TICK,
            liquidity: full_liquidity,
            factor_num: coef_full,
            factor_den: coef_full,
        });
    }
    let target_u = U256::from_u64(target);

    let coef_at = |half_width: i32| -> Option<U256> {
        let lower = tick_math::get_sqrt_ratio_at_tick(OPENING_TICK.checked_sub(half_width)?)?;
        let upper = tick_math::get_sqrt_ratio_at_tick(OPENING_TICK.checked_add(half_width)?)?;
        capital_coefficient(lower, OPENING_SQRT_PRICE_X96, upper)
    };

    // factor(w) = coef_full / coef(w) is decreasing in w. Find the largest w
    // whose factor is still at least `target`.
    let max_half_width =
        (tick_math::MAX_TICK - OPENING_TICK).min(OPENING_TICK - tick_math::MIN_TICK);
    let (mut low, mut high) = (1_i32, max_half_width);
    while low < high {
        let mid = low + (high - low + 1) / 2;
        let reaches_target = match coef_at(mid) {
            // coef_full / coef_mid >= target  <=>  coef_full >= target * coef_mid
            Some(coef) => coef
                .checked_mul(target_u)
                .is_some_and(|scaled| coef_full >= scaled),
            None => false,
        };
        if reaches_target {
            low = mid;
        } else {
            high = mid - 1;
        }
    }

    // |coef_full - target*coef| / coef, compared across the two neighbours by
    // cross-multiplication so no division and no float enters the choice.
    let error_terms = |half_width: i32| -> Option<(U256, U256)> {
        let coef = coef_at(half_width)?;
        let scaled = coef.checked_mul(target_u)?;
        let gap = if coef_full >= scaled {
            coef_full.checked_sub(scaled)?
        } else {
            scaled.checked_sub(coef_full)?
        };
        Some((gap, coef))
    };

    let mut best = low;
    if let (Some((gap_a, coef_a)), Some((gap_b, coef_b))) = (error_terms(low), error_terms(low + 1))
    {
        // gap_a / coef_a  vs  gap_b / coef_b
        if let (Some(left), Some(right)) = (gap_a.checked_mul(coef_b), gap_b.checked_mul(coef_a)) {
            if right < left {
                best = low + 1;
            }
        }
    }

    let coef_best = coef_at(best)?;
    let liquidity = mul_div(U256::from_u128(full_liquidity), coef_full, coef_best)?;
    Some(ConcentratedChoice {
        half_width_ticks: best,
        liquidity: liquidity.as_u128()?,
        factor_num: coef_full,
        factor_den: coef_best,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_price_is_exact() {
        // sqrtPriceX96^2 == 100 * (2^96)^2 with no remainder.
        let q96 = U256::from_u128(1 << 96);
        assert_eq!(
            OPENING_SQRT_PRICE_X96
                .checked_mul(OPENING_SQRT_PRICE_X96)
                .unwrap(),
            q96.checked_mul(q96)
                .unwrap()
                .checked_mul(U256::from_u64(100))
                .unwrap()
        );
    }

    #[test]
    fn opening_tick_is_what_the_pool_would_derive() {
        assert_eq!(
            tick_math::get_tick_at_sqrt_ratio(OPENING_SQRT_PRICE_X96).unwrap(),
            OPENING_TICK
        );
    }

    #[test]
    fn full_range_holds_the_benchmark_capital() {
        let (_, state) = full_range_pool(FULL_RANGE_LIQUIDITY);
        let amount0 = sqrt_price_math::get_amount0_delta(
            state.sqrt_price_x96,
            tick_math::MAX_SQRT_RATIO,
            state.liquidity,
            true,
        )
        .unwrap();
        let amount1 = sqrt_price_math::get_amount1_delta(
            tick_math::MIN_SQRT_RATIO,
            state.sqrt_price_x96,
            state.liquidity,
            true,
        )
        .unwrap();
        // 100 token0 and 10 000 token1, less the 54 wei each that the finite
        // range endpoints strand. Recorded exactly rather than rounded away.
        assert_eq!(amount0.to_string(), "99999999999999999946");
        assert_eq!(amount1.to_string(), "9999999999999999999946");
    }

    #[test]
    fn concentrated_pool_brackets_the_opening_tick() {
        let (config, state) = concentrated_pool(200, FULL_RANGE_LIQUIDITY);
        let ticks = config.ticks.as_slice();
        assert_eq!(ticks.len(), 2);
        assert!(ticks[0].tick < state.tick && ticks[1].tick > state.tick);
        assert_eq!(ticks[0].liquidity_net, -ticks[1].liquidity_net);
    }

    /// The half widths are recorded, not asserted to be "correct": they are what
    /// the integer search returns, and a change to the search must show up here.
    #[test]
    fn impact_matched_ranges_are_pinned() {
        let expected = [
            // (target, half width, achieved factor to 6 dp)
            (2_u64, 13_864_i32, "1.999963"),
            (5, 4_463, "5.000094"),
            (10, 2_107, "10.001420"),
            (20, 1_026, "19.998426"),
            (50, 404, "50.009108"),
            (100, 201, "100.008300"),
            (1000, 20, "1000.550082"),
        ];
        for (target, half_width, factor) in expected {
            let choice = concentrated_for_impact_factor(target, FULL_RANGE_LIQUIDITY)
                .unwrap_or_else(|| panic!("no range found for target {target}"));
            assert_eq!(choice.half_width_ticks, half_width, "target {target}");
            assert_eq!(choice.factor_string(6), factor, "target {target}");
        }
    }

    /// A tick is 1 bp wide, so an exact match is generally unreachable. What must
    /// hold is that the residual is small and is reported rather than rounded
    /// away, which `factor_string` above records digit for digit.
    #[test]
    fn impact_matching_lands_within_a_tick_of_the_target() {
        for target in [2_u64, 5, 10, 20, 50, 100, 1000] {
            let choice = concentrated_for_impact_factor(target, FULL_RANGE_LIQUIDITY).unwrap();
            // |achieved - target| / target < 1e-3
            let target_u = U256::from_u64(target);
            let achieved_scaled = mul_div(
                choice.factor_num,
                U256::from_u64(1_000_000),
                choice.factor_den,
            )
            .unwrap();
            let target_scaled = target_u.checked_mul(U256::from_u64(1_000_000)).unwrap();
            let gap = if achieved_scaled >= target_scaled {
                achieved_scaled.checked_sub(target_scaled).unwrap()
            } else {
                target_scaled.checked_sub(achieved_scaled).unwrap()
            };
            let tolerance = target_u.checked_mul(U256::from_u64(1_000)).unwrap();
            assert!(
                gap <= tolerance,
                "target {target}: achieved {} is further than 0.1% away",
                choice.factor_string(6)
            );
        }
    }

    /// Narrower ranges must concentrate more, monotonically -- the property the
    /// binary search relies on.
    #[test]
    fn narrower_ranges_concentrate_more() {
        let mut previous = U256::ZERO;
        for half_width in [20_i32, 201, 404, 1_026, 2_107, 4_463, 13_864] {
            let lower = tick_math::get_sqrt_ratio_at_tick(OPENING_TICK - half_width).unwrap();
            let upper = tick_math::get_sqrt_ratio_at_tick(OPENING_TICK + half_width).unwrap();
            let coefficient = capital_coefficient(lower, OPENING_SQRT_PRICE_X96, upper).unwrap();
            assert!(
                coefficient > previous,
                "coefficient must grow with the half width at {half_width}"
            );
            previous = coefficient;
        }
        assert!(
            full_range_coefficient() > previous,
            "the full range must have the largest coefficient of all"
        );
    }

    /// Same deposited capital, by construction. Checked against the pinned
    /// `getAmount*Delta` rather than against the formula that chose it.
    #[test]
    fn impact_matched_positions_hold_the_same_capital() {
        let full = full_range_pool(FULL_RANGE_LIQUIDITY);
        let full_value = position_value(
            tick_math::MIN_SQRT_RATIO,
            tick_math::MAX_SQRT_RATIO,
            full.1.sqrt_price_x96,
            full.1.liquidity,
        );

        for target in [2_u64, 10, 100, 1000] {
            let choice = concentrated_for_impact_factor(target, FULL_RANGE_LIQUIDITY).unwrap();
            let lower =
                tick_math::get_sqrt_ratio_at_tick(OPENING_TICK - choice.half_width_ticks).unwrap();
            let upper =
                tick_math::get_sqrt_ratio_at_tick(OPENING_TICK + choice.half_width_ticks).unwrap();
            let value = position_value(lower, upper, OPENING_SQRT_PRICE_X96, choice.liquidity);

            // Within 1e-9 of the full-range capital, in token1 wei terms.
            let gap = if value >= full_value {
                value.checked_sub(full_value).unwrap()
            } else {
                full_value.checked_sub(value).unwrap()
            };
            let tolerance = mul_div(full_value, U256::from_u64(1), U256::from_u64(1_000_000_000))
                .unwrap()
                .checked_add(U256::from_u64(1))
                .unwrap();
            assert!(
                gap <= tolerance,
                "target {target}: capital {value} differs from {full_value} by {gap}"
            );
        }
    }

    /// `amount1 + amount0 * price`, in token1 wei, from the pinned delta helpers.
    fn position_value(
        sqrt_lower: U256,
        sqrt_upper: U256,
        sqrt_price: U256,
        liquidity: u128,
    ) -> U256 {
        let amount0 =
            sqrt_price_math::get_amount0_delta(sqrt_price, sqrt_upper, liquidity, false).unwrap();
        let amount1 =
            sqrt_price_math::get_amount1_delta(sqrt_lower, sqrt_price, liquidity, false).unwrap();
        // price = sqrt_price^2 / 2^192, applied without leaving the integers.
        let q96 = U256::ONE.checked_shl(96).unwrap();
        let base_in_quote =
            mul_div(mul_div(amount0, sqrt_price, q96).unwrap(), sqrt_price, q96).unwrap();
        amount1.checked_add(base_in_quote).unwrap()
    }
}
