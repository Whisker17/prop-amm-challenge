//! Exact 256-bit unsigned integer arithmetic.
//!
//! Every Solidity port in this crate runs on `U256` so that operation order,
//! truncation and overflow behaviour can be reproduced literally. No floating
//! point appears anywhere in this module.
//!
//! Semantics mirror Solidity 0.8 checked arithmetic: the `checked_*` operations
//! return `None` where Solidity would revert, and `wrapping_*` reproduces the
//! `unchecked { ... }` blocks that DODO relies on.

use std::cmp::Ordering;
use std::fmt;

/// Little-endian limbs: `limbs[0]` is the least significant 64 bits.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct U256 {
    limbs: [u64; 4],
}

impl U256 {
    pub const ZERO: U256 = U256 {
        limbs: [0, 0, 0, 0],
    };
    pub const ONE: U256 = U256 {
        limbs: [1, 0, 0, 0],
    };
    pub const MAX: U256 = U256 {
        limbs: [u64::MAX; 4],
    };

    #[inline]
    pub const fn from_u64(value: u64) -> U256 {
        U256 {
            limbs: [value, 0, 0, 0],
        }
    }

    #[inline]
    pub const fn from_u128(value: u128) -> U256 {
        U256 {
            limbs: [value as u64, (value >> 64) as u64, 0, 0],
        }
    }

    #[inline]
    pub const fn from_limbs(limbs: [u64; 4]) -> U256 {
        U256 { limbs }
    }

    #[inline]
    pub const fn limbs(&self) -> [u64; 4] {
        self.limbs
    }

    #[inline]
    pub const fn is_zero(&self) -> bool {
        self.limbs[0] == 0 && self.limbs[1] == 0 && self.limbs[2] == 0 && self.limbs[3] == 0
    }

    #[inline]
    pub const fn fits_u128(&self) -> bool {
        self.limbs[2] == 0 && self.limbs[3] == 0
    }

    #[inline]
    pub const fn fits_u64(&self) -> bool {
        self.limbs[1] == 0 && self.limbs[2] == 0 && self.limbs[3] == 0
    }

    /// Low 128 bits, discarding anything above.
    #[inline]
    pub const fn low_u128(&self) -> u128 {
        (self.limbs[0] as u128) | ((self.limbs[1] as u128) << 64)
    }

    #[inline]
    pub const fn as_u128(&self) -> Option<u128> {
        if self.fits_u128() {
            Some(self.low_u128())
        } else {
            None
        }
    }

    #[inline]
    pub const fn as_u64(&self) -> Option<u64> {
        if self.fits_u64() {
            Some(self.limbs[0])
        } else {
            None
        }
    }

    pub fn bit_len(&self) -> u32 {
        for i in (0..4).rev() {
            if self.limbs[i] != 0 {
                return 64 * i as u32 + (64 - self.limbs[i].leading_zeros());
            }
        }
        0
    }

    // ---------------- addition / subtraction ----------------

    #[inline]
    pub fn overflowing_add(self, other: U256) -> (U256, bool) {
        let mut out = [0u64; 4];
        let mut carry = 0u64;
        for (i, limb) in out.iter_mut().enumerate() {
            let (sum, c1) = self.limbs[i].overflowing_add(other.limbs[i]);
            let (sum, c2) = sum.overflowing_add(carry);
            *limb = sum;
            carry = (c1 as u64) + (c2 as u64);
        }
        (U256 { limbs: out }, carry != 0)
    }

    #[inline]
    pub fn checked_add(self, other: U256) -> Option<U256> {
        let (value, overflow) = self.overflowing_add(other);
        if overflow {
            None
        } else {
            Some(value)
        }
    }

    #[inline]
    pub fn wrapping_add(self, other: U256) -> U256 {
        self.overflowing_add(other).0
    }

    #[inline]
    pub fn overflowing_sub(self, other: U256) -> (U256, bool) {
        let mut out = [0u64; 4];
        let mut borrow = 0u64;
        for (i, limb) in out.iter_mut().enumerate() {
            let (diff, b1) = self.limbs[i].overflowing_sub(other.limbs[i]);
            let (diff, b2) = diff.overflowing_sub(borrow);
            *limb = diff;
            borrow = (b1 as u64) + (b2 as u64);
        }
        (U256 { limbs: out }, borrow != 0)
    }

