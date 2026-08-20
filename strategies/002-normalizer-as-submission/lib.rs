use pinocchio::{account_info::AccountInfo, entrypoint, pubkey::Pubkey, ProgramResult};
use prop_amm_submission_sdk::{set_return_data_bytes, set_return_data_u64};

const NAME: &str = "002 Normalizer As Submission";
const MODEL_USED: &str = "Claude Sonnet 5";
// No PARAMS block: this strategy has no free parameter (docs/DESIGN.md §2.8) — it is a
// faithful port of the fixed opponent curve, not a family to search.
const FEE_BPS: u128 = 30;
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

/// A meaningful zero (docs/DESIGN.md §2.8): the normalizer opponent's own mechanism
/// (`crates/shared/src/normalizer.rs::compute_swap`), reimplemented here against the
/// submission ABI rather than called directly — a submission always receives
/// `wincode`-decoded instruction data (see `ComputeSwapInstruction` above), unlike the
/// normalizer's own internal call path, which the simulator invokes with a distinct raw
/// little-endian byte layout (`crates/shared/src/normalizer.rs`'s own doc comment). The
/// arithmetic — ceiling-division constant product — is otherwise identical:
/// `net = input_amount * (10_000 - FEE_BPS) / 10_000`, `output = reserve - ceil(k / new_reserve)`.
///
/// `FEE_BPS = 30` is **not** per-simulation symmetric with the live opponent: the actual
/// opponent's fee is resampled every simulation as `norm_fee_bps ~ U[30, 80]`
/// (`crates/shared/src/config.rs::HyperparameterVariance`, injected into its storage —
/// `crates/sim/src/engine.rs::amm_norm.set_initial_storage`), and a submission has no
/// channel to observe that draw — it only ever sees its own reserves and storage, never the
/// opponent's. `30` is `SimulationConfig::default()`'s own `norm_fee_bps` and the floor of
/// the sampled range, so this is the opponent's mechanism at one fixed, representative point
/// within its range, not a per-simulation mirror. That is still a meaningful zero (§2.8): it
/// shows how the router splits flow between two AMMs running the *identical formula*, rather
/// than measuring some other, unrelated curve against the opponent.
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
