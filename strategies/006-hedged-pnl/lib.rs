use pinocchio::{account_info::AccountInfo, entrypoint, pubkey::Pubkey, ProgramResult};
use prop_amm_submission_sdk::{set_return_data_bytes, set_return_data_u64, set_storage};

const NAME: &str = "006 Hedged PnL (Linear Price Impact)";
// This is a from-scratch adaptation of a prose-only mechanism (no source code to preserve
// unchanged) — see NOTES.md § Provenance — so MODEL_USED names the model that wrote this
// port, not a source's own constant.
const MODEL_USED: &str = "Claude Sonnet 5";

// ============================================================================
// 006 Hedged PnL — reduced 2D linear price-impact model (mid + fixed half-spread).
//
// Source: docs/references/006-hedged-pnl/README.md's "Linear Price Impact Model" — a
// two-sided, linear-marginal-price AMM. The full 4-coefficient (k++/k+-/k-+/k--) form is
// dynamically unstable (NOTES.md § Frozen parameter space): the only stationary manifold
// collapses it to ONE mid price plus a FIXED half-spread, both sides sharing a single
// impact slope `k`. That is what this port implements — an invented reduction, not the
// doc's own literal formula (which cannot pass validate.rs for any `k`, see NOTES.md).
//
// MECHANISM — per side, a quadratic trading function around a resting mid:
//   buy  (side 0, pay Y receive X): cost(x)  = p_buy*x  + 0.5*k*x^2   (invert for output)
//   sell (side 1, pay X receive Y): output(x) = p_sell*x - 0.5*k*x^2  (direct)
// where p_buy = mid*(1+delta_bps/10000), p_sell = mid*(1-delta_bps/10000).
//
// STATE — `mid` is carried in storage and moves by exactly `k * x_executed` per trade
// (up for a buy, down for a sell) — the same slope that shapes the curve itself, so the
// spread never drifts independently of it (NOTES.md's stability argument). The
// arbitrageur trades against whichever side is mispriced and drives `mid` back toward the
// true fair price for free (docs/DESIGN.md §2.9 cross-cutting finding #11); retail-driven
// updates are a noisier proxy, same caveat every state-keeping strategy in this freeze
// inherits (finding #11's own corollary).
//
// COLD START / GARBAGE STATE — a magic sentinel gates every read: mismatch (zeroed
// storage, or `validate.rs`'s random-byte probe) falls back to `reserve_y/reserve_x`
// rather than trusting any other byte in storage (finding #4, #5).
//
// SHAPE SAFETY — both sides read only reserves + storage, never `input_amount`, so each
// quote is a fixed-parameter curve within its own shape check (finding #3).
//
// SATURATING TAIL — the raw quadratic form is only used up to a switch point (a fraction
// of the vertex for sell, a fraction of the reserve for buy); beyond it a
// `A - B/(x+C)` tail (NOTES.md's invented adaptation, value+slope matched at the switch)
// carries the curve strictly monotone and concave all the way to `MAX_INPUT_AMOUNT`,
// asymptoting below the live reserve (findings #1, #6, #7).
// ============================================================================

// ---- fixed-point scale -------------------------------------------------------
const SCALE: u128 = 1_000_000_000; // both the price fixed-point scale and NANO_SCALE

// ---- searched parameters (docs/DESIGN.md §2.4/§2.5) --------------------------
// === PARAMS BEGIN ===
const K_SCALED: u128 = 1_639_729_121; // range: 250000000..=32000000000
const DELTA_BPS: u128 = 67; // range: 5..=200
                            // === PARAMS END ===

// ---- frozen, un-searched (NOTES.md § Frozen parameter space) -----------------
const MIN_MID_FP: u128 = 1_000; // real price floor ~1e-6 Y/X — sanitises degenerate reserves
const MAX_MID_FP: u128 = 1_000_000_000_000_000; // real price ceiling 1e6 Y/X — sane-band clamp (finding #5)
const SWITCH_FRACTION_NUM: u128 = 1; // switch point sits at 1/2 of the reserve cap
const SWITCH_FRACTION_DEN: u128 = 2;
const RESERVE_CAP_NUM: u128 = 999; // reserve cap = 99.9% of the live reserve
const RESERVE_CAP_DEN: u128 = 1_000;

// ---- storage byte layout (1024 bytes total, little-endian) ------------------
//   [0..8]   MAGIC   u64  — "initialized" sentinel
//   [8..24]  mid_fp  u128 — fixed-point mid price (scale SCALE), Y per X
//   [24..1024] unused (zeroed)
const OFF_MAGIC: usize = 0;
const OFF_MID: usize = 8;
const STATE_END: usize = 24;
const MAGIC: u64 = 0x4845_4447_4550_4E4C; // "HEDGEPNL"-ish sentinel

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
            // set_storage so mid drifts identically on native and BPF.
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

