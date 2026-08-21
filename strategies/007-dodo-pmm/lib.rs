use pinocchio::{account_info::AccountInfo, entrypoint, pubkey::Pubkey, ProgramResult};
use prop_amm_submission_sdk::{set_return_data_bytes, set_return_data_u64, set_storage};

const NAME: &str = "007 DODO PMM";
const MODEL_USED: &str = "Claude Sonnet 5";

// ============================================================================
// DODO Proactive Market Maker, collapsed to R = ONE with the arbitrageur as the oracle.
//
// Source: DODOEX/contractV2 @ 2f1bcdac7ef1beee7599a756e2eed26732c2536d
// (docs/references/007-dodo-pmm/{PMMPricing.sol, DODOMath.sol}). Port target per the
// issue: DPP's two-sided PMM, not DVM (one-sided by construction) and not the full
// R-state machine (three independent failure modes at real states) — see NOTES.md
// § Fidelity self-assessment for the full rejection reasoning.
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
// family (NOTES.md § Nested 0-line).
//
// ANCHOR UPDATE (after_swap, no sqrt). Re-anchoring is not a no-op for a curved book: the
// PMM's own marginal price at the post-trade reserve point differs from the pre-trade
// anchor's naive ry/rx ratio by the same R_f multiplier the quote used, so
// `after_swap` recomputes the curve's own post-trade mid (`i_old * R_f` or `i_old / R_f`,
// R_f = [(K_DEN-k)*Vp^2 + k*V0^2] / (K_DEN*Vp^2)) rather than the raw reserve ratio.
// `V0`/`Vp` (the shortage side's pre/post-trade reserve) are reconstructed from the
// after_swap payload alone — no reserves beyond the anchor price need to be stored, since
// re-anchoring every trade means the last anchor's target always equals the reserve just
// before the next trade.
//
// SHAPE SAFETY. `solve_quadratic_for_trade` reads only storage-derived state (the anchor
// price) plus compile-time consts and the live reserves passed alongside `input_amount` —
// never `input_amount` itself for anything but `delta`, so the curve quoted for a given
// state is a pure function of trade size: monotone and concave in input for k in (0,1].
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