    #[inline]
    pub fn checked_sub(self, other: U256) -> Option<U256> {
        let (value, borrow) = self.overflowing_sub(other);
        if borrow {
            None
        } else {
            Some(value)
        }
    }

    #[inline]
    pub fn wrapping_sub(self, other: U256) -> U256 {
        self.overflowing_sub(other).0
    }

    // ---------------- multiplication ----------------

    /// Full 512-bit product as `[low limbs, high limbs]` (little-endian, 8 limbs).
    pub fn full_mul(self, other: U256) -> [u64; 8] {
        let mut out = [0u64; 8];
        for i in 0..4 {
            if self.limbs[i] == 0 {
                continue;
            }
            let mut carry = 0u128;
            for j in 0..4 {
                let idx = i + j;
                let cur =
                    out[idx] as u128 + (self.limbs[i] as u128) * (other.limbs[j] as u128) + carry;
                out[idx] = cur as u64;
                carry = cur >> 64;
            }
            let mut idx = i + 4;
            while carry != 0 {
                let cur = out[idx] as u128 + carry;
                out[idx] = cur as u64;
                carry = cur >> 64;
                idx += 1;
            }
        }
        out
    }

    #[inline]
    pub fn checked_mul(self, other: U256) -> Option<U256> {
        let product = self.full_mul(other);
        if product[4] != 0 || product[5] != 0 || product[6] != 0 || product[7] != 0 {
            return None;
        }
        Some(U256 {
            limbs: [product[0], product[1], product[2], product[3]],
        })
    }

    /// Solidity `unchecked` multiplication: the low 256 bits of the product.
    #[inline]
    pub fn wrapping_mul(self, other: U256) -> U256 {
        let product = self.full_mul(other);
        U256 {
            limbs: [product[0], product[1], product[2], product[3]],
        }
    }

    // ---------------- shifts ----------------

    pub fn checked_shl(self, shift: u32) -> Option<U256> {
        if shift == 0 {
            return Some(self);
        }
        if shift >= 256 {
            return if self.is_zero() {
                Some(U256::ZERO)
            } else {
                None
            };
        }
        if self.bit_len() + shift > 256 {
            return None;
        }
        Some(self.wrapping_shl(shift))
    }

    pub fn wrapping_shl(self, shift: u32) -> U256 {
        let shift = shift % 256;
        if shift == 0 {
            return self;
        }
        let limb_shift = (shift / 64) as usize;
        let bit_shift = shift % 64;
        let mut out = [0u64; 4];
        for i in (0..4).rev() {
            if i < limb_shift {
                break;
            }
            let src = i - limb_shift;
            let mut value = self.limbs[src] << bit_shift;
            if bit_shift > 0 && src > 0 {
                value |= self.limbs[src - 1] >> (64 - bit_shift);
            }
            out[i] = value;
        }
        U256 { limbs: out }
    }

    /// Logical right shift. Named `shift_right` rather than `shr` so it cannot
    /// be mistaken for `std::ops::Shr`, which this type deliberately does not
    /// implement (every operation here is explicit).
    pub fn shift_right(self, shift: u32) -> U256 {
        if shift == 0 {
            return self;
        }
        if shift >= 256 {
            return U256::ZERO;
        }
        let limb_shift = (shift / 64) as usize;
        let bit_shift = shift % 64;
        let mut out = [0u64; 4];
        for (i, limb) in out.iter_mut().enumerate() {
            let src = i + limb_shift;
            if src >= 4 {
                break;
            }
            let mut value = self.limbs[src] >> bit_shift;
            if bit_shift > 0 && src + 1 < 4 {
                value |= self.limbs[src + 1] << (64 - bit_shift);
            }
            *limb = value;
        }
        U256 { limbs: out }
    }

    // ---------------- division ----------------

    /// Truncating division and remainder. `None` when the divisor is zero,
    /// matching Solidity's division-by-zero revert (which applies even inside
    /// `unchecked` blocks).
    pub fn checked_div_rem(self, other: U256) -> Option<(U256, U256)> {
        if other.is_zero() {
            return None;
        }
        if self < other {
            return Some((U256::ZERO, self));
        }
        if let Some(divisor) = other.as_u64() {
            let (quotient, remainder) = self.div_rem_small(divisor);
            return Some((quotient, U256::from_u64(remainder)));
        }
        if self.fits_u128() {
            // `other <= self` here, so the divisor fits as well.
            let a = self.low_u128();
            let b = other.low_u128();
            return Some((U256::from_u128(a / b), U256::from_u128(a % b)));
        }
        Some(div_rem_knuth(self, other))
    }

