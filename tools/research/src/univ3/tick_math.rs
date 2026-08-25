//! Verbatim Rust port of Uniswap V3 `TickMath`.
//!
//! Upstream: Uniswap/v3-core `contracts/libraries/TickMath.sol`
//! Commit:   e3589b192d0be27e100cd0daaf6c97204fdb1899 (tag `v1.0.0`)
//! sha256:   83cf64b2ca84001effd16e007b49bac5359143b6c3132bfe42907b2426a0c5f5
//! Licence:  GPL-2.0-or-later (per the file's own SPDX header)
//!
//! Every magic constant, every branch and every rounding step is transcribed
//! as written. Note that upstream compiles at Solidity **0.7.6**, where
//! arithmetic *wraps* — so the multiplications here use `wrapping_mul`, which is
//! what the EVM actually does, rather than checked arithmetic.
//!
//! `None` is returned exactly where the Solidity reverts (`require(... , 'T')`
//! and `require(..., 'R')`).

use super::bits;
use crate::u256::U256;

/// `int24 internal constant MIN_TICK = -887272;`
pub const MIN_TICK: i32 = -887_272;
/// `int24 internal constant MAX_TICK = -MIN_TICK;`
pub const MAX_TICK: i32 = -MIN_TICK;

/// `uint160 internal constant MIN_SQRT_RATIO = 4295128739;`
pub const MIN_SQRT_RATIO: U256 = U256::from_u128(4_295_128_739);
/// `uint160 internal constant MAX_SQRT_RATIO = 1461446703485210103287273052203988822378723970342;`
///
/// 160 bits wide, so it is written as limbs rather than a `u128` literal.
/// `tests::max_sqrt_ratio_limbs_are_the_documented_decimal` pins the decimal.
pub const MAX_SQRT_RATIO: U256 = U256::from_limbs([
    0x5d95_1d52_6398_8d26,
    0xefd1_fc6a_5064_8849,
    0x0000_0000_fffd_8963,
    0x0000_0000_0000_0000,
]);

/// `0x100000000000000000000000000000000`, i.e. `1 << 128`.
const Q128: U256 = U256::from_limbs([0, 0, 1, 0]);

/// `getSqrtRatioAtTick(int24 tick)`
///
/// Returns `None` where upstream reverts with `'T'`.
pub fn get_sqrt_ratio_at_tick(tick: i32) -> Option<U256> {
    let abs_tick = (tick as i64).unsigned_abs();
    if abs_tick > MAX_TICK as u64 {
        return None; // require(absTick <= uint256(MAX_TICK), 'T')
    }

    // The constant chain, in upstream order. Each step is `(ratio * C) >> 128`
    // with 0.7.6 wrapping semantics.
    let mut ratio = if abs_tick & 0x1 != 0 {
        U256::from_u128(0xfffc_b933_bd6f_ad37_aa2d_162d_1a59_4001)
    } else {
        Q128
    };
    const STEPS: [(u64, u128); 19] = [
        (0x2, 0xfff9_7272_373d_4132_59a4_6990_580e_213a),
        (0x4, 0xfff2_e50f_5f65_6932_ef12_357c_f3c7_fdcc),
        (0x8, 0xffe5_caca_7e10_e4e6_1c36_24ea_a094_1cd0),
        (0x10, 0xffcb_9843_d60f_6159_c9db_5883_5c92_6644),
        (0x20, 0xff97_3b41_fa98_c081_472e_6896_dfb2_54c0),
        (0x40, 0xff2e_a164_66c9_6a38_43ec_78b3_26b5_2861),
        (0x80, 0xfe5d_ee04_6a99_a2a8_11c4_61f1_969c_3053),
        (0x100, 0xfcbe_86c7_900a_88ae_dcff_c83b_479a_a3a4),
        (0x200, 0xf987_a725_3ac4_1317_6f2b_074c_f781_5e54),
        (0x400, 0xf339_2b08_22b7_0005_940c_7a39_8e4b_70f3),
        (0x800, 0xe715_9475_a2c2_9b74_43b2_9c7f_a6e8_89d9),
        (0x1000, 0xd097_f3bd_fd20_22b8_845a_d8f7_92aa_5825),
        (0x2000, 0xa9f7_4646_2d87_0fdf_8a65_dc1f_90e0_61e5),
        (0x4000, 0x70d8_69a1_56d2_a1b8_90bb_3df6_2baf_32f7),
        (0x8000, 0x31be_135f_97d0_8fd9_8123_1505_542f_cfa6),
        (0x10000, 0x09aa_508b_5b7a_84e1_c677_de54_f3e9_9bc9),
        (0x20000, 0x005d_6af8_dedb_8119_6699_c329_225e_e604),
        (0x40000, 0x0000_2216_e584_f5fa_1ea9_2604_1bed_fe98),
        (0x80000, 0x0000_0000_048a_1703_91f7_dc42_444e_8fa2),
    ];
    // The table above was extracted programmatically from the pinned
    // TickMath.sol rather than transcribed by hand, and each constant is
    // zero-padded to 32 hex digits so a dropped or added nibble is visible.
    for (bit, constant) in STEPS {
        if abs_tick & bit != 0 {
            ratio = ratio
                .wrapping_mul(U256::from_u128(constant))
                .shift_right(128);
        }
    }

    if tick > 0 {
        // ratio = type(uint256).max / ratio
        ratio = U256::MAX.checked_div(ratio)?;
    }

    // (ratio >> 32) + (ratio % (1 << 32) == 0 ? 0 : 1), downcast to uint160
    let shifted = ratio.shift_right(32);
    let remainder = bits::and(ratio, U256::from_u64(u32::MAX as u64));
    let rounded = if remainder.is_zero() {
        shifted
    } else {
        shifted.checked_add(U256::ONE)?
    };
    Some(bits::to_uint160_truncating(rounded))
}

