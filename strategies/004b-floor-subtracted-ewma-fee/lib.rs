use pinocchio::{account_info::AccountInfo, entrypoint, pubkey::Pubkey, ProgramResult};
use prop_amm_submission_sdk::{set_return_data_bytes, set_return_data_u64, set_storage};

const NAME: &str = "004b Floor-Subtracted EWMA Fee";
// The vol-fee mapping's shape changes (linear -> floor-subtracted, §2.9 variant) relative
// to the parent `004-ewma-shock-decay-fee`, so this names the model that wrote this
// variant rather than the parent's preserved source constant (NOTES.md § Fidelity
// self-assessment).
const MODEL_USED: &str = "Claude Sonnet 5";
const STORAGE_SIZE: usize = 1024;

// ============================================================================
// 004b Floor-Subtracted EWMA Fee — a §2.9 variant of `004-ewma-shock-decay-fee`.
//
// fee = BASE + vol_fee + shock_fee, capped at MAX_FEE.
//   vol_fee   = max(ewma_vol - FLOOR, 0) * VOL_MULT_NUM / VOL_MULT_DEN  (the mechanism
//               change: a floor-subtracted, finer-gain residual replaces the parent's plain
//               `ewma_vol * VOL_MULT`, decoupling the calm-regime fee level from the
//               high-sigma slope — NOTES.md § Objective)
//   shock_fee = 0, frozen (the parent's own fit priced this layer negative; `after_swap`
//               still ticks the counter so the state trajectory stays byte-identical to the
//               parent's — NOTES.md § Containment)
//
// SHAPE SAFETY — fee_from_storage reads ONLY storage, never the input amount, so each
// side's quote is a pure fee-discounted constant-product curve (monotone + concave).
//
// PARITY — the BPF tag-2 path decodes storage from the instruction, runs the SAME
// after_swap on a local buffer, then persists via set_storage, so native and BPF fee
// trajectories are identical.
// ============================================================================

// ---- vol -> fee mapping, searched in bps (docs/DESIGN.md §2.4/§2.5) -------------------
// === PARAMS BEGIN ===
const BASE_BPS: u64 = 34; // range: 4..=80
const VOL_MULT_NUM: u64 = 8; // range: 0..=64
const FLOOR_BPS: u64 = 0; // range: 0..=60
const MAX_FEE_BPS: u64 = 391; // range: 66..=500
                              // === PARAMS END ===

// Frozen: quantization choice (a power of two, so NUM/DEN lowers to a shift), not an
// independent parameter (NOTES.md § Frozen parameter space).
const VOL_MULT_DEN: u64 = 8;
// Frozen dead at 0: the parent's own fit priced the shock layer negative, and both
// candidate repairs are out of scope for a §2.9 variant (NOTES.md § Objective). Kept as a
// named term (rather than deleted) so `after_swap`'s counter keeps ticking harmlessly and
// the state trajectory stays byte-identical to the parent's.
const SHOCK_FEE_PER_STEP_BPS: u64 = 0;

// Derived, not independent parameters: the fee arithmetic runs on a 1e9 scale (1 bps =
// 1e9/10_000 = 100_000), so each searched bps value is rescaled once here rather than
// duplicating the fee formula in two unit systems — same pattern the parent's own
// `BASE_FEE_1E9`/`MAX_FEE_1E9` used.
const BASE_FEE_1E9: u64 = BASE_BPS * 100_000;
const FLOOR_1E9: u64 = FLOOR_BPS * 100_000;
const MAX_FEE_1E9: u64 = MAX_FEE_BPS * 100_000;
const SHOCK_FEE_PER_STEP_1E9: u64 = SHOCK_FEE_PER_STEP_BPS * 100_000;

// Frozen at source anchors, inherited unchanged from the parent's own review amendment:
// interacts multiplicatively with the searched gains above, so searching it buys little
// beyond what those already express.
const ALPHA_1E9: u128 = 200_000_000; // vol EWMA alpha = 0.20
const ONE_M_ALPHA_1E9: u128 = 1_000_000_000 - ALPHA_1E9; // derived, not independent
const SHOCK_THRESHOLD_1E9: u64 = 5_000_000; // 0.5% price move re-arms the shock
const SHOCK_DECAY_STEPS: u64 = 8;