    #[inline]
    pub fn checked_div(self, other: U256) -> Option<U256> {
        self.checked_div_rem(other).map(|(q, _)| q)
    }

    #[inline]
    pub fn checked_rem(self, other: U256) -> Option<U256> {
        self.checked_div_rem(other).map(|(_, r)| r)
    }

    /// Division by a single 64-bit limb.
    pub fn div_rem_small(self, divisor: u64) -> (U256, u64) {
        debug_assert!(divisor != 0);
        let d = divisor as u128;
        let mut remainder: u128 = 0;
        let mut out = [0u64; 4];
        for i in (0..4).rev() {
            let cur = (remainder << 64) | self.limbs[i] as u128;
            out[i] = (cur / d) as u64;
            remainder = cur % d;
        }
        (U256 { limbs: out }, remainder as u64)
    }

    // ---------------- byte encoding ----------------

    /// Little-endian 32-byte encoding (used for the simulation's storage blob).
    pub fn to_le_bytes(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        for i in 0..4 {
            out[i * 8..(i + 1) * 8].copy_from_slice(&self.limbs[i].to_le_bytes());
        }
        out
    }

    /// Decode a little-endian 32-byte value. Shorter slices are zero-extended;
    /// longer slices are truncated to the first 32 bytes.
    pub fn from_le_bytes(bytes: &[u8]) -> U256 {
        let mut limbs = [0u64; 4];
        for (i, limb) in limbs.iter_mut().enumerate() {
            let start = i * 8;
            if start >= bytes.len() {
                break;
            }
            let end = (start + 8).min(bytes.len());
            let mut buf = [0u8; 8];
            buf[..end - start].copy_from_slice(&bytes[start..end]);
            *limb = u64::from_le_bytes(buf);
        }
        U256 { limbs }
    }

    // ---------------- parsing / formatting ----------------

    pub fn from_dec_str(text: &str) -> Option<U256> {
        if text.is_empty() {
            return None;
        }
        let mut value = U256::ZERO;
        let ten = U256::from_u64(10);
        for byte in text.bytes() {
            if !byte.is_ascii_digit() {
                return None;
            }
            value = value.checked_mul(ten)?;
            value = value.checked_add(U256::from_u64((byte - b'0') as u64))?;
        }
        Some(value)
    }

    pub fn from_hex_str(text: &str) -> Option<U256> {
        let body = text
            .strip_prefix("0x")
            .or_else(|| text.strip_prefix("0X"))
            .unwrap_or(text);
        if body.is_empty() {
            return None;
        }
        let mut value = U256::ZERO;
        for byte in body.bytes() {
            let digit = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => return None,
            };
            value = value.checked_shl(4)?;
            value = value.checked_add(U256::from_u64(digit as u64))?;
        }
        Some(value)
    }
}

// ---------------- 512-bit multiply-divide (Uniswap V3 `FullMath`) ----------------