/// `getTickAtSqrtRatio(uint160 sqrtPriceX96)`
///
/// Returns `None` where upstream reverts with `'R'`. Note the asymmetry that is
/// easy to get wrong: the lower bound is inclusive and the upper bound is
/// exclusive.
pub fn get_tick_at_sqrt_ratio(sqrt_price_x96: U256) -> Option<i32> {
    if sqrt_price_x96 < MIN_SQRT_RATIO || sqrt_price_x96 >= MAX_SQRT_RATIO {
        return None; // require(sqrtPriceX96 >= MIN_SQRT_RATIO && sqrtPriceX96 < MAX_SQRT_RATIO, 'R')
    }
    let ratio = sqrt_price_x96.checked_shl(32)?;

    // --- most significant bit, as the assembly block computes it ---
    let mut r = ratio;
    let mut msb: u32 = 0;
    const MSB_STEPS: [(u32, u32); 8] = [
        (7, 128),
        (6, 64),
        (5, 32),
        (4, 16),
        (3, 8),
        (2, 4),
        (1, 2),
        (0, 1),
    ];
    for (shift, width) in MSB_STEPS {
        // f = shl(shift, gt(r, 2^width - 1))
        let threshold = if width == 128 {
            Q128.wrapping_sub(U256::ONE)
        } else {
            U256::from_u128((1u128 << width) - 1)
        };
        let f = if r > threshold { 1u32 << shift } else { 0 };
        msb |= f;
        if shift > 0 {
            r = r.shift_right(f);
        }
    }

    r = if msb >= 128 {
        ratio.shift_right(msb - 127)
    } else {
        ratio.checked_shl(127 - msb)?
    };

    // int256 log_2 = (int256(msb) - 128) << 64, kept as raw bits.
    let msb_signed = U256::from_u64(msb as u64).wrapping_sub(U256::from_u64(128));
    let mut log_2 = msb_signed.wrapping_shl(64);

    // 14 refinement rounds, each contributing one fractional bit.
    for bit in (50..=63).rev() {
        r = r.wrapping_mul(r).shift_right(127);
        let f = r.shift_right(128);
        log_2 = bits::or(log_2, f.wrapping_shl(bit));
        // The final round in upstream omits the `r := shr(f, r)` update.
        if bit > 50 {
            let shift = if f.is_zero() { 0 } else { 1 };
            r = r.shift_right(shift);
        }
    }

    // int256 log_sqrt10001 = log_2 * 255738958999603826347141;
    let log_sqrt10001 = log_2.wrapping_mul(U256::from_u128(255_738_958_999_603_826_347_141));

    let tick_low = bits::to_int24(bits::sar(
        log_sqrt10001.wrapping_sub(U256::from_u128(
            3_402_992_956_809_132_418_596_140_100_660_247_210,
        )),
        128,
    ));
    let tick_hi = bits::to_int24(bits::sar(
        log_sqrt10001.wrapping_add(U256::from_u128(
            291_339_464_771_989_622_907_027_621_153_398_088_495,
        )),
        128,
    ));

    if tick_low == tick_hi {
        Some(tick_low)
    } else if get_sqrt_ratio_at_tick(tick_hi)? <= sqrt_price_x96 {
        Some(tick_hi)
    } else {
        Some(tick_low)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact sqrt price for price 100: `10 * 2^96`, which is exact because
    /// `sqrt(100) = 10`.
    fn sqrt_price_100() -> U256 {
        U256::from_u128(792_281_625_142_643_375_935_439_503_360)
    }

    #[test]
    fn max_sqrt_ratio_limbs_are_the_documented_decimal() {
        assert_eq!(
            MAX_SQRT_RATIO.to_string(),
            "1461446703485210103287273052203988822378723970342"
        );
        assert_eq!(MIN_SQRT_RATIO.to_string(), "4295128739");
        assert_eq!(MAX_SQRT_RATIO.bit_len(), 160);
    }

    #[test]
    fn declared_constants_are_reproduced_by_the_function() {
        // MIN_SQRT_RATIO / MAX_SQRT_RATIO are documented upstream as exactly
        // getSqrtRatioAtTick(MIN_TICK) / (MAX_TICK).
        assert_eq!(get_sqrt_ratio_at_tick(MIN_TICK).unwrap(), MIN_SQRT_RATIO);
        assert_eq!(get_sqrt_ratio_at_tick(MAX_TICK).unwrap(), MAX_SQRT_RATIO);
    }

    #[test]
    fn tick_zero_is_one_in_q96() {
        assert_eq!(get_sqrt_ratio_at_tick(0).unwrap(), U256::from_u128(1 << 96));
    }

    #[test]
    fn out_of_range_ticks_revert() {
        assert_eq!(get_sqrt_ratio_at_tick(MAX_TICK + 1), None);
        assert_eq!(get_sqrt_ratio_at_tick(MIN_TICK - 1), None);
    }

    #[test]
    fn price_100_maps_to_tick_46054() {
        assert_eq!(get_tick_at_sqrt_ratio(sqrt_price_100()).unwrap(), 46054);
    }

    #[test]
    fn sqrt_ratio_bounds_bracket_the_tick() {
        // getSqrtRatioAtTick(t) <= p < getSqrtRatioAtTick(t + 1)
        let p = sqrt_price_100();
        let t = get_tick_at_sqrt_ratio(p).unwrap();
        assert!(get_sqrt_ratio_at_tick(t).unwrap() <= p);
        assert!(get_sqrt_ratio_at_tick(t + 1).unwrap() > p);
    }

    #[test]
    fn round_trip_holds_across_the_tick_domain() {
        for tick in [
            MIN_TICK,
            MIN_TICK + 1,
            -100_000,
            -46_054,
            -1,
            0,
            1,
            46_054,
            100_000,
            MAX_TICK - 1,
        ] {
            let ratio = get_sqrt_ratio_at_tick(tick).unwrap();
            assert_eq!(
                get_tick_at_sqrt_ratio(ratio).unwrap(),
                tick,
                "round trip failed at tick {tick}"
            );
        }
    }

    #[test]
    fn tick_domain_bounds_are_asymmetric() {
        // min inclusive, max exclusive
        assert!(get_tick_at_sqrt_ratio(MIN_SQRT_RATIO).is_some());
        assert_eq!(
            get_tick_at_sqrt_ratio(MIN_SQRT_RATIO.wrapping_sub(U256::ONE)),
            None
        );
        assert_eq!(get_tick_at_sqrt_ratio(MAX_SQRT_RATIO), None);
        assert!(get_tick_at_sqrt_ratio(MAX_SQRT_RATIO.wrapping_sub(U256::ONE)).is_some());
    }

    #[test]
    fn monotonic_in_tick() {
        let mut previous = U256::ZERO;
        for tick in (-887_000..=887_000).step_by(7919) {
            let ratio = get_sqrt_ratio_at_tick(tick).unwrap();
            assert!(ratio > previous, "not monotonic at {tick}");
            previous = ratio;
        }
    }
}