// ---- storage byte layout (1024 bytes total, little-endian) ------------------
//   [0..8]   ewma_vol     u64 — EWMA of |delta price / price|, scaled 1e9
//   [8..16]  last_rx      u64 — reserve_x saved after previous swap
//   [16..24] last_ry      u64 — reserve_y saved after previous swap
//   [24..32] shock_steps  u64 — countdown after a large price move (max SHOCK_DECAY_STEPS)
//   [32..1024] unused (zeroed)
const OFF_EWMA_VOL: usize = 0;
const OFF_LAST_RX: usize = 8;
const OFF_LAST_RY: usize = 16;
const OFF_SHOCK_STEPS: usize = 24;
const STATE_END: usize = 32;

#[derive(wincode::SchemaRead)]
struct ComputeSwapInstruction {
    side: u8,
    input_amount: u64,
    reserve_x: u64,
    reserve_y: u64,
    storage: [u8; STORAGE_SIZE],
}

#[cfg(not(feature = "no-entrypoint"))]
entrypoint!(process_instruction);

pub fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    if instruction_data.is_empty() {
        return Ok(());
    }

    match instruction_data[0] {
        0 | 1 => set_return_data_u64(compute_swap(instruction_data)),
        2 => handle_after_swap(instruction_data),
        3 => set_return_data_bytes(NAME.as_bytes()),
        4 => set_return_data_bytes(get_model_used().as_bytes()),
        _ => {}
    }

    Ok(())
}

// BPF path: copy storage snapshot from instruction data, run after_swap, persist via
// set_storage — the native and BPF fee trajectories then agree by construction.
fn handle_after_swap(d: &[u8]) {
    if d.len() < 42 + STORAGE_SIZE {
        return;
    }
    let mut storage = [0u8; STORAGE_SIZE];
    storage.copy_from_slice(&d[42..42 + STORAGE_SIZE]);
    after_swap(d, &mut storage);
    let _ = set_storage(&storage);
}

pub fn get_model_used() -> &'static str {
    MODEL_USED
}

// ---- little-endian helpers --------------------------------------------------
#[inline]
fn read_u64(b: &[u8], off: usize) -> u64 {
    let mut arr = [0u8; 8];
    arr.copy_from_slice(&b[off..off + 8]);
    u64::from_le_bytes(arr)
}

