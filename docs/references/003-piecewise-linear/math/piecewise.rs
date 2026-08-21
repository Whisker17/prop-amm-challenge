//! Piecewise Linear AMM Math
//!
//! Extends the linear AMM formulas to work with 7-point piecewise curves.
//! Each side has 6 segments with equal quantity distribution.

use super::{buy_base_with_quote, calculate_k, sell_base_for_quote, PRICE_SCALE};
use crate::state::NUM_SEGMENTS;

/// Buy base tokens with quote tokens across piecewise segments (ask side)
///
/// Iterates through segments from lowest price (0) to highest (5),
/// consuming liquidity and accumulating base tokens bought.
///
/// # Arguments
/// * `quote_in` - Quote tokens to spend (native units)
/// * `prices` - 7 price points (ascending)
/// * `total_quantity` - Total base quantity available
/// * `consumed` - Amount of base already consumed
///
/// # Returns
/// * `Some((base_out, quote_used, new_consumed))` on success
/// * `None` on math error
pub fn buy_base_piecewise(
    quote_in: u64,
    prices: &[u64; 7],
    total_quantity: u64,
    consumed: u64,
) -> Option<(u64, u64, u64)> {
    if quote_in == 0 || total_quantity == 0 {
        return Some((0, 0, consumed));
    }

    let remaining = total_quantity.saturating_sub(consumed);
    if remaining == 0 {
        return Some((0, 0, consumed));
    }

    let q_segment = total_quantity / NUM_SEGMENTS as u64;
    if q_segment == 0 {
        return Some((0, 0, consumed));
    }

    let mut base_out: u64 = 0;
    let mut quote_remaining = quote_in;
    let mut current_consumed = consumed;

    // Iterate through segments from low to high price
    for _ in 0..NUM_SEGMENTS {
        if quote_remaining == 0 {
            break;
        }

        // Determine current segment
        let seg_idx = (current_consumed / q_segment) as usize;
        if seg_idx >= NUM_SEGMENTS {
            break; // All liquidity consumed
        }

        // Calculate remaining in current segment
        let consumed_in_seg = current_consumed % q_segment;
        let remaining_in_seg = q_segment.saturating_sub(consumed_in_seg);

        if remaining_in_seg == 0 {
            continue;
        }

        // Get segment price bounds
        let p_low = prices[seg_idx];
        let p_high = prices[seg_idx + 1];

        if p_high <= p_low {
            return None; // Invalid price ordering
        }

        // Calculate effective lower price based on consumption within segment
        // The price has already moved up based on what's been consumed
        let effective_lower = interpolate_price(p_low, p_high, consumed_in_seg, q_segment);

        // Try to buy within this segment
        let (base_bought, _new_lower) =
            buy_base_with_quote(quote_remaining, remaining_in_seg, effective_lower, p_high)?;

        if base_bought == 0 {
            break;
        }

        // Calculate quote actually used for this purchase
        let quote_used = calculate_quote_for_base(base_bought, effective_lower, p_high, remaining_in_seg)?;

        base_out = base_out.checked_add(base_bought)?;
        current_consumed = current_consumed.checked_add(base_bought)?;
        quote_remaining = quote_remaining.saturating_sub(quote_used);
    }

    let quote_used = quote_in.saturating_sub(quote_remaining);
    Some((base_out, quote_used, current_consumed))
}

