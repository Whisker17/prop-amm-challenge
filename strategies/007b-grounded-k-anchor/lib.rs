use pinocchio::{account_info::AccountInfo, entrypoint, pubkey::Pubkey, ProgramResult};
use prop_amm_submission_sdk::{set_return_data_bytes, set_return_data_u64, set_storage};

const NAME: &str = "007b Grounded K Anchor";
// The k<1 after_swap path is the mechanism change (recursive i_old*R_f -> raw-ratio
// re-grounding for every K_BPS), so this names the model that wrote this variant rather
// than the parent's preserved source constant (NOTES.md § Fidelity self-assessment).
const MODEL_USED: &str = "Claude Sonnet 5";

// ============================================================================
// 007b Grounded K Anchor — a docs/DESIGN.md §2.9 variant of `007-dodo-pmm` (WHI-1272).
//
// Parent source: DODOEX/contractV2 @ 2f1bcdac7ef1beee7599a756e2eed26732c2536d
// (docs/references/007-dodo-pmm/{PMMPricing.sol, DODOMath.sol}). Same PMM quote, same
// R = ONE collapse, same fee-on-output as the parent; the only mechanism change is the
// after_swap anchor update (below). No new `docs/references/` directory — the parent's
// remains the mechanism source, same pattern as `003b`.
//
// MECHANISM. State is an anchor price `i` (quote per base, fixed point at P_SCALE) and a
// curvature `k` (K_BPS/K_DEN, in (0,1]). At R=ONE both sides' targets equal their live
// reserves (B0=B, Q0=Q), so `_SolveQuadraticFunctionForTrade`'s general V0/V1 collapses to
// a single reserve `V` on the side being solved for:
//
//   (1-k)*V2^2 + b*V2 - k*V0^2 = 0,  -b = (1-k)*V - k*V - i*delta = V*(1-2k) - i*delta
//   V2 = (-b + sqrt(b^2 + 4*(1-k)*k*V^2)) / (2*(1-k))
//   output = V - V2, floored (V2 ceiled) so rounding always favors the pool
//
// k=1 collapses to the exact zero-fee CPMM output `fair*V/(V+fair)` (PMMPricing's own
// `k==ONE` special case) — this is the containment anchor against `001-cpmm-fee`'s CPMM
// family (parent NOTES.md § Step 0.5, "3. Containment demonstration").
//
// ANCHOR UPDATE (after_swap, no sqrt). One family, every K_BPS: write
// `raw_ratio_price(rx_post, ry_post)` from current reserves only. The parent special-cased
// this behind `if K_BPS >= K_DEN` and kept `i_old * R_f` / `i_old / R_f` for interior k;
// that recursive form has no mechanism to self-correct (parent NOTES.md § A runaway
// anchor, found and fixed). This variant deletes those helpers rather than conditionalising
// them, so k is a depth-profile knob around a CPMM-like mid. The k=1 storage write is
// bit-identical to the parent (same clamped raw_ratio_price). Periodic / EWMA re-grounding
// is forbidden — N / the weight would be an invented searched parameter.
//
// SHAPE SAFETY. `solve_quadratic_for_trade` reads only storage-derived state (the anchor
// price) plus compile-time consts and the live reserves passed alongside `input_amount` —
// never `input_amount` itself for anything but `delta`, so the curve quoted for a given
// state is a pure function of trade size: monotone and concave in input for k in (0,1].
// Interior-k numerical guards (dust fallback to the k=1 closed form below 0.001 tokens;
// multiply-first disc_term; empty-pool instead of a V2>V1 0-hole) do not change the k=1
// path and are not a second re-grounding family. The k<1 quadratic's in-range return is
// still `v - v_out`, same as the parent — no k=0-style `min(fair, v)` cap on the
// normal path.
//
// PARITY. The BPF tag-2 path decodes storage from data[42..], runs the SAME after_swap on
// a local buffer, then persists via set_storage — identical to `005`'s pattern.
// ============================================================================

// ---- fixed-point scales -----------------------------------------------------
const P_SCALE: u128 = 1_000_000_000; // price fixed point: i_fp = ry*P_SCALE/rx (quote/base)
const K_DEN: u128 = 10_000; // curvature denominator: k = K_BPS/K_DEN