// ---- integer sqrt (Newton, u128) -------------------------------------------
#[inline]
fn isqrt(n: u128) -> u128 {
    if n < 2 {
        return n;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

#[inline]
fn clamp_mid(mid: u128) -> u128 {
    if mid < MIN_MID_FP {
        MIN_MID_FP
    } else if mid > MAX_MID_FP {
        MAX_MID_FP
    } else {
        mid
    }
}

/// Reserve-ratio fallback: the only anchor available before any trade has ever fired
/// `after_swap` (cold start), and the sanitiser for garbage storage (finding #4, #5).
#[inline]
fn reserve_mid(rx: u128, ry: u128) -> u128 {
    if rx == 0 {
        return MAX_MID_FP; // no X liquidity: price of X is maximal, not undefined
    }
    if ry == 0 {
        return MIN_MID_FP; // no Y liquidity: price of X is minimal
    }
    clamp_mid(ry.saturating_mul(SCALE) / rx)
}

/// Loads `mid_fp` from storage, gated by the magic sentinel — mismatch (zeroed or
/// random-byte storage) always falls back to `reserve_mid`, never reads var/count-style
/// fields from unrecognised bytes (finding #3: storage + compile-time constants only,
/// finding #5: garbage state sanitised).
#[inline]
fn mid_from_state(rx: u128, ry: u128, storage: &[u8]) -> u128 {
    if storage.len() < STATE_END || rd_u64(storage, OFF_MAGIC) != MAGIC {
        return reserve_mid(rx, ry);
    }
    clamp_mid(rd16(storage, OFF_MID))
}

/// Forward cost to buy `x` (nano X) at buy-side price `buy_fp`, impact `k`: exact for any
/// `x` (the buy-side cost function has no vertex — see NOTES.md), computed divide-early to
/// stay inside u128 for the realistic reserve magnitudes this is ever called with.
#[inline]
fn cost_buy(x: u128, buy_fp: u128, k: u128) -> u128 {
    let term1 = buy_fp.saturating_mul(x) / SCALE;
    let kx_over_scale = k.saturating_mul(x) / SCALE;
    let term2 = kx_over_scale.saturating_mul(x) / (2 * SCALE);
    term1.saturating_add(term2)
}

/// Stable inversion of `cost_buy`: `x = 2*SCALE*y / (sqrt(buy_fp^2 + 2*k*y) + buy_fp)`.
/// The naive `(sqrt(...) - buy_fp)/k` form is cancellation-prone near small `y`
/// (docs/DESIGN.md §2.9 cross-cutting finding #7); this is the stable rearrangement.
#[inline]
fn invert_buy(y: u128, buy_fp: u128, k: u128) -> u128 {
    let inner = buy_fp
        .saturating_mul(buy_fp)
        .saturating_add(2u128.saturating_mul(k).saturating_mul(y));
    let root = isqrt(inner);
    let denom = root.saturating_add(buy_fp);
    if denom == 0 {
        return 0;
    }
    (2u128.saturating_mul(SCALE).saturating_mul(y)) / denom
}

/// A `RESERVE_CAP - D/((v - switch) + C)` tail: value- and slope-matched to a raw
/// increasing-concave curve at `(switch, value_at_switch)`, strictly monotone and concave
/// for every `v > switch`, asymptoting to `reserve_cap` without ever reaching it
/// (NOTES.md § Saturating tail). `slope_num/slope_den` is the raw curve's own slope at the
/// switch point, kept as a fraction rather than rounded to preserve the match.
#[inline]
fn saturating_tail(
    v: u128,
    switch: u128,
    value_at_switch: u128,
    reserve_cap: u128,
    slope_num: u128,
    slope_den: u128,
) -> u128 {
    if reserve_cap <= value_at_switch || slope_num == 0 {
        return value_at_switch;
    }
    let headroom = reserve_cap - value_at_switch;
    // C = headroom / slope = headroom * slope_den / slope_num
    let c = headroom.saturating_mul(slope_den) / slope_num;
    if c == 0 {
        return reserve_cap;
    }
    let d = c.saturating_mul(headroom);
    let w = v.saturating_sub(switch).saturating_add(c);
    if w == 0 {
        return value_at_switch;
    }
    reserve_cap.saturating_sub(d / w)
}

/// Buy side (side 0): input is Y (nano), output is X (nano).
fn buy_output(y_in: u128, rx: u128, mid_fp: u128) -> u64 {
    if rx == 0 {
        return 0;
    }
    let reserve_cap_x = rx.saturating_mul(RESERVE_CAP_NUM) / RESERVE_CAP_DEN;
    if reserve_cap_x == 0 {
        return 0;
    }
    let v0 = reserve_cap_x.saturating_mul(SWITCH_FRACTION_NUM) / SWITCH_FRACTION_DEN;
    let buy_fp = mid_fp.saturating_mul(10_000 + DELTA_BPS) / 10_000;

    let y0 = cost_buy(v0, buy_fp, K_SCALED);
    let raw = if y_in <= y0 {
        invert_buy(y_in, buy_fp, K_SCALED)
    } else {
        // dx/dy at the switch = SCALE^2 / (k*v0 + SCALE*buy_fp) — kept as a fraction
        // (num=SCALE^2, den=k*v0+SCALE*buy_fp) so saturating_tail need not round it.
        let slope_den = K_SCALED
            .saturating_mul(v0)
            .saturating_add(SCALE.saturating_mul(buy_fp));
        let slope_num = SCALE.saturating_mul(SCALE);
        saturating_tail(y_in, y0, v0, reserve_cap_x, slope_num, slope_den)
    };
    raw.min(reserve_cap_x) as u64
}

/// Sell side (side 1): input is X (nano), output is Y (nano).
fn sell_output(x_in: u128, ry: u128, mid_fp: u128) -> u64 {
    if ry == 0 {
        return 0;
    }
    let reserve_cap_y = ry.saturating_mul(RESERVE_CAP_NUM) / RESERVE_CAP_DEN;
    if reserve_cap_y == 0 {
        return 0;
    }
    let sell_fp = mid_fp.saturating_mul(10_000 - DELTA_BPS) / 10_000;
    if sell_fp == 0 {
        return 0;
    }

    // x0 = min(half the vertex, a reserve-safe bound) — the first term keeps the raw
    // quadratic on its increasing branch (finding: "the doc's raw formula cannot pass for
    // any k" — the vertex is at sell_fp*SCALE/k); the second guarantees
    // cost_sell(x0) <= reserve_cap_y/2 via cost_sell(x) <= sell_fp*x/SCALE regardless of k
    // (NOTES.md § Saturating tail).
    let vertex_half = sell_fp.saturating_mul(SCALE) / (2 * K_SCALED);
    let reserve_safe = reserve_cap_y.saturating_mul(SCALE) / (2 * sell_fp);
    let x0 = vertex_half.min(reserve_safe);

    let kx0_over_scale = K_SCALED.saturating_mul(x0) / SCALE;
    let term2_0 = kx0_over_scale.saturating_mul(x0) / (2 * SCALE);
    let v0 = (sell_fp.saturating_mul(x0) / SCALE).saturating_sub(term2_0);

    let raw = if x_in <= x0 {
        let kx_over_scale = K_SCALED.saturating_mul(x_in) / SCALE;
        let term2 = kx_over_scale.saturating_mul(x_in) / (2 * SCALE);
        (sell_fp.saturating_mul(x_in) / SCALE).saturating_sub(term2)
    } else {
        // slope at x0 = sell_fp/SCALE - k*x0/SCALE^2 = (sell_fp*SCALE - k*x0) / SCALE^2.
        let slope_num = sell_fp
            .saturating_mul(SCALE)
            .saturating_sub(K_SCALED.saturating_mul(x0));
        let slope_den = SCALE.saturating_mul(SCALE);
        saturating_tail(x_in, x0, v0, reserve_cap_y, slope_num, slope_den)
    };
    raw.min(reserve_cap_y) as u64
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
    let storage: &[u8] = if data.len() >= 25 + 1024 {
        &data[25..25 + 1024]
    } else {
        &[]
    };
    let mid_fp = mid_from_state(rx, ry, storage);
    match side {
        0 => buy_output(input, rx, mid_fp),
        1 => sell_output(input, ry, mid_fp),
        _ => 0,
    }
}

/// Called after EVERY executed trade. `mid` moves by exactly `k * x_executed` — up for a
/// buy (X grows scarcer), down for a sell — the same slope that shapes the quoted curve,
/// which is what keeps the collapsed 2D state stationary (NOTES.md § Frozen parameter
/// space's stability argument) rather than drifting independently of the curve itself.
pub fn after_swap(data: &[u8], storage: &mut [u8]) {
    if data.len() < 42 || storage.len() < STATE_END {
        return;
    }
    let side = data[1];
    let input = rd_u64(data, 2) as u128;
    let output = rd_u64(data, 10) as u128;
    let rx_post = rd8(data, 18);
    let ry_post = rd8(data, 26);

    let base_mid = if rd_u64(storage, OFF_MAGIC) == MAGIC {
        clamp_mid(rd16(storage, OFF_MID))
    } else {
        reserve_mid(rx_post, ry_post)
    };

    let x_executed = if side == 0 { output } else { input };
    let delta = K_SCALED.saturating_mul(x_executed) / SCALE;
    let new_mid = clamp_mid(if side == 0 {
        base_mid.saturating_add(delta)
    } else {
        base_mid.saturating_sub(delta)
    });

    wr_u64(storage, OFF_MAGIC, MAGIC);
    wr16(storage, OFF_MID, new_mid);
}