/// Sell base tokens for quote tokens across piecewise segments (bid side)
///
/// Iterates through segments from highest price (5) to lowest (0),
/// consuming bid liquidity and accumulating quote tokens received.
///
/// # Arguments
/// * `base_in` - Base tokens to sell (native units)
/// * `prices` - 7 price points (ascending)
/// * `total_quantity` - Total quote quantity available on bid side
/// * `consumed` - Amount of quote already consumed
///
/// # Returns
/// * `Some((quote_out, base_used, new_consumed))` on success
/// * `None` on math error
pub fn sell_base_piecewise(
    base_in: u64,
    prices: &[u64; 7],
    total_quantity: u64,
    consumed: u64,
) -> Option<(u64, u64, u64)> {
    if base_in == 0 || total_quantity == 0 {
        return Some((0, 0, consumed));
    }

    let remaining_quote = total_quantity.saturating_sub(consumed);
    if remaining_quote == 0 {
        return Some((0, 0, consumed));
    }

    let q_segment = total_quantity / NUM_SEGMENTS as u64;
    if q_segment == 0 {
        return Some((0, 0, consumed));
    }

    let mut quote_out: u64 = 0;
    let mut base_remaining = base_in;
    let mut current_consumed = consumed;

    // Iterate through segments from high to low price (5 → 0)
    for _ in 0..NUM_SEGMENTS {
        if base_remaining == 0 {
            break;
        }

        // Determine current segment (counting from high end)
        // consumed / q_segment gives how many segments consumed from high side
        let segments_consumed = (current_consumed / q_segment) as usize;
        if segments_consumed >= NUM_SEGMENTS {
            break; // All liquidity consumed
        }

        // Current segment index (5, 4, 3, 2, 1, 0)
        let seg_idx = NUM_SEGMENTS - 1 - segments_consumed;

        // Calculate remaining in current segment
        let consumed_in_seg = current_consumed % q_segment;
        let remaining_in_seg = q_segment.saturating_sub(consumed_in_seg);

        if remaining_in_seg == 0 {
            continue;
        }

        // Get segment price bounds
        let p_low = prices[seg_idx];
        let p_high = prices[seg_idx + 1];

        if p_high <= p_low {
            return None; // Invalid price ordering
        }

        // Calculate effective upper price based on consumption within segment
        // The price has already moved down based on what's been consumed
        let effective_upper = interpolate_price_down(p_high, p_low, consumed_in_seg, q_segment);

        // Try to sell within this segment
        let (quote_received, _new_upper) =
            sell_base_for_quote(base_remaining, remaining_in_seg, p_low, effective_upper)?;

        if quote_received == 0 {
            break;
        }

        // Calculate base actually used for this sale
        let base_used = calculate_base_for_quote(quote_received, p_low, effective_upper, remaining_in_seg)?;

        quote_out = quote_out.checked_add(quote_received)?;
        current_consumed = current_consumed.checked_add(quote_received)?;
        base_remaining = base_remaining.saturating_sub(base_used);
    }

    let base_used = base_in.saturating_sub(base_remaining);
    Some((quote_out, base_used, current_consumed))
}

/// Interpolate price based on consumption within a segment (moving up)
/// Used for ask side where price increases as liquidity is consumed
#[inline]
fn interpolate_price(p_low: u64, p_high: u64, consumed: u64, total: u64) -> u64 {
    if total == 0 || consumed == 0 {
        return p_low;
    }
    if consumed >= total {
        return p_high;
    }

    let range = p_high.saturating_sub(p_low);
    let offset = (range as u128) * (consumed as u128) / (total as u128);
    p_low.saturating_add(offset as u64)
}

/// Interpolate price based on consumption within a segment (moving down)
/// Used for bid side where price decreases as liquidity is consumed
#[inline]
fn interpolate_price_down(p_high: u64, p_low: u64, consumed: u64, total: u64) -> u64 {
    if total == 0 || consumed == 0 {
        return p_high;
    }
    if consumed >= total {
        return p_low;
    }

    let range = p_high.saturating_sub(p_low);
    let offset = (range as u128) * (consumed as u128) / (total as u128);
    p_high.saturating_sub(offset as u64)
}

/// Calculate quote required to buy a given amount of base in a segment
#[inline]
fn calculate_quote_for_base(
    base_bought: u64,
    lower_price: u64,
    upper_price: u64,
    segment_quantity: u64,
) -> Option<u64> {
    if base_bought == 0 {
        return Some(0);
    }

    let k = calculate_k(segment_quantity, lower_price, upper_price)?;
    if k == 0 {
        return None;
    }

    // Cost = integral of P(q) dq from 0 to base_bought
    // = P_lower * base_bought + base_bought² / (2k)
    // In scaled arithmetic:
    // term1 = base_bought * lower_price / SCALE
    // term2 = base_bought² * SCALE / (2k) / SCALE = base_bought² / (2k)

    let term1 = (base_bought as u128) * (lower_price as u128) / (PRICE_SCALE as u128);
    let base_sq = (base_bought as u128) * (base_bought as u128);
    let term2 = base_sq / (2 * k as u128);

    let quote = term1.checked_add(term2)?;
    Some(quote as u64)
}

