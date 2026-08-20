use pinocchio::{account_info::AccountInfo, entrypoint, pubkey::Pubkey, ProgramResult};
use prop_amm_submission_sdk::{set_return_data_bytes, set_return_data_u64};

const NAME: &str = "001 CPMM Fee";
// Preserved unchanged from `programs/starter/src/lib.rs`: NAME and some comments differ
// (see NOTES.md § Provenance), but the compute_swap mechanism itself is untouched, so the
// field describing which model produced that mechanism hasn't changed either.
const MODEL_USED: &str = "GPT-5.3-Codex";
// === PARAMS BEGIN ===
const FEE_BPS: u128 = 66; // range: 1..=500
// === PARAMS END ===
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

/// Constant-product AMM with a single free parameter: the fee, in basis points out of
/// 10,000 (the same convention `crates/shared/src/normalizer.rs` uses). This is the 0-line
/// (docs/DESIGN.md §2.8). The `FEE_BPS = 500` this replaced was exactly the starter's own
/// 950/1000 fee ratio (`(10_000 - 500) / 10_000 = 950 / 1000`, and scaling a truncating
/// integer division's numerator and denominator by the same factor never changes its floor,
/// so the bps rewrite was bit-for-bit equivalent to the starter's own arithmetic for every
/// input — verified via `bench anchor` before any search ran). The committed value below is
/// `bench fit --strategy strategies/001-cpmm-fee`'s winning point; see `NOTES.md` § Fitted
/// point for the full curve and train/validation numbers.
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
            let net_y = input_amount * (10_000 - FEE_BPS) / 10_000;
            let new_ry = reserve_y + net_y;
            let k_div = (k + new_ry - 1) / new_ry;
            reserve_x.saturating_sub(k_div) as u64
        }
        1 => {
            let net_x = input_amount * (10_000 - FEE_BPS) / 10_000;
            let new_rx = reserve_x + net_x;
            let k_div = (k + new_rx - 1) / new_rx;
            reserve_y.saturating_sub(k_div) as u64
        }
        _ => 0,
    }
}
