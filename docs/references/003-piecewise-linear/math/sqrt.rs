//! Newton-Raphson Square Root
//!
//! Implements iterative square root for u64 and u128 values.
//! Target: < 0.01% error
//!
//! Algorithm: x_{n+1} = (x_n + S/x_n) / 2

use super::scaled::PRICE_SCALE;

/// Integer square root of u64 using Newton-Raphson
/// Returns floor(sqrt(val))
#[inline]
pub fn sqrt_u64(val: u64) -> u64 {
    if val == 0 {
        return 0;
    }
    if val == 1 {
        return 1;
    }

    // Initial guess: start high to ensure convergence from above
    // 2^((log2(val) + 2) / 2) gives us a value >= sqrt(val)
    let leading_zeros = val.leading_zeros();
    let bit_pos = 63 - leading_zeros;
    let mut x = 1u64 << ((bit_pos / 2) + 1);

    // Newton-Raphson: x_{n+1} = (x_n + val/x_n) / 2
    // Converges monotonically from above
    loop {
        let x_new = (x + val / x) / 2;
        if x_new >= x {
            break;
        }
        x = x_new;
    }

    // Final adjustment to ensure floor(sqrt)
    // After Newton-Raphson from above, x should be very close
    while x * x > val {
        x -= 1;
    }

    x
}

/// Integer square root of u128 using Newton-Raphson
/// Returns floor(sqrt(val)) as u64 (for values up to ~3.4e38)
#[inline]
pub fn sqrt_u128(val: u128) -> u64 {
    if val == 0 {
        return 0;
    }
    if val <= u64::MAX as u128 {
        return sqrt_u64(val as u64);
    }

    // Initial guess: start high to ensure convergence from above
    let leading_zeros = val.leading_zeros();
    let bit_pos = 127 - leading_zeros;
    let mut x = 1u128 << ((bit_pos / 2) + 1);

    // Newton-Raphson: converges monotonically from above
    loop {
        let x_new = (x + val / x) / 2;
        if x_new >= x {
            break;
        }
        x = x_new;
    }

    // Final adjustment
    while x * x > val {
        x -= 1;
    }

    // Result should fit in u64 for our use cases
    x as u64
}

/// Square root of a scaled price value
/// Input: price scaled by 10^9
/// Output: sqrt(price) scaled by 10^9
///
/// For a scaled value x = X * 10^9:
/// sqrt_scaled(x) = sqrt(X * 10^9) * sqrt(10^9) = sqrt(X * 10^18)
#[inline]
pub fn sqrt_scaled(val: u64) -> u64 {
    if val == 0 {
        return 0;
    }

    // To get sqrt with same scaling, we compute:
    // sqrt(val * PRICE_SCALE) which gives sqrt(val) * sqrt(PRICE_SCALE)
    // Since val is already scaled, this gives us the correct scaling
    let scaled_up = (val as u128) * (PRICE_SCALE as u128);
    sqrt_u128(scaled_up)
}