/// Verbatim Rust port of `FullMath.mulDiv`: `floor(a * b / denominator)`
/// computed over the full 512-bit product, so an intermediate that overflows
/// 256 bits does not lose precision.
///
/// Upstream: Uniswap/v3-core `contracts/libraries/FullMath.sol`
/// Commit:   `e3589b192d0be27e100cd0daaf6c97204fdb1899` (tag v1.0.0), solc 0.7.6
/// sha256:   `54087aee268a6938a85a408d7b14481b5c2c956c21508d5583f1bf48ec6d69ba`
///
/// `None` means "the Solidity call would have reverted". Upstream has exactly
/// two revert conditions and both are reproduced branch for branch:
///
/// * `prod1 == 0` (the product fits in 256 bits) and `require(denominator > 0)`
///   fails, i.e. `denominator == 0`;
/// * `prod1 != 0` and `require(denominator > prod1)` fails. That require is the
///   "result fits in a uint256" test — `prod1 * 2**256 <= a * b` means
///   `denominator <= prod1` implies `floor(a * b / denominator) >= 2**256` —
///   and it also rules out `denominator == 0` on this branch.
///
/// The **division itself** is not a transcription of Remco Bloemen's
/// modular-inverse sequence (`twos` factoring + Newton-Raphson inverse). That
/// sequence exists to make a 512-by-256 division cheap in EVM gas; it computes
/// nothing that the exact quotient does not. Here the same quotient is obtained
/// by a 512-bit Knuth algorithm D ([`div_rem_512`]), which is exact for every
/// input, so the returned value and the revert conditions are bit-identical to
/// the pinned Solidity. `tests/u256_reference.rs` checks that against 1 070
/// vectors produced by executing that Solidity.
pub fn mul_div(a: U256, b: U256, denominator: U256) -> Option<U256> {
    // 512-bit multiply [prod1 prod0] = a * b
    let product = a.full_mul(b);
    let prod0 = U256::from_limbs([product[0], product[1], product[2], product[3]]);
    let prod1 = U256::from_limbs([product[4], product[5], product[6], product[7]]);

    // Handle non-overflow cases, 256 by 256 division
    if prod1.is_zero() {
        if denominator.is_zero() {
            return None; // require(denominator > 0)
        }
        return prod0.checked_div(denominator);
    }

    // Make sure the result is less than 2**256. Also prevents denominator == 0.
    if denominator <= prod1 {
        return None; // require(denominator > prod1)
    }

    // 512 by 256 division.
    let (quotient, _remainder) = div_rem_512(product, denominator);
    debug_assert!(
        quotient[4] == 0 && quotient[5] == 0 && quotient[6] == 0 && quotient[7] == 0,
        "require(denominator > prod1) guarantees a 256-bit quotient"
    );
    Some(U256::from_limbs([
        quotient[0],
        quotient[1],
        quotient[2],
        quotient[3],
    ]))
}

/// Verbatim Rust port of `FullMath.mulDivRoundingUp`:
/// `ceil(a * b / denominator)`, same upstream file and commit as [`mul_div`].
///
/// ```text
/// result = mulDiv(a, b, denominator);
/// if (mulmod(a, b, denominator) > 0) {
///     require(result < type(uint256).max);
///     result++;
/// }
/// ```
///
/// So there are three revert conditions: the two inherited from [`mul_div`],
/// plus `result == type(uint256).max` with a non-zero remainder (the increment
/// would overflow). `mul_div` runs first and has already rejected
/// `denominator == 0`, so the `mulmod` below always has a non-zero modulus.
pub fn mul_div_rounding_up(a: U256, b: U256, denominator: U256) -> Option<U256> {
    let result = mul_div(a, b, denominator)?;
    // `mul_div` returned `Some`, so `denominator != 0` and `mul_mod` cannot be
    // `None` here.
    if !mul_mod(a, b, denominator)?.is_zero() {
        if result == U256::MAX {
            return None; // require(result < type(uint256).max)
        }
        return result.checked_add(U256::ONE);
    }
    Some(result)
}

/// The EVM `mulmod(a, b, m)` opcode for a non-zero modulus: the exact
/// remainder of the 512-bit product `a * b` modulo `m`, with no intermediate
/// truncation.
///
/// Returns `None` for `m == 0`. **This is deliberately not the opcode's own
/// behaviour**: `MULMOD` with a zero modulus pushes `0` (and solc >= 0.8 adds a
/// `Panic(0x12)` check on top, which solc 0.7.6 — the version v3-core is
/// compiled with — does not). `FullMath.mulDivRoundingUp` only reaches `mulmod`
/// after `mulDiv` has already reverted for `denominator == 0`, so the
/// difference is unobservable there; `None` is used here so that a caller which
/// is *not* FullMath cannot silently read a zero-modulus result as `0`.
pub fn mul_mod(a: U256, b: U256, m: U256) -> Option<U256> {
    if m.is_zero() {
        return None;
    }
    let (_quotient, remainder) = div_rem_512(a.full_mul(b), m);
    Some(remainder)
}

