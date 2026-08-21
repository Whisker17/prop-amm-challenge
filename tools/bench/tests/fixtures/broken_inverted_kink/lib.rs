use pinocchio::{account_info::AccountInfo, entrypoint, pubkey::Pubkey, ProgramResult};
use prop_amm_submission_sdk::{set_return_data_bytes, set_return_data_u64};

const NAME: &str = "Broken Inverted Kink (WHI-1212 fixture)";
const MODEL_USED: &str = "None";
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
        // tag 2 = after_swap (no-op — this fixture needs no per-trade state)
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

/// Deliberately broken fixture (WHI-1212 acceptance criteria): a straight line up to a
/// 1000-Y kink — inside `bench fuzz`'s dense-sweep window but well beyond `prop-amm
/// validate`'s own 10-point, max-200 probe (`crates/cli/src/commands/validate.rs`) — after
/// which the marginal rate *increases* rather than decreasing. This is an inverted kink: a
/// concavity violation `bench fuzz` must catch and `prop-amm validate` cannot. Never
/// intended to be a real candidate — committed only as a known-bad fixture.
pub fn compute_swap(data: &[u8]) -> u64 {
    let decoded: ComputeSwapInstruction = match wincode::deserialize(data) {
        Ok(decoded) => decoded,
        Err(_) => return 0,
    };

    let input_amount = decoded.input_amount as u128;
    if decoded.reserve_x == 0 || decoded.reserve_y == 0 || input_amount == 0 {
        return 0;
    }

    // 1000.0 in the ABI's nano (1e9) fixed-point scale.
    const KINK_INPUT_NANO: u128 = 1_000 * 1_000_000_000;
    const SLOPE_BEFORE_NUM: u128 = 8; // marginal rate 0.8x before the kink
    const SLOPE_AFTER_NUM: u128 = 12; // marginal rate 1.2x after — steeper: the inversion
    const SLOPE_DEN: u128 = 10;

    let output = if input_amount <= KINK_INPUT_NANO {
        input_amount * SLOPE_BEFORE_NUM / SLOPE_DEN
    } else {
        let base = KINK_INPUT_NANO * SLOPE_BEFORE_NUM / SLOPE_DEN;
        let extra = (input_amount - KINK_INPUT_NANO) * SLOPE_AFTER_NUM / SLOPE_DEN;
        base + extra
    };

    output.min(u64::MAX as u128) as u64
}
