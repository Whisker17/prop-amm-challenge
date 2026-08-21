//! Scaled u64 Arithmetic
//!
//! Uses u64 with 10^9 scaling for prices, and native token units for quantities.
//! This provides sufficient precision while using efficient native u64 operations.

/// Scaling factor for prices: 10^9 (1 billion)
/// Example: $150.50 = 150_500_000_000
pub const PRICE_SCALE: u64 = 1_000_000_000;

/// USDC has 6 decimals, so 1 USDC = 1_000_000 micro-units
pub const USDC_DECIMALS: u64 = 1_000_000;

/// SOL has 9 decimals, so 1 SOL = 1_000_000_000 lamports
pub const SOL_DECIMALS: u64 = 1_000_000_000;

/// Multiply two scaled values: (a * b) / SCALE
/// Uses u128 intermediate to avoid overflow
#[inline]
pub fn mul_scaled(a: u64, b: u64) -> Option<u64> {
    let product = (a as u128) * (b as u128);
    let result = product / (PRICE_SCALE as u128);
    if result > u64::MAX as u128 {
        return None;
    }
    Some(result as u64)
}

/// Divide two scaled values: (a * SCALE) / b
/// Uses u128 intermediate to maintain precision
#[inline]
pub fn div_scaled(a: u64, b: u64) -> Option<u64> {
    if b == 0 {
        return None;
    }
    let numerator = (a as u128) * (PRICE_SCALE as u128);
    let result = numerator / (b as u128);
    if result > u64::MAX as u128 {
        return None;
    }
    Some(result as u64)
}

/// Multiply price by quantity, returning in native token units
/// price is scaled by 10^9, quantity is in native units
/// result = (price * quantity) / PRICE_SCALE
#[inline]
pub fn mul_price_quantity(price: u64, quantity: u64) -> Option<u64> {
    let product = (price as u128) * (quantity as u128);
    let result = product / (PRICE_SCALE as u128);
    if result > u64::MAX as u128 {
        return None;
    }
    Some(result as u64)
}

/// Divide quantity by price, maintaining precision
/// quantity is in native units, price is scaled by 10^9
/// result = (quantity * PRICE_SCALE) / price
#[inline]
pub fn div_quantity_price(quantity: u64, price: u64) -> Option<u64> {
    if price == 0 {
        return None;
    }
    let numerator = (quantity as u128) * (PRICE_SCALE as u128);
    let result = numerator / (price as u128);
    if result > u64::MAX as u128 {
        return None;
    }
    Some(result as u64)
}

/// Safe addition with overflow check
#[inline]
pub fn add(a: u64, b: u64) -> Option<u64> {
    a.checked_add(b)
}

/// Safe subtraction with underflow check
#[inline]
pub fn sub(a: u64, b: u64) -> Option<u64> {
    a.checked_sub(b)
}

/// Convert a price to scaled format
/// e.g., 150.50 USD -> from_price(150, 500_000_000) = 150_500_000_000
#[inline]
pub const fn from_price(integer: u64, frac_nanos: u64) -> u64 {
    integer * PRICE_SCALE + frac_nanos
}

/// Extract integer part from scaled price
#[inline]
pub const fn price_integer(scaled: u64) -> u64 {
    scaled / PRICE_SCALE
}

/// Extract fractional part (in nanos) from scaled price
#[inline]
pub const fn price_frac(scaled: u64) -> u64 {
    scaled % PRICE_SCALE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mul_scaled() {
        // 2.0 * 3.0 = 6.0
        let a = 2 * PRICE_SCALE;
        let b = 3 * PRICE_SCALE;
        let c = mul_scaled(a, b).unwrap();
        assert_eq!(c, 6 * PRICE_SCALE);
    }

    #[test]
    fn test_mul_scaled_fractional() {
        // 1.5 * 2.0 = 3.0
        let a = PRICE_SCALE + PRICE_SCALE / 2; // 1.5
        let b = 2 * PRICE_SCALE; // 2.0
        let c = mul_scaled(a, b).unwrap();
        assert_eq!(c, 3 * PRICE_SCALE);
    }

    #[test]
    fn test_div_scaled() {
        // 6.0 / 2.0 = 3.0
        let a = 6 * PRICE_SCALE;
        let b = 2 * PRICE_SCALE;
        let c = div_scaled(a, b).unwrap();
        assert_eq!(c, 3 * PRICE_SCALE);
    }

    #[test]
    fn test_div_scaled_fractional() {
        // 5.0 / 2.0 = 2.5
        let a = 5 * PRICE_SCALE;
        let b = 2 * PRICE_SCALE;
        let c = div_scaled(a, b).unwrap();
        assert_eq!(c, 2 * PRICE_SCALE + PRICE_SCALE / 2);
    }

    #[test]
    fn test_mul_price_quantity() {
        // Price: $150 per SOL, quantity: 2 SOL = 2_000_000_000 lamports
        // Expected: $300 worth = 300_000_000 USDC micro-units
        let price = 150 * PRICE_SCALE; // $150 scaled
        let quantity = 2 * SOL_DECIMALS; // 2 SOL in lamports
        let result = mul_price_quantity(price, quantity).unwrap();
        // Result is in USDC micro-units scaled appropriately
        // (150 * 10^9) * (2 * 10^9) / 10^9 = 300 * 10^9
        assert_eq!(result, 300 * PRICE_SCALE);
    }

    #[test]
    fn test_div_quantity_price() {
        // $300 USDC worth at $150/SOL should give ~2 SOL
        let quantity = 300 * PRICE_SCALE;
        let price = 150 * PRICE_SCALE;
        let result = div_quantity_price(quantity, price).unwrap();
        assert_eq!(result, 2 * PRICE_SCALE);
    }

    #[test]
    fn test_from_price() {
        // $150.50
        let price = from_price(150, 500_000_000);
        assert_eq!(price, 150_500_000_000);
        assert_eq!(price_integer(price), 150);
        assert_eq!(price_frac(price), 500_000_000);
    }

    #[test]
    fn test_div_by_zero() {
        assert_eq!(div_scaled(100, 0), None);
        assert_eq!(div_quantity_price(100, 0), None);
    }
}