impl PartialOrd for U256 {
    #[inline]
    fn partial_cmp(&self, other: &U256) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for U256 {
    #[inline]
    fn cmp(&self, other: &U256) -> Ordering {
        // Compare most-significant limb first; a derived impl would be wrong.
        for i in (0..4).rev() {
            match self.limbs[i].cmp(&other.limbs[i]) {
                Ordering::Equal => continue,
                non_equal => return non_equal,
            }
        }
        Ordering::Equal
    }
}

impl fmt::Display for U256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_zero() {
            return f.write_str("0");
        }
        // 10^19 is the largest power of ten below 2^64.
        const CHUNK: u64 = 10_000_000_000_000_000_000;
        let mut chunks: Vec<u64> = Vec::with_capacity(5);
        let mut value = *self;
        while !value.is_zero() {
            let (quotient, remainder) = value.div_rem_small(CHUNK);
            chunks.push(remainder);
            value = quotient;
        }
        let mut out = String::with_capacity(chunks.len() * 19);
        out.push_str(&chunks.last().unwrap().to_string());
        for chunk in chunks.iter().rev().skip(1) {
            out.push_str(&format!("{chunk:019}"));
        }
        f.write_str(&out)
    }
}

impl fmt::Debug for U256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

// ---------------- Knuth algorithm D ----------------

#[inline]
fn shl_in(low: u32, high: u32, shift: u32) -> u32 {
    if shift == 0 {
        low
    } else {
        (low << shift) | (high >> (32 - shift))
    }
}

#[inline]
fn shr_out(low: u32, high: u32, shift: u32) -> u32 {
    if shift == 0 {
        low
    } else {
        (low >> shift) | (high << (32 - shift))
    }
}

fn to_u32_limbs(value: U256) -> ([u32; 8], usize) {
    let limbs = value.limbs();
    let mut out = [0u32; 8];
    for i in 0..4 {
        out[2 * i] = limbs[i] as u32;
        out[2 * i + 1] = (limbs[i] >> 32) as u32;
    }
    let mut len = 8;
    while len > 1 && out[len - 1] == 0 {
        len -= 1;
    }
    (out, len)
}

fn from_u32_limbs(limbs: &[u32]) -> U256 {
    let mut out = [0u64; 4];
    for (i, limb) in limbs.iter().enumerate().take(8) {
        out[i / 2] |= (*limb as u64) << (32 * (i % 2));
    }
    U256::from_limbs(out)
}

/// Knuth algorithm D (TAOCP 4.3.1) on 32-bit limbs. The caller guarantees a
/// non-zero divisor with at least three significant 32-bit limbs and
/// `dividend >= divisor`.
fn div_rem_knuth(dividend: U256, divisor: U256) -> (U256, U256) {
    let (u_limbs, m) = to_u32_limbs(dividend);
    let (v_limbs, n) = to_u32_limbs(divisor);
    let mut quotient = [0u32; 8];
    let mut remainder = [0u32; 8];
    knuth_div_u32(&u_limbs, m, &v_limbs, n, &mut quotient, &mut remainder);
    (from_u32_limbs(&quotient), from_u32_limbs(&remainder))
}

