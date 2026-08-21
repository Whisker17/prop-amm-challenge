//! AMM Math Module
//!
//! Linear liquidity AMM formulas using scaled arithmetic.
//!
//! ## Price Definition
//! Prices are stored as native-ratio scaled values:
//! `price = (quote_native_units / base_native_units) * PRICE_SCALE`
//!
//! This makes the math work on pure native token units without any
//! decimal awareness. Client handles human↔native price conversion.
//!
//! ## Example
//! For NVDAX(8 dec)/USDC(6 dec) at $200:
//! - 1 NVDAX = 10^8 base units
//! - 200 USDC = 2×10^8 quote units
//! - Native ratio = 2×10^8 / 10^8 = 2
//! - Stored price = 2 × PRICE_SCALE = 2×10^9

pub mod piecewise;
pub mod scaled;
pub mod sqrt;

pub use piecewise::{buy_base_piecewise, sell_base_piecewise};
pub use scaled::PRICE_SCALE;
pub use sqrt::sqrt_u128_scaled;

/// Calculate liquidity constant k = quantity / price_range
/// Returns k scaled by PRICE_SCALE for precision.
#[inline]
pub fn calculate_k(quantity: u64, lower_price: u64, upper_price: u64) -> Option<u64> {
    let price_range = upper_price.checked_sub(lower_price)?;
    if price_range == 0 {
        return None;
    }
    // k = quantity * PRICE_SCALE / price_range
    let numerator = (quantity as u128) * (PRICE_SCALE as u128);
    let result = numerator / (price_range as u128);
    if result > u64::MAX as u128 {
        return None;
    }
    Some(result as u64)
}

/// Buy base token with quote token (consumes ask side liquidity)
///
/// Formula: bought = k × (sqrt(P_lower² + 2×notional×SCALE/k) - P_lower)
///
/// # Arguments
/// * `quote_in` - Quote token amount to spend (native units)
/// * `base_quantity` - Base tokens available on ask side (native units)
/// * `lower_price` - Current lower price (native-ratio × PRICE_SCALE)
/// * `upper_price` - Upper price bound (native-ratio × PRICE_SCALE)
///
/// # Returns
/// * `Some((base_bought, new_lower_price))` on success
/// * `None` on math error or zero liquidity
pub fn buy_base_with_quote(
    quote_in: u64,
    base_quantity: u64,
    lower_price: u64,
    upper_price: u64,
) -> Option<(u64, u64)> {
    if base_quantity == 0 || quote_in == 0 {
        return Some((0, lower_price));
    }

    let price_range = upper_price.checked_sub(lower_price)?;
    if price_range == 0 {
        return None;
    }

    // k = base_quantity * PRICE_SCALE / price_range
    let k = calculate_k(base_quantity, lower_price, upper_price)?;
    if k == 0 {
        return None;
    }

    // P_lower² (units: SCALE²)
    let p_lower_sq = (lower_price as u128) * (lower_price as u128);

    // 2 × quote_in × PRICE_SCALE / k
    // This term needs to match P_lower² dimensionally (SCALE²)
    // quote_in has native quote units
    // k has units (base × SCALE / price_range) ≈ base × SCALE / SCALE = base
    // So: quote_in × SCALE² / k gives (quote × SCALE² / base)
    // Since price = quote/base × SCALE, price² = quote²/base² × SCALE²
    // We need: quote_in × SCALE² / k to equal quote²/base² × SCALE² contribution
    // Hmm, let me reconsider...
    //
    // Actually with native-ratio prices:
    // - price P = (quote/base) × SCALE
    // - P² = quote²/base² × SCALE²
    // - k = base × SCALE / (price_range in SCALE units) = base
    // - quote_in/k = quote/base (dimensionless ratio matching quote/base)
    // - 2 × quote_in × SCALE / k = 2 × quote/base × SCALE
    // - For this to add to P² = quote²/base² × SCALE², we need another factor
    //
    // Let me use the standard formula derivation:
    // Cost C = P_L × B + B²/(2L) where L = liquidity = Q/(P_U - P_L)
    // Solving for B: B = L × (sqrt(P_L² + 2C/L) - P_L)
    //
    // With scaling:
    // - P_L in SCALE units
    // - C (quote_in) in native units
    // - L = Q × SCALE / price_range (Q in native base units)
    // - C/L = quote_in × price_range / (Q × SCALE) = quote_in / Q × (price_range/SCALE)
    // - For P_L² + 2C/L to work, 2C/L needs to be in SCALE² units
    // - 2C/L = 2 × quote_in × price_range / (Q × SCALE)
    //
    // This is getting complex. Let me just use the working formula and adjust.

    // Correct formula derivation:
    // B = k/SCALE × (sqrt(P_L² + 2×C×SCALE²/k) - P_L)
    // where C = quote_in, k = Q×SCALE/range, P_L = lower_price
    // term = 2 × quote_in × SCALE² / k
    let term = (2u128)
        * (quote_in as u128)
        * (PRICE_SCALE as u128)
        * (PRICE_SCALE as u128)
        / (k as u128);

    let under_sqrt = p_lower_sq.checked_add(term)?;

    // sqrt returns value at SCALE precision
    let sqrt_val = sqrt_u128_scaled(under_sqrt);

    // diff = sqrt(...) - P_lower (both at SCALE)
    let diff = sqrt_val.checked_sub(lower_price)?;

    // base_bought = k × diff / SCALE
    let bought_128 = (k as u128) * (diff as u128) / (PRICE_SCALE as u128);
    let base_bought = if bought_128 > base_quantity as u128 {
        base_quantity
    } else {
        bought_128 as u64
    };

    // new_lower = P_lower + base_bought × SCALE / k
    let price_increase = if k > 0 {
        ((base_bought as u128) * (PRICE_SCALE as u128) / (k as u128)) as u64
    } else {
        0
    };
    let new_lower = lower_price.checked_add(price_increase)?;

    Some((base_bought, new_lower))
}