/// Square root of a u128 value that's already at 10^18 scale
/// (e.g., result of price² where price is scaled by 10^9)
/// Returns result scaled by 10^9
#[inline]
pub fn sqrt_u128_scaled(val: u128) -> u64 {
    if val == 0 {
        return 0;
    }
    sqrt_u128(val)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqrt_u64_zero() {
        assert_eq!(sqrt_u64(0), 0);
    }

    #[test]
    fn test_sqrt_u64_one() {
        assert_eq!(sqrt_u64(1), 1);
    }

    #[test]
    fn test_sqrt_u64_four() {
        assert_eq!(sqrt_u64(4), 2);
    }

    #[test]
    fn test_sqrt_u64_perfect_squares() {
        assert_eq!(sqrt_u64(9), 3);
        assert_eq!(sqrt_u64(16), 4);
        assert_eq!(sqrt_u64(25), 5);
        assert_eq!(sqrt_u64(100), 10);
        assert_eq!(sqrt_u64(10000), 100);
        assert_eq!(sqrt_u64(40401), 201);
    }

    #[test]
    fn test_sqrt_u64_non_perfect() {
        // sqrt(2) ≈ 1.414, floor = 1
        assert_eq!(sqrt_u64(2), 1);
        // sqrt(8) ≈ 2.83, floor = 2
        assert_eq!(sqrt_u64(8), 2);
        // sqrt(99) ≈ 9.95, floor = 9
        assert_eq!(sqrt_u64(99), 9);
    }

    #[test]
    fn test_sqrt_u64_large() {
        // sqrt(10^18) = 10^9
        assert_eq!(sqrt_u64(1_000_000_000_000_000_000), 1_000_000_000);
    }

    #[test]
    fn test_sqrt_u128_large() {
        // sqrt(10^36) = 10^18
        let val: u128 = 1_000_000_000_000_000_000_000_000_000_000_000_000;
        assert_eq!(sqrt_u128(val), 1_000_000_000_000_000_000);
    }

    #[test]
    fn test_sqrt_scaled() {
        // sqrt(4.0 scaled) = 2.0 scaled
        // 4.0 * 10^9 = 4_000_000_000
        // sqrt(4.0) = 2.0
        // 2.0 * 10^9 = 2_000_000_000
        let four_scaled = 4 * PRICE_SCALE;
        let result = sqrt_scaled(four_scaled);
        let expected = 2 * PRICE_SCALE;
        // Allow small error due to integer math
        let error = if result > expected {
            result - expected
        } else {
            expected - result
        };
        assert!(error < expected / 10000, "error: {}, expected: {}", error, expected);
    }

    #[test]
    fn test_sqrt_scaled_large_price() {
        // sqrt(10000.0 scaled) = 100.0 scaled
        // 10000 * 10^9 = 10_000_000_000_000
        let val = 10_000 * PRICE_SCALE;
        let result = sqrt_scaled(val);
        let expected = 100 * PRICE_SCALE;
        let error = if result > expected {
            result - expected
        } else {
            expected - result
        };
        assert!(error < expected / 10000, "result: {}, expected: {}", result, expected);
    }

    #[test]
    fn test_sqrt_scaled_fractional() {
        // sqrt(2.25 scaled) = 1.5 scaled
        // 2.25 * 10^9 = 2_250_000_000
        // 1.5 * 10^9 = 1_500_000_000
        let val = PRICE_SCALE * 2 + PRICE_SCALE / 4; // 2.25
        let result = sqrt_scaled(val);
        let expected = PRICE_SCALE + PRICE_SCALE / 2; // 1.5
        let error = if result > expected {
            result - expected
        } else {
            expected - result
        };
        assert!(error < expected / 10000, "result: {}, expected: {}", result, expected);
    }

    #[test]
    fn test_sqrt_u128_scaled() {
        // sqrt of a squared price (10^18 scale)
        // price = 150 * 10^9 = 150_000_000_000
        // price² = 22500 * 10^18
        // sqrt(price²) = price = 150 * 10^9
        let price = 150 * PRICE_SCALE;
        let price_squared = (price as u128) * (price as u128);
        let result = sqrt_u128_scaled(price_squared);
        assert_eq!(result, price);
    }

    #[test]
    fn test_sqrt_accuracy() {
        // Test accuracy across a range of values
        // All should be within 0.01% of true sqrt
        let test_vals: [u64; 5] = [
            1 * PRICE_SCALE,
            100 * PRICE_SCALE,
            1000 * PRICE_SCALE,
            150_000_000_000, // $150
            18_000_000_000_000, // $18,000 (like BTC)
        ];

        for val in test_vals {
            let result = sqrt_scaled(val);
            // Verify by squaring: (result/10^9)² * 10^9 ≈ val
            let result_squared = (result as u128) * (result as u128) / (PRICE_SCALE as u128);
            let error = if result_squared > val as u128 {
                result_squared - val as u128
            } else {
                val as u128 - result_squared
            };
            let max_error = val as u128 / 10000; // 0.01%
            assert!(
                error <= max_error,
                "sqrt({}) = {}, squared back = {}, error = {}, max = {}",
                val, result, result_squared, error, max_error
            );
        }
    }
}