/// Knuth algorithm D (TAOCP 4.3.1) on 32-bit limbs, shared by the
/// 256-by-256 ([`div_rem_knuth`]) and 512-by-256 ([`div_rem_512`]) divisions.
///
/// `u_limbs` holds `m` significant limbs, `v_limbs` holds `n >= 2` significant
/// limbs, and the caller guarantees `u >= v` (hence `m >= n`). The `m - n + 1`
/// quotient limbs are written to `q_out` and the `n` remainder limbs to
/// `r_out`; higher limbs of either output are left as the caller set them.
///
/// Bounds: `m <= 16` (a 512-bit dividend) and `n <= 8` (a 256-bit divisor).
fn knuth_div_u32(
    u_limbs: &[u32],
    m: usize,
    v_limbs: &[u32],
    n: usize,
    q_out: &mut [u32],
    r_out: &mut [u32],
) {
    const BASE: u64 = 1 << 32;

    debug_assert!(n >= 2 && m >= n && m <= 16 && n <= 8);
    // `m - n + 1` is the quotient limb count Knuth D produces, not an off-by-one
    // dressed up as a comparison; `> m - n` would say the same thing while hiding
    // where the number comes from.
    #[allow(clippy::int_plus_one)]
    {
        debug_assert!(q_out.len() >= m - n + 1 && r_out.len() >= n);
    }

    let shift = v_limbs[n - 1].leading_zeros();

    // Normalised divisor.
    let mut vn = [0u32; 8];
    for i in (1..n).rev() {
        vn[i] = shl_in(v_limbs[i], v_limbs[i - 1], shift);
    }
    vn[0] = v_limbs[0] << shift;

    // Normalised dividend with one extra high limb.
    let mut un = [0u32; 17];
    un[m] = if shift == 0 {
        0
    } else {
        v_limbs_shift_high(u_limbs[m - 1], shift)
    };
    for i in (1..m).rev() {
        un[i] = shl_in(u_limbs[i], u_limbs[i - 1], shift);
    }
    un[0] = u_limbs[0] << shift;

    for j in (0..=(m - n)).rev() {
        let numerator = ((un[j + n] as u64) << 32) | (un[j + n - 1] as u64);
        let mut qhat = numerator / (vn[n - 1] as u64);
        let mut rhat = numerator % (vn[n - 1] as u64);

        loop {
            if qhat >= BASE || qhat * (vn[n - 2] as u64) > (rhat << 32) + (un[j + n - 2] as u64) {
                qhat -= 1;
                rhat += vn[n - 1] as u64;
                if rhat < BASE {
                    continue;
                }
            }
            break;
        }

        // Multiply and subtract.
        let mut borrow: i64 = 0;
        let mut diff: i64;
        for i in 0..n {
            let product = qhat * (vn[i] as u64);
            diff = (un[i + j] as i64) - borrow - ((product & 0xffff_ffff) as i64);
            un[i + j] = diff as u32;
            borrow = ((product >> 32) as i64) - (diff >> 32);
        }
        diff = (un[j + n] as i64) - borrow;
        un[j + n] = diff as u32;

        q_out[j] = qhat as u32;
        if diff < 0 {
            // qhat was one too large: add the divisor back.
            q_out[j] = q_out[j].wrapping_sub(1);
            let mut carry: u64 = 0;
            for i in 0..n {
                let sum = (un[i + j] as u64) + (vn[i] as u64) + carry;
                un[i + j] = sum as u32;
                carry = sum >> 32;
            }
            un[j + n] = ((un[j + n] as u64).wrapping_add(carry)) as u32;
        }
    }

    // Denormalise the remainder.
    for (i, out) in r_out.iter_mut().enumerate().take(n) {
        *out = shr_out(un[i], un[i + 1], shift);
    }
}

#[inline]
fn v_limbs_shift_high(limb: u32, shift: u32) -> u32 {
    debug_assert!(shift > 0 && shift < 32);
    limb >> (32 - shift)
}

/// 512-by-256 truncating division, the dividend being the little-endian 8-limb
/// product produced by [`U256::full_mul`]. Returns `(quotient, remainder)`,
/// where the quotient is itself up to 512 bits wide (callers that require a
/// 256-bit quotient must establish that themselves — [`mul_div`] does it with
/// `require(denominator > prod1)`).
///
/// The caller guarantees `divisor != 0`.
///
/// Three paths, in the same spirit as [`U256::checked_div_rem`]:
/// a single-`u64` divisor is handled by schoolbook long division over the eight
/// limbs; a dividend that fits in 256 bits is delegated to the 256-bit divider;
/// everything else goes through Knuth D with a 16-limb dividend.
fn div_rem_512(dividend: [u64; 8], divisor: U256) -> ([u64; 8], U256) {
    debug_assert!(!divisor.is_zero());

    if let Some(small) = divisor.as_u64() {
        let d = small as u128;
        let mut remainder: u128 = 0;
        let mut quotient = [0u64; 8];
        for i in (0..8).rev() {
            let current = (remainder << 64) | dividend[i] as u128;
            quotient[i] = (current / d) as u64;
            remainder = current % d;
        }
        return (quotient, U256::from_u128(remainder));
    }

    let low = U256::from_limbs([dividend[0], dividend[1], dividend[2], dividend[3]]);
    let high = U256::from_limbs([dividend[4], dividend[5], dividend[6], dividend[7]]);
    if high.is_zero() {
        // The product fits in 256 bits: reuse the 256-bit divider.
        let (quotient, remainder) = low
            .checked_div_rem(divisor)
            .expect("divisor is non-zero by contract");
        let q = quotient.limbs();
        return ([q[0], q[1], q[2], q[3], 0, 0, 0, 0], remainder);
    }

    // `high != 0` gives at least 9 significant 32-bit dividend limbs, and the
    // divisor has at most 8 (at least 3, since the `as_u64` path is taken
    // first), so `m > n >= 3` here and the dividend is strictly the larger.
    let (u_limbs, m) = to_u32_limbs_512(dividend);
    let (v_limbs, n) = to_u32_limbs(divisor);
    let mut quotient = [0u32; 16];
    let mut remainder = [0u32; 8];
    knuth_div_u32(&u_limbs, m, &v_limbs, n, &mut quotient, &mut remainder);
    (from_u32_limbs_512(&quotient), from_u32_limbs(&remainder))
}