// ---- searched parameters -----------------------------------------------------
// K_BPS: curvature, in units of 1e-4 of ONE (k = K_BPS/K_DEN). FEE_BPS: fee on the output,
// bps of 10_000 — same convention and frozen range as 001-cpmm-fee's own FEE_BPS.
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
// differences well below a nano (see NOTES.md § CU and arithmetic risk); at the clamped
// extreme, `shift` collapses to 0 (no headroom left) where nano-level precision is moot
// anyway (the reserve itself is ~5.8e8 tokens). Capped at 60 bits so `2*shift` never
// approaches u128's 128-bit width regardless of `n`.
#[inline]
fn scaled_isqrt(n: u128) -> (u128, u128) {
    if n == 0 {
        return (0, 1);
    }
    let shift = (n.leading_zeros() / 2).min(60);
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
    (a + b - 1) / b
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

    if k_bps >= K_DEN {
        // k=1: exact zero-fee CPMM output (PMMPricing's own k==ONE special case) — the
        // containment anchor against 001-cpmm-fee's family (NOTES.md § Nested 0-line).
        return fair.saturating_mul(v) / v.saturating_add(fair);
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
    // in u128 even at v = RESERVE_CLAMP (v^2 alone is already ~3.3e35).
    let v2 = v.saturating_mul(v);
    let v2_over_kd2 = v2 / (K_DEN * K_DEN);
    let disc_term = v2_over_kd2.saturating_mul(4 * k_bps * kd_minus_k);
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
        let numerator = (v2 / K_DEN).saturating_mul(2 * k_bps).saturating_mul(scale);
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

    // DODOMath's `V2 > V1 -> return 0` guard, folded into the saturating subtraction: only
    // reachable via ceil-jitter at the smallest inputs, where a zero output is monotone-safe.
    v.saturating_sub(v_out)
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
    ry_c.saturating_mul(P_SCALE) / rx_c
}

/// R_f's denominator, K_DEN-scaled so the caller's multiply-by-`i_old` stays in u128:
/// `R_f = [(K_DEN-k)*Vp^2 + k*V0^2] / (K_DEN*Vp^2)`, returned as `(denom_scaled, Vp^2)`
/// where `denom_scaled ~= [(K_DEN-k)*Vp^2 + k*V0^2] / K_DEN`.
#[inline]
fn rf_denominator_scaled(v0: u128, vp: u128, k_bps: u128) -> (u128, u128) {
    let vp2 = vp.saturating_mul(vp);
    let v02 = v0.saturating_mul(v0);
    let vp2_kd = vp2 / K_DEN;
    let v02_kd = v02 / K_DEN;
    let denom_scaled = (K_DEN - k_bps)
        .saturating_mul(vp2_kd)
        .saturating_add(k_bps.saturating_mul(v02_kd));
    (denom_scaled, vp2)
}

/// Post-trade mid when the QUOTE side is short (side=1, sold base): `i_old / R_f`.
#[inline]
fn post_trade_mid_divide(i_old: u128, v0: u128, vp: u128, k_bps: u128) -> u128 {
    if vp == 0 {
        return i_old;
    }
    let (denom_scaled, vp2) = rf_denominator_scaled(v0, vp, k_bps);
    if denom_scaled == 0 {
        return i_old;
    }
    i_old.saturating_mul(vp2) / denom_scaled
}

/// Post-trade mid when the BASE side is short (side=0, bought base): `i_old * R_f`.
#[inline]
fn post_trade_mid_multiply(i_old: u128, v0: u128, vp: u128, k_bps: u128) -> u128 {
    if vp == 0 {
        return i_old;
    }
    let (denom_scaled, vp2) = rf_denominator_scaled(v0, vp, k_bps);
    if vp2 == 0 {
        return i_old;
    }
    i_old.saturating_mul(denom_scaled) / vp2
}

/// Called after EVERY executed trade (not sampled once per step, unlike `004`/`005`'s
/// estimators — PMM re-anchors to R=ONE on every trade by construction; NOTES.md § Anchor
/// update rule). No sqrt: both directions are rational functions of the reconstructed
/// pre-trade reserve and the post-trade reserve on the shortage side.
pub fn after_swap(data: &[u8], storage: &mut [u8]) {
    if data.len() < 42 || storage.len() < STATE_END {
        return;
    }
    let side = data[1];
    let input = rd8(data, 2);
    let output = rd8(data, 10);
    let rx_post = rd8(data, 18);
    let ry_post = rd8(data, 26);
    if rx_post == 0 || ry_post == 0 || (side != 0 && side != 1) {
        return;
    }

    let magic = rd_u64(storage, OFF_MAGIC);
    if magic != MAGIC {
        // First ever call: no valid pre-trade anchor exists to derive a curved re-anchor
        // from. Initialize at R=ONE with the current (post-trade) reserve ratio — the
        // faithful cold-start reading, since the initial reserves encode the initial price
        // exactly (cross-cutting §4/§5).
        let price = ry_post.saturating_mul(P_SCALE) / rx_post;
        wr_u64(storage, OFF_MAGIC, MAGIC);
        wr16(storage, OFF_ANCHOR_PRICE, price);
        return;
    }

    let i_old = rd16(storage, OFF_ANCHOR_PRICE);

    // Reconstruct the PRE-trade reserves — == the last re-anchor's B0/Q0, since every prior
    // trade re-anchored to R=ONE — from the payload alone; no extra state needed.
    let (rx_pre, ry_pre) = if side == 1 {
        (
            rx_post.saturating_sub(input),
            ry_post.saturating_add(output),
        )
    } else {
        (
            rx_post.saturating_add(output),
            ry_post.saturating_sub(input),
        )
    };
    if rx_pre == 0 || ry_pre == 0 {
        return;
    }

    let rx_pre_c = clamp_reserve(rx_pre);
    let ry_pre_c = clamp_reserve(ry_pre);
    let rx_post_c = clamp_reserve(rx_post);
    let ry_post_c = clamp_reserve(ry_post);

    let new_price = if side == 1 {
        post_trade_mid_divide(i_old, ry_pre_c, ry_post_c, K_BPS)
    } else {
        post_trade_mid_multiply(i_old, rx_pre_c, rx_post_c, K_BPS)
    };

    wr16(storage, OFF_ANCHOR_PRICE, new_price);
}