// ---- overflow guards (docs/DESIGN.md §2.9 cross-cutting finding #7) --------
// Working reserves are clamped here, never saturated into the math below: at u128, squaring
// an unclamped u64::MAX reserve overflows well before the quadratic's discriminant is
// formed. 2^59 nano (~5.8e8 tokens) is unreachable in real simulations (starting reserves
// are O(1e2-1e4) tokens) and only guards `validate`'s synthetic random-byte reserve probes.
const RESERVE_CLAMP: u128 = 1u128 << 59;
// Cap the fair-value input (i*delta/P_SCALE) at this multiple of the reserve being solved
// for, returning the cap-point output for anything larger — a terminal plateau, monotone by
// construction, not a truncation.
const INPUT_CAP_MULT: u128 = 16;
// Interior-k only: below this reserve the quadratic's b_sig flip is not monotone, so
// solve_quadratic_for_trade falls back to the k=1 closed form. 1e6 nano = 0.001 tokens.
// Not a PARAMS search dimension.
const DUST_RESERVE: u128 = 1_000_000;

// ---- searched parameters -----------------------------------------------------
// K_BPS: curvature, in units of 1e-4 of ONE (k = K_BPS/K_DEN). FEE_BPS: fee on the output,
// bps of 10_000 — same convention and frozen range as 001-cpmm-fee's own FEE_BPS.
// WHI-1273: these values are the 300-point search's own winner (K_BPS upper bound;
// FEE_BPS is the search-fitted fee, coincidentally equal to 001@66, not copied from it).
// === PARAMS BEGIN ===
const K_BPS: u128 = 10_000; // range: 25..=10000
const FEE_BPS: u128 = 66; // range: 1..=500
                          // === PARAMS END ===

// ---- storage byte layout (1024 bytes total, little-endian) ------------------
//   [0..8]   MAGIC          u64   — "initialized" sentinel
//   [8..24]  anchor_price   u128  — last PMM mid (quote-per-base, P_SCALE fixed point)
//   [24..1024] unused (zeroed)
const OFF_MAGIC: usize = 0;
const OFF_ANCHOR_PRICE: usize = 8;
const STATE_END: usize = 24; // last byte offset we touch (exclusive)
const MAGIC: u64 = 0x444F_444F_5F50_4D4D; // "DODO_PMM"-ish sentinel

#[cfg(not(feature = "no-entrypoint"))]
entrypoint!(process_instruction);

pub fn process_instruction(_pid: &Pubkey, _a: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.is_empty() {
        return Ok(());
    }
    match data[0] {
        0 | 1 => set_return_data_u64(compute_swap(data)),
        2 => {
            // BPF mirror of native after_swap: decode current storage from the
            // instruction, run the SAME logic on a local buffer, then persist via
            // set_storage so the anchor updates identically on native and BPF.
            if data.len() >= 42 + 1024 {
                let mut s = [0u8; 1024];
                s.copy_from_slice(&data[42..42 + 1024]);
                after_swap(data, &mut s);
                let _ = set_storage(&s);
            }
        }
        3 => set_return_data_bytes(NAME.as_bytes()),
        4 => set_return_data_bytes(get_model_used().as_bytes()),
        _ => {}
    }
    Ok(())
}

pub fn get_model_used() -> &'static str {
    MODEL_USED
}

// ---- little-endian helpers --------------------------------------------------
#[inline]
fn rd_u64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes([
        b[o],
        b[o + 1],
        b[o + 2],
        b[o + 3],
        b[o + 4],
        b[o + 5],
        b[o + 6],
        b[o + 7],
    ])
}

#[inline]
fn rd8(b: &[u8], o: usize) -> u128 {
    rd_u64(b, o) as u128
}

#[inline]
fn rd16(b: &[u8], o: usize) -> u128 {
    u128::from_le_bytes([
        b[o],
        b[o + 1],
        b[o + 2],
        b[o + 3],
        b[o + 4],
        b[o + 5],
        b[o + 6],
        b[o + 7],
        b[o + 8],
        b[o + 9],
        b[o + 10],
        b[o + 11],
        b[o + 12],
        b[o + 13],
        b[o + 14],
        b[o + 15],
    ])
}

#[inline]
fn wr_u64(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 8].copy_from_slice(&v.to_le_bytes());
}

#[inline]
fn wr16(b: &mut [u8], o: usize, v: u128) {
    b[o..o + 16].copy_from_slice(&v.to_le_bytes());
}

