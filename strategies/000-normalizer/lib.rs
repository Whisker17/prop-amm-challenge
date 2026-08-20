use pinocchio::{account_info::AccountInfo, entrypoint, pubkey::Pubkey, ProgramResult};
use prop_amm_submission_sdk::{set_return_data_bytes, set_return_data_u64};

const NAME: &str = "000 Normalizer (as submission)";
// This file is never submitted to the challenge UI (docs/GIT_WORKFLOW.md's "deploy" only
// ever means a strategies/NNN-<slug> candidate) — it exists solely as the bench-internal
// baseline docs/DESIGN.md §2.8 calls "normalizer-as-submission". MODEL_USED is therefore
// inert here; "None" only reflects that this mechanism is copied, not authored.
const MODEL_USED: &str = "None";
// Same fee as crates/shared/src/normalizer.rs / programs/normalizer/src/lib.rs (997/1000 =
// 30 bps) — "the same curve as the opponent" (docs/DESIGN.md §2.8), not a fitted point.
const FEE_NUMERATOR: u128 = 997;
const FEE_DENOMINATOR: u128 = 1000;
const STORAGE_SIZE: usize = 1024;

#[derive(wincode::SchemaRead)]
struct ComputeSwapInstruction {
    side: u8,
    input_amount: u64,
    reserve_x: u64,
    reserve_y: u64,
    _storage: [u8; STORAGE_SIZE],
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
        // tag 0 or 1 = compute_swap (side)
        0 | 1 => {
            let output = compute_swap(instruction_data);
            set_return_data_u64(output);
        }
        // tag 2 = after_swap (no-op — constant product needs no per-trade state)
        2 => {}
        // tag 3 = get_name (for leaderboard display)
        3 => set_return_data_bytes(NAME.as_bytes()),
        // tag 4 = get_model_used (for metadata display)
        4 => set_return_data_bytes(get_model_used().as_bytes()),
        _ => {}
    }

    Ok(())
}

pub fn get_model_used() -> &'static str {
    MODEL_USED
}

/// Constant-product AMM at the normalizer's own fee (docs/DESIGN.md §2.8's third baseline):
/// "the same curve as the opponent", run through the submission interface so bench's compile
/// path (`prop-amm build`/`validate`) can load it like any other candidate.
pub fn compute_swap(data: &[u8]) -> u64 {
    let decoded: ComputeSwapInstruction = match wincode::deserialize(data) {
        Ok(decoded) => decoded,
        Err(_) => return 0,
    };

    let side = decoded.side;
    let input_amount = decoded.input_amount as u128;
    let reserve_x = decoded.reserve_x as u128;
    let reserve_y = decoded.reserve_y as u128;

    if reserve_x == 0 || reserve_y == 0 {
        return 0;
    }

    let k = reserve_x * reserve_y;

    match side {
        0 => {
            let net_y = input_amount * FEE_NUMERATOR / FEE_DENOMINATOR;
            let new_ry = reserve_y + net_y;
            let k_div = (k + new_ry - 1) / new_ry;
            reserve_x.saturating_sub(k_div) as u64
        }
        1 => {
            let net_x = input_amount * FEE_NUMERATOR / FEE_DENOMINATOR;
            let new_rx = reserve_x + net_x;
            let k_div = (k + new_rx - 1) / new_rx;
            reserve_y.saturating_sub(k_div) as u64
        }
        _ => 0,
    }
}