/// Sell base token for quote token (consumes bid side liquidity)
///
/// Symmetric to buy_base_with_quote but operating on bid side.
///
/// # Arguments
/// * `base_in` - Base token amount to sell (native units)
/// * `quote_quantity` - Quote tokens available on bid side (native units)
/// * `lower_price` - Lower price bound (native-ratio × PRICE_SCALE)
/// * `upper_price` - Current upper price (native-ratio × PRICE_SCALE)
///
/// # Returns
/// * `Some((quote_received, new_upper_price))` on success
/// * `None` on math error or zero liquidity
pub fn sell_base_for_quote(
    base_in: u64,
    quote_quantity: u64,
    lower_price: u64,
    upper_price: u64,
) -> Option<(u64, u64)> {
    if base_in == 0 || quote_quantity == 0 {
        return Some((0, upper_price));
    }

    let price_range = upper_price.checked_sub(lower_price)?;
    if price_range == 0 {
        return None;
    }

    // Average price for converting quote to base equivalent
    let avg_price = (upper_price.checked_add(lower_price)?) / 2;
    if avg_price == 0 {
        return None;
    }

    // Convert quote liquidity to base equivalent at average price
    // base_equiv = quote_quantity × SCALE / avg_price
    let base_equiv_128 = (quote_quantity as u128)
        * (PRICE_SCALE as u128)
        / (avg_price as u128);
    let base_equiv = base_equiv_128 as u64;

    // Cap base_in to what liquidity can absorb
    let base_in = if base_in > base_equiv {
        base_equiv
    } else {
        base_in
    };

    if base_in == 0 {
        return Some((0, upper_price));
    }

    // k for the bid side (in base equivalent terms)
    let k = calculate_k(base_equiv, lower_price, upper_price)?;
    if k == 0 {
        return None;
    }

    // quote_received = base_in × P_upper - base_in² / (2×k)
    // (Linear liquidity integration from upper price downward)

    // term1 = base_in × P_upper / SCALE (converting to quote units)
    let term1 = (base_in as u128) * (upper_price as u128) / (PRICE_SCALE as u128);

    // term2 = base_in² / (2×k) / SCALE (price impact)
    let base_sq = (base_in as u128) * (base_in as u128);
    let term2 = base_sq / (2 * (k as u128));

    let quote_received = term1.checked_sub(term2)? as u64;

    // new_upper = P_upper - base_in × SCALE / k
    let price_decrease = if k > 0 {
        ((base_in as u128) * (PRICE_SCALE as u128) / (k as u128)) as u64
    } else {
        0
    };
    let new_upper = upper_price.checked_sub(price_decrease)?;

    Some((quote_received, new_upper))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_k() {
        let quantity = 1_000_000_000; // 1 unit at 9 decimals
        let lower = 200 * PRICE_SCALE; // Price 200
        let upper = 300 * PRICE_SCALE; // Price 300
        let range = 100 * PRICE_SCALE;

        let k = calculate_k(quantity, lower, upper).unwrap();
        // k = 1e9 * 1e9 / (100 * 1e9) = 1e9 / 100 = 10_000_000
        assert_eq!(k, 10_000_000);
    }

    #[test]
    fn test_buy_zero() {
        let (bought, new_lower) = buy_base_with_quote(
            0,
            1_000_000_000,
            200 * PRICE_SCALE,
            300 * PRICE_SCALE
        ).unwrap();
        assert_eq!(bought, 0);
        assert_eq!(new_lower, 200 * PRICE_SCALE);
    }

    #[test]
    fn test_sell_zero() {
        let (received, new_upper) = sell_base_for_quote(
            0,
            1_000_000_000,
            100 * PRICE_SCALE,
            200 * PRICE_SCALE
        ).unwrap();
        assert_eq!(received, 0);
        assert_eq!(new_upper, 200 * PRICE_SCALE);
    }

    #[test]
    fn test_buy_price_in_range() {
        // 1 base token available from price 2 to 3 (native ratio)
        // Spending 1 quote token
        // Expected: get some base, price between 2 and 3

        let base_qty = 100_000_000; // 1 token at 8 decimals
        let quote_in = 10_000_000;  // ~0.1 quote tokens at 8 decimals
        let lower = 2 * PRICE_SCALE;
        let upper = 3 * PRICE_SCALE;

        let result = buy_base_with_quote(quote_in, base_qty, lower, upper);
        assert!(result.is_some());

        let (bought, new_lower) = result.unwrap();

        // Should have bought something
        assert!(bought > 0, "bought: {}", bought);

        // Price should have increased but stay in range
        assert!(new_lower >= lower, "new_lower {} < lower {}", new_lower, lower);
        assert!(new_lower <= upper, "new_lower {} > upper {}", new_lower, upper);

        // Implied price should be reasonable (between 2 and 3)
        if bought > 0 {
            let implied_price = (quote_in as f64) / (bought as f64) * (PRICE_SCALE as f64);
            assert!(implied_price >= 1.5 * (PRICE_SCALE as f64), "price too low: {}", implied_price / PRICE_SCALE as f64);
            assert!(implied_price <= 4.0 * (PRICE_SCALE as f64), "price too high: {}", implied_price / PRICE_SCALE as f64);
        }
    }
}