/// Calculate base required to receive a given amount of quote in a segment
#[inline]
fn calculate_base_for_quote(
    quote_received: u64,
    lower_price: u64,
    upper_price: u64,
    _segment_quantity: u64,
) -> Option<u64> {
    if quote_received == 0 {
        return Some(0);
    }

    // For selling, quote_received relates to base_in via:
    // quote = base * P_upper / SCALE - base² / (2k)
    // This is quadratic in base, so we need to solve it
    // For simplicity, use the ratio approach:
    // base ≈ quote * SCALE / avg_price

    let avg_price = (upper_price.checked_add(lower_price)?) / 2;
    if avg_price == 0 {
        return None;
    }

    let base = (quote_received as u128) * (PRICE_SCALE as u128) / (avg_price as u128);
    Some(base as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interpolate_price() {
        let p_low = 100 * PRICE_SCALE;
        let p_high = 200 * PRICE_SCALE;

        // 0% consumed -> p_low
        assert_eq!(interpolate_price(p_low, p_high, 0, 100), p_low);

        // 50% consumed -> midpoint
        assert_eq!(interpolate_price(p_low, p_high, 50, 100), 150 * PRICE_SCALE);

        // 100% consumed -> p_high
        assert_eq!(interpolate_price(p_low, p_high, 100, 100), p_high);
    }

    #[test]
    fn test_buy_base_piecewise_simple() {
        // 6 segments, prices from $100 to $700 (100 spacing)
        let prices = [
            100 * PRICE_SCALE,
            200 * PRICE_SCALE,
            300 * PRICE_SCALE,
            400 * PRICE_SCALE,
            500 * PRICE_SCALE,
            600 * PRICE_SCALE,
            700 * PRICE_SCALE,
        ];

        // Total quantity: 600 units (100 per segment)
        let total_quantity = 600_000_000u64;
        let consumed = 0u64;

        // Try to buy with some quote
        let quote_in = 100_000_000u64; // 100 quote units

        let result = buy_base_piecewise(quote_in, &prices, total_quantity, consumed);
        assert!(result.is_some());

        let (base_out, quote_used, new_consumed) = result.unwrap();

        // Should have bought something
        assert!(base_out > 0, "base_out should be > 0");
        assert!(quote_used > 0, "quote_used should be > 0");
        assert!(quote_used <= quote_in, "quote_used should be <= quote_in");
        assert_eq!(new_consumed, base_out, "new_consumed should equal base_out");
    }

    #[test]
    fn test_sell_base_piecewise_simple() {
        // 6 segments, prices from $100 to $700
        let prices = [
            100 * PRICE_SCALE,
            200 * PRICE_SCALE,
            300 * PRICE_SCALE,
            400 * PRICE_SCALE,
            500 * PRICE_SCALE,
            600 * PRICE_SCALE,
            700 * PRICE_SCALE,
        ];

        // Total quote quantity: 600 units (100 per segment)
        let total_quantity = 600_000_000u64;
        let consumed = 0u64;

        // Try to sell some base
        let base_in = 10_000_000u64; // 10 base units

        let result = sell_base_piecewise(base_in, &prices, total_quantity, consumed);
        assert!(result.is_some());

        let (quote_out, base_used, new_consumed) = result.unwrap();

        // Should have received something
        assert!(quote_out > 0, "quote_out should be > 0");
        assert!(base_used > 0, "base_used should be > 0");
        assert!(base_used <= base_in, "base_used should be <= base_in");
        assert_eq!(new_consumed, quote_out, "new_consumed should equal quote_out");
    }

    #[test]
    fn test_piecewise_zero_quantity() {
        let prices = [
            100 * PRICE_SCALE,
            200 * PRICE_SCALE,
            300 * PRICE_SCALE,
            400 * PRICE_SCALE,
            500 * PRICE_SCALE,
            600 * PRICE_SCALE,
            700 * PRICE_SCALE,
        ];

        // Zero total quantity
        let result = buy_base_piecewise(100, &prices, 0, 0);
        assert_eq!(result, Some((0, 0, 0)));

        let result = sell_base_piecewise(100, &prices, 0, 0);
        assert_eq!(result, Some((0, 0, 0)));
    }

    #[test]
    fn test_piecewise_fully_consumed() {
        let prices = [
            100 * PRICE_SCALE,
            200 * PRICE_SCALE,
            300 * PRICE_SCALE,
            400 * PRICE_SCALE,
            500 * PRICE_SCALE,
            600 * PRICE_SCALE,
            700 * PRICE_SCALE,
        ];

        let total_quantity = 600_000_000u64;
        let consumed = total_quantity; // Fully consumed

        // Should return 0 output
        let result = buy_base_piecewise(100_000_000, &prices, total_quantity, consumed);
        assert_eq!(result, Some((0, 0, consumed)));

        let result = sell_base_piecewise(100_000_000, &prices, total_quantity, consumed);
        assert_eq!(result, Some((0, 0, consumed)));
    }
}