// ---- integer sqrt (Newton, u128, seeded from leading_zeros) -----------------
// PMM's discriminant reaches ~1e38 (vs. `005`'s ~1e4 R2_CAP), where a naive `x0 = n` seed
// needs ~63 halving iterations (~126 u128 divisions, ~1.3e5 CU on SBF — over the 100_000 CU
// limit; docs/DESIGN.md §2.9 cross-cutting finding #9). Seeding from `leading_zeros` starts
// within roughly a factor of sqrt(2) of the true root, converging in <= 8 iterations.
#[inline]
fn isqrt(n: u128) -> u128 {
    if n < 2 {
        return n;
    }
    let lz = n.leading_zeros();
    let mut x = 1u128 << ((128 - lz + 1) / 2);
    let mut y = (x + n / x) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

// Scale a discriminant into whatever leading-zero headroom it has left in u128 before taking
// its square root, so `isqrt`'s inherent +/-1-unit floor error lands `scale` times finer than
// the un-scaled result. Needed because the discriminant's *absolute* magnitude swings by ~15
// orders of magnitude between `validate`'s small fixed probe (~1e22, at real reserves of a
// few thousand tokens) and the reserve-clamped extreme (~1e37) — a fixed scale that's safe at
// one end wastes precision or overflows at the other. At the small end this resolves output
// differences well below a nano (see NOTES.md § Clamping before squaring; adaptive-precision
// sqrt for the concavity violation this fixed); at the clamped
// extreme, `shift` collapses to 0 (no headroom left) where nano-level precision is moot
// anyway (the reserve itself is ~5.8e8 tokens). `shift = n.leading_zeros()/2` (integer
// division) already keeps `2*shift <= n.leading_zeros()`, so `n << 2*shift` never loses a
// bit regardless of `n` — no extra cap needed for that.
#[inline]
fn scaled_isqrt(n: u128) -> (u128, u128) {
    if n == 0 {
        return (0, 1);
    }
    let shift = n.leading_zeros() / 2;
    let scale = 1u128 << shift;
    (isqrt(n << (2 * shift)), scale)
}

#[inline]
fn clamp_reserve(v: u128) -> u128 {
    v.min(RESERVE_CLAMP)
}

#[inline]
fn div_ceil(a: u128, b: u128) -> u128 {
    if b == 0 {
        return 0;
    }
    a.saturating_add(b - 1) / b
}

/// Fair-value of `delta` at price `i_fp`, in the reserve `v`'s own units, capped at
/// `INPUT_CAP_MULT * v` — a monotone-safe plateau instead of a `checked_mul` overflow for
/// the arbitrageur's largest brackets (up to ~1.8e19 nano) against a stale, tiny anchor.
#[inline]
fn fair_amount(v: u128, delta: u128, i_fp: u128) -> u128 {
    let cap = v.saturating_mul(INPUT_CAP_MULT);
    let raw = match i_fp.checked_mul(delta) {
        Some(p) => p / P_SCALE,
        None => u128::MAX,
    };
    raw.min(cap)
}

/// `_SolveQuadraticFunctionForTrade` at R=ONE (V0=V1=v): solves for the reserve output on
/// the shortage side, floored via a ceiled `V2` (docs/references/007-dodo-pmm/DODOMath.sol).
fn solve_quadratic_for_trade(v: u128, delta: u128, i_fp: u128, k_bps: u128) -> u128 {
    if v == 0 || delta == 0 || i_fp == 0 {
        return 0;
    }
    let fair = fair_amount(v, delta, i_fp);
    if fair == 0 {
        return 0;
    }

    let k1_out = fair.saturating_mul(v) / v.saturating_add(fair);
    if k_bps >= K_DEN {
        // k=1: exact zero-fee CPMM output (PMMPricing's own k==ONE special case) — the
        // containment anchor against 001-cpmm-fee's family (NOTES.md § Step 0.5,
        // "3. Containment demonstration").
        return k1_out;
    }
    // Dust reserves: the k<1 quadratic's b_sig flip is not monotone (0-hole then a dip
    // below even the k=1 fill). Fall back to the k=1 closed form, which is monotone for
    // any v. Unreachable at the committed k=1 point; reached at interior k with a
    // raw-ratio mid after a 2000-step drain (007b seed 9008).
    if v < DUST_RESERVE {
        return k1_out;
    }

    let kd_minus_k = K_DEN - k_bps;
    // part2 = k*V0^2/V1 + i*delta; at R=ONE, V0=V1=v so k*V0^2/V1 = k*v exactly (no
    // rounding loss from the general V0 != V1 case).
    let part2 = (k_bps.saturating_mul(v) / K_DEN).saturating_add(fair);
    let part1 = kd_minus_k.saturating_mul(v) / K_DEN;

    let (b_abs, b_sig) = if part1 >= part2 {
        (part1 - part2, false)
    } else {
        (part2 - part1, true)
    };

    // discriminant = b^2 + 4*(1-k)*k*v^2, scaled through K_DEN^2 so the intermediate stays
    // in u128 even at v = RESERVE_CLAMP (v^2 alone is already ~3.3e35). Named `v_sq` (not
    // `v2`) to avoid colliding with DODOMath.sol's own `V2` — the *output* reserve this
    // function solves for, computed below as `v_out`.
    //
    // Multiply-then-divide when it fits: `(v^2 * 4k(1-k)) / K_DEN^2` keeps the k-term at
    // dust reserves where `v^2 / K_DEN^2` floors to 0 and the divide-first form (parent)
    // degenerates. Fall back to divide-first only when the multiply would overflow u128
    // (v near RESERVE_CLAMP).
    let v_sq = v.saturating_mul(v);
    let four_k_1mk = 4 * k_bps * kd_minus_k;
    let kd2 = K_DEN * K_DEN;
    let disc_term = match v_sq.checked_mul(four_k_1mk) {
        Some(num) => num / kd2,
        None => (v_sq / kd2).saturating_mul(four_k_1mk),
    };
    let discriminant = b_abs.saturating_mul(b_abs).saturating_add(disc_term);
    // scale * true_sqrt(discriminant), floor-accurate to within 1 part in `scale` instead of
    // isqrt's usual +/-1 absolute unit — see scaled_isqrt's own comment for why a fixed scale
    // can't work across this family's full reserve range.
    let (sqrt_disc_scaled, scale) = scaled_isqrt(discriminant);

    let v_out = if b_sig {
        // Rationalised form (docs/DESIGN.md §2.9 cross-cutting finding #7;
        // crates/sim/src/curve_checks.rs::exposes_false_positive_from_cancellation_prone_concave_curve
        // is the committed proof the naive `sqrt(disc) - b_abs` form trips the checker on a
        // provably legal curve): V2 = 2*k*V0^2 / (sqrt(disc) + b_abs), ceiled. Both sides of
        // the ratio carry the same `scale` factor so it cancels exactly.
        let denom = sqrt_disc_scaled.saturating_add(b_abs.saturating_mul(scale));
        if denom == 0 {
            return 0;
        }
        let numerator = (v_sq / K_DEN)
            .saturating_mul(2 * k_bps)
            .saturating_mul(scale);
        div_ceil(numerator, denom)
    } else {
        // V2 = (b_abs + sqrt(disc)) / (2*(1-k)), ceiled. Same `scale`-both-sides cancellation.
        let denom = (2 * kd_minus_k).saturating_mul(scale);
        let numerator = b_abs
            .saturating_mul(scale)
            .saturating_add(sqrt_disc_scaled)
            .saturating_mul(K_DEN);
        div_ceil(numerator, denom)
    };

    // DODOMath's `V2 > V1 -> return 0` is not monotone-safe: a slightly smaller input
    // already returned ~v. Dust (`v < 1e6`) never reaches here; this fires when
    // `v >= DUST_RESERVE` and ceil-jitter still pushes V2 over v. Cap at the trade's
    // own fair so the plateau stays monotone in input (`min(fair, v-1)`).
    if v_out >= v {
        return fair.min(v.saturating_sub(1));
    }
    v - v_out
}

pub fn compute_swap(data: &[u8]) -> u64 {
    if data.len() < 25 {
        return 0;
    }
    let side = data[0];
    if side != 0 && side != 1 {
        return 0;
    }
    let input = rd8(data, 1);
    let rx = rd8(data, 9);
    let ry = rd8(data, 17);
    if rx == 0 || ry == 0 || input == 0 {
        return 0;
    }
    let storage: &[u8] = if data.len() >= 25 + 1024 {
        &data[25..25 + 1024]
    } else {
        &[]
    };

    let rx_c = clamp_reserve(rx);
    let ry_c = clamp_reserve(ry);
    let i_fp = anchor_price(storage, rx_c, ry_c);

    // At R=ONE, side=1 (sell base) solves on the quote reserve at price i (quote/base);
    // side=0 (buy base, pay quote) solves on the base reserve at the reciprocal price
    // (base/quote) — mirrors PMMPricing's sellBaseToken/sellQuoteToken split.
    let (v, i_used) = if side == 1 {
        (ry_c, i_fp)
    } else {
        (rx_c, reciprocal_fp(i_fp))
    };

    let gross = solve_quadratic_for_trade(v, input, i_used, K_BPS);
    if gross == 0 {
        return 0;
    }
    // Fee on the OUTPUT (source-faithful: DVMTrader.querySellBase/querySellQuote fee the
    // receive amount, not the pay amount — the load-bearing difference from 001's
    // fee-on-input CPMM, and the dominant term in the containment gap; NOTES.md).
    let fee = gross * FEE_BPS / 10_000;
    let net = gross.saturating_sub(fee);
    if net > u64::MAX as u128 {
        u64::MAX
    } else {
        net as u64
    }
}

#[inline]
fn reciprocal_fp(i_fp: u128) -> u128 {
    if i_fp == 0 {
        return 0;
    }
    // P_SCALE^2 / i_fp: reciprocal at the same P_SCALE fixed point. Floors to 0 for an
    // extreme (validator-probe-only) anchor price — monotone-safe (see fair_amount above).
    (P_SCALE * P_SCALE) / i_fp
}

/// The raw reserve ratio (quote per base) at `P_SCALE` fixed point — used as the
/// cold-start/garbage-state fallback AND as the warm after_swap write for every `K_BPS`
/// (NOTES.md § Pinned mechanism).
#[inline]
fn raw_ratio_price(rx: u128, ry: u128) -> u128 {
    ry.saturating_mul(P_SCALE) / rx
}

/// Reads the stored anchor price, falling back to the live reserve ratio for cold start and
/// garbage state (magic-sentinel check, docs/DESIGN.md §2.9 cross-cutting findings #4/#5) —
/// the initial reserves encode the initial price exactly, so this is correct at t=0 too.
fn anchor_price(storage: &[u8], rx_c: u128, ry_c: u128) -> u128 {
    if storage.len() >= STATE_END {
        let magic = rd_u64(storage, OFF_MAGIC);
        if magic == MAGIC {
            return rd16(storage, OFF_ANCHOR_PRICE);
        }
    }
    raw_ratio_price(rx_c, ry_c)
}

/// Called after EVERY executed trade (not sampled once per step, unlike `004`/`005`'s
/// estimators — PMM re-anchors to R=ONE on every trade by construction; see this file's own
/// header comment, "ANCHOR UPDATE"). One family: the post-trade mid is the raw post-trade
/// reserve ratio for every `K_BPS`, never a recursive `i_old * R_f`. The parent's k=1
/// special case (`K_BPS >= K_DEN` → the same `wr16` of `raw_ratio_price`) is preserved as
/// the unconditional write, so at the parent's committed point the storage update is
/// bit-identical.
pub fn after_swap(data: &[u8], storage: &mut [u8]) {
    if data.len() < 42 || storage.len() < STATE_END {
        return;
    }
    let side = data[1];
    let rx_post = rd8(data, 18);
    let ry_post = rd8(data, 26);
    if rx_post == 0 || ry_post == 0 || (side != 0 && side != 1) {
        return;
    }

    // Clamped the same way `anchor_price`/`compute_swap` clamp reserves before any ratio —
    // clamping here is what makes this write agree with `anchor_price`'s own cold-start
    // fallback bit-for-bit, including on `validate`'s synthetic near-`u64::MAX` reserve
    // probes. The parent applied the same clamp on both the k=1 early-return and the
    // (now deleted) recursive branch.
    let rx_post_c = clamp_reserve(rx_post);
    let ry_post_c = clamp_reserve(ry_post);
    let new_price = raw_ratio_price(rx_post_c, ry_post_c);

    let magic = rd_u64(storage, OFF_MAGIC);
    if magic != MAGIC {
        // First ever call: initialize at R=ONE with the current (post-trade) reserve
        // ratio — the faithful cold-start reading, since the initial reserves encode the
        // initial price exactly (cross-cutting §4/§5). Same write as the warm path.
        wr_u64(storage, OFF_MAGIC, MAGIC);
        wr16(storage, OFF_ANCHOR_PRICE, new_price);
        return;
    }

    wr16(storage, OFF_ANCHOR_PRICE, new_price);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after_swap_payload(
        side: u8,
        input: u64,
        output: u64,
        rx_post: u64,
        ry_post: u64,
    ) -> Vec<u8> {
        let mut data = vec![0u8; 42];
        data[0] = 2;
        data[1] = side;
        data[2..10].copy_from_slice(&input.to_le_bytes());
        data[10..18].copy_from_slice(&output.to_le_bytes());
        data[18..26].copy_from_slice(&rx_post.to_le_bytes());
        data[26..34].copy_from_slice(&ry_post.to_le_bytes());
        data
    }

    fn primed_storage(anchor: u128) -> [u8; 1024] {
        let mut storage = [0u8; 1024];
        wr_u64(&mut storage, OFF_MAGIC, MAGIC);
        wr16(&mut storage, OFF_ANCHOR_PRICE, anchor);
        storage
    }

    /// Independent of `raw_ratio_price`: quote-per-base at `P_SCALE` from the post-trade
    /// reserves themselves. This is the spec's post-trade mid for every `K_BPS`.
    /// `after_swap` does not read `K_BPS`; these tests therefore cover all k. Restoring
    /// a `K_BPS >= K_DEN` branch would make them pass at the committed 10_000 even if
    /// the k<1 path were recursive again — re-run at `K_BPS=2500` if that branch returns.
    fn spec_raw_ratio(rx: u128, ry: u128) -> u128 {
        ry * P_SCALE / rx
    }

    #[test]
    fn after_swap_sell_side_snaps_stale_anchor_to_post_trade_reserve_ratio() {
        // Warm state, sell-base (side=1). The stored mid is 100× the live ratio — the
        // recursive `i_old / R_f` form scales that error instead of replacing it.
        let rx_post = 100u64 * 1_000_000_000;
        let ry_post = 10_000u64 * 1_000_000_000;
        let expected = spec_raw_ratio(rx_post as u128, ry_post as u128);
        let stale = expected.saturating_mul(100);

        let mut storage = primed_storage(stale);
        let data = after_swap_payload(1, 1_000_000_000, 80_000_000_000, rx_post, ry_post);
        after_swap(&data, &mut storage);

        let got = rd16(&storage, OFF_ANCHOR_PRICE);
        assert_eq!(
            got, expected,
            "after_swap must write ry_post*P_SCALE/rx_post from current reserves only; \
             got {got} want {expected} (stale was {stale})"
        );
    }

    #[test]
    fn after_swap_buy_side_snaps_stale_anchor_to_post_trade_reserve_ratio() {
        let rx_post = 80u64 * 1_000_000_000;
        let ry_post = 12_000u64 * 1_000_000_000;
        let expected = spec_raw_ratio(rx_post as u128, ry_post as u128);
        let stale = expected / 50;

        let mut storage = primed_storage(stale);
        let data = after_swap_payload(0, 2_000_000_000, 15_000_000_000, rx_post, ry_post);
        after_swap(&data, &mut storage);

        let got = rd16(&storage, OFF_ANCHOR_PRICE);
        assert_eq!(
            got, expected,
            "buy-side after_swap must also write the raw post-trade ratio; \
             got {got} want {expected} (stale was {stale})"
        );
    }

    #[test]
    fn dust_reserve_falls_back_to_k1_and_is_monotone() {
        // Seed-9008 monotonicity hole (35220000→0 after 35210000→8235): rx=8291 nano
        // (< DUST_RESERVE), so k_bps is not read — this asserts the k-independent k=1
        // fallback, not the interior-k quadratic.
        let rx = 8_291u64;
        let ry = 35_386_792u64;
        let mut prev = 0u128;
        let mut input = 10_000_000u128;
        let i_fp = raw_ratio_price(rx as u128, ry as u128);
        let i_used = reciprocal_fp(i_fp);
        let fair = fair_amount(rx as u128, input, i_used);
        // Independent oracle of the k=1 closed form — do not substitute `k1_out` from
        // solve_quadratic_for_trade or the assertion becomes tautological.
        let k1 = fair.saturating_mul(rx as u128) / (rx as u128).saturating_add(fair);
        let first = solve_quadratic_for_trade(rx as u128, input, i_used, 25);
        assert_eq!(
            first, k1,
            "dust path must equal the k=1 closed form, not the quadratic"
        );
        while input <= 80_000_000 {
            let out = solve_quadratic_for_trade(rx as u128, input, i_used, 25);
            assert!(
                out >= prev,
                "non-monotone dust buy: prev_out={prev} input={input} out={out}"
            );
            prev = out;
            input += 10_000;
        }
    }
}