#[inline]
fn write_u64(b: &mut [u8], off: usize, v: u64) {
    b[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

#[inline]
fn ceil_div(a: u128, b: u128) -> u128 {
    (a + b - 1) / b
}

// ---- compute_swap ------------------------------------------------------------

pub fn compute_swap(data: &[u8]) -> u64 {
    let d: ComputeSwapInstruction = match wincode::deserialize(data) {
        Ok(x) => x,
        Err(_) => return 0,
    };

    let input = d.input_amount as u128;
    let rx = d.reserve_x as u128;
    let ry = d.reserve_y as u128;
    if rx == 0 || ry == 0 || input == 0 {
        return 0;
    }

    let fee_1e9 = fee_from_storage(&d.storage); // MUST NOT depend on input
    let keep = (1_000_000_000u128).saturating_sub(fee_1e9);
    let k = rx * ry;
    let net = input * keep / 1_000_000_000;

    match d.side {
        0 => {
            let new_ry = ry + net;
            let new_rx = ceil_div(k, new_ry);
            rx.saturating_sub(new_rx) as u64
        }
        1 => {
            let new_rx = rx + net;
            let new_ry = ceil_div(k, new_rx);
            ry.saturating_sub(new_ry) as u64
        }
        _ => 0,
    }
}

/// Fee in the source's native 1e9 scale. Depends ONLY on storage (never on `input_amount`),
/// so the resulting CPMM curve is monotone + concave per side. Zero-initialized storage
/// (every simulation's cold-start state) reads `ewma_vol = 0, shock_steps = 0`; at the
/// containment point (`FLOOR_1E9 = 0`) the residual is then also `0`, giving
/// `fee = BASE_FEE_1E9` with no separate sentinel/fallback branch needed — see the parent's
/// NOTES.md § Cold start and garbage-state handling (inherited unchanged) for why this also
/// sanitises random-byte storage (`crates/cli/src/commands/validate.rs`'s randomized probe)
/// without a magic check: `saturating_sub`/`saturating_mul`/`saturating_add` and a final
/// `.min(MAX_FEE_1E9)` clamp bound the output for any `u64` state, garbage or not.
fn fee_from_storage(storage: &[u8]) -> u128 {
    if storage.len() < STATE_END {
        return BASE_FEE_1E9 as u128;
    }
    let ewma_vol = read_u64(storage, OFF_EWMA_VOL);
    let shock_steps = read_u64(storage, OFF_SHOCK_STEPS).min(SHOCK_DECAY_STEPS);

    // The mechanism change: a floor-subtracted residual (a ReLU on state, never on
    // `input_amount`, so no kink enters the quoted curve) replaces the parent's plain
    // `ewma_vol * VOL_MULT`. Below the floor the residual is 0 and the fee sits at `BASE`
    // regardless of `ewma_vol`; above it, a steeper `VOL_MULT_NUM/VOL_MULT_DEN` slope can be
    // afforded without also raising the calm-regime fee (NOTES.md § Objective).
    let residual = (ewma_vol as u128).saturating_sub(FLOOR_1E9 as u128);
    let vol_fee = residual * VOL_MULT_NUM as u128 / VOL_MULT_DEN as u128;
    let shock_fee = shock_steps.saturating_mul(SHOCK_FEE_PER_STEP_1E9) as u128;

    (BASE_FEE_1E9 as u128 + vol_fee + shock_fee).min(MAX_FEE_1E9 as u128)
}

/// Called after EVERY executed trade. Updates the vol EWMA from the relative price move
/// since the last trade and re-arms/decays the shock countdown.
///
/// Byte-identical to the parent's (`004-ewma-shock-decay-fee`) — the shock counter keeps
/// ticking harmlessly even though `SHOCK_FEE_PER_STEP_BPS` is frozen at 0, which is what
/// makes the state trajectory, and therefore containment against the parent, bit-exact
/// (NOTES.md § Containment).
pub fn after_swap(data: &[u8], storage: &mut [u8]) {
    if data.len() < 42 || storage.len() < STATE_END {
        return;
    }

    let cur_rx = read_u64(data, 18) as u128;
    let cur_ry = read_u64(data, 26) as u128;

    let old_vol = read_u64(storage, OFF_EWMA_VOL);
    let last_rx = read_u64(storage, OFF_LAST_RX) as u128;
    let last_ry = read_u64(storage, OFF_LAST_RY) as u128;
    let shock_steps = read_u64(storage, OFF_SHOCK_STEPS).min(SHOCK_DECAY_STEPS);

    // Relative price change |delta(ry/rx)| / (ry/rx) via cross-multiplication, avoiding a
    // division on the price itself. `last_rx == 0` covers the very first call (storage is
    // zero-initialized at the start of every simulation): no move sample yet, EWMA stays 0.
    let price_change_1e9: u64 = if last_rx > 0 && last_ry > 0 && cur_rx > 0 {
        let cross_new = cur_ry.saturating_mul(last_rx);
        let cross_old = last_ry.saturating_mul(cur_rx);
        let diff = if cross_new > cross_old {
            cross_new - cross_old
        } else {
            cross_old - cross_new
        };
        let denom = last_ry.saturating_mul(cur_rx);
        if denom > 0 {
            diff.saturating_mul(1_000_000_000)
                .checked_div(denom)
                .unwrap_or(u128::MAX)
                .min(u64::MAX as u128) as u64
        } else {
            0
        }
    } else {
        0
    };

    // EWMA vol: alpha = 0.20, converges to steady-state vol within ~15 steps.
    let new_vol = ((ALPHA_1E9 * price_change_1e9 as u128 + ONE_M_ALPHA_1E9 * old_vol as u128)
        / 1_000_000_000) as u64;

    // Shock: reset countdown on a large move, else decay by 1.
    let new_shock = if price_change_1e9 >= SHOCK_THRESHOLD_1E9 {
        SHOCK_DECAY_STEPS
    } else {
        shock_steps.saturating_sub(1)
    };

    write_u64(storage, OFF_EWMA_VOL, new_vol);
    write_u64(storage, OFF_LAST_RX, cur_rx as u64);
    write_u64(storage, OFF_LAST_RY, cur_ry as u64);
    write_u64(storage, OFF_SHOCK_STEPS, new_shock);
}