fn to_u32_limbs_512(value: [u64; 8]) -> ([u32; 16], usize) {
    let mut out = [0u32; 16];
    for i in 0..8 {
        out[2 * i] = value[i] as u32;
        out[2 * i + 1] = (value[i] >> 32) as u32;
    }
    let mut len = 16;
    while len > 1 && out[len - 1] == 0 {
        len -= 1;
    }
    (out, len)
}

fn from_u32_limbs_512(limbs: &[u32]) -> [u64; 8] {
    let mut out = [0u64; 8];
    for (i, limb) in limbs.iter().enumerate().take(16) {
        out[i / 2] |= (*limb as u64) << (32 * (i % 2));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(text: &str) -> U256 {
        U256::from_dec_str(text).expect("decimal literal")
    }

    #[test]
    fn add_sub_edges() {
        assert_eq!(U256::MAX.checked_add(U256::ONE), None);
        assert_eq!(U256::ZERO.checked_sub(U256::ONE), None);
        assert_eq!(U256::MAX.wrapping_add(U256::ONE), U256::ZERO);
        assert_eq!(U256::ZERO.wrapping_sub(U256::ONE), U256::MAX);
        assert_eq!(
            dec("18446744073709551615").checked_add(U256::ONE).unwrap(),
            dec("18446744073709551616")
        );
    }

    #[test]
    fn mul_edges() {
        assert_eq!(U256::MAX.checked_mul(U256::from_u64(2)), None);
        assert_eq!(
            U256::MAX.wrapping_mul(U256::from_u64(2)),
            U256::MAX.wrapping_sub(U256::ONE)
        );
        assert_eq!(
            dec("1000000000000000000")
                .checked_mul(dec("1000000000000000000"))
                .unwrap(),
            dec("1000000000000000000000000000000000000")
        );
    }

    #[test]
    fn div_paths_agree() {
        // Single-limb divisor path.
        assert_eq!(
            dec("1000000000000000000000000000000000000")
                .checked_div(dec("1000000000000000000"))
                .unwrap(),
            dec("1000000000000000000")
        );
        // u128 fast path.
        assert_eq!(
            dec("340282366920938463463374607431768211455")
                .checked_div(dec("18446744073709551617"))
                .unwrap(),
            dec("18446744073709551615")
        );
        // Knuth path: 256-bit dividend, >64-bit divisor.
        let a =
            dec("115792089237316195423570985008687907853269984665640564039457584007913129639935");
        let b = dec("340282366920938463463374607431768211455");
        assert_eq!(
            a.checked_div(b).unwrap(),
            dec("340282366920938463463374607431768211457")
        );
        assert_eq!(a.checked_rem(b).unwrap(), dec("0"));
    }

    #[test]
    fn shifts() {
        assert_eq!(U256::ONE.checked_shl(255).unwrap().bit_len(), 256);
        assert_eq!(U256::ONE.checked_shl(256), None);
        assert_eq!(U256::MAX.checked_shl(1), None);
        assert_eq!(
            U256::ONE.checked_shl(64).unwrap(),
            dec("18446744073709551616")
        );
        assert_eq!(dec("18446744073709551616").shift_right(64), U256::ONE);
        assert_eq!(U256::MAX.shift_right(255), U256::ONE);
        assert_eq!(U256::MAX.shift_right(256), U256::ZERO);
    }

    #[test]
    fn display_matches_decimal_input() {
        for text in [
            "0",
            "1",
            "9",
            "10",
            "1000000000000000000",
            "9999999999999999999",
            "10000000000000000000",
            "115792089237316195423570985008687907853269984665640564039457584007913129639935",
        ] {
            assert_eq!(dec(text).to_string(), text);
        }
    }
}
