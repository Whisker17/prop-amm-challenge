use pinocchio::{account_info::AccountInfo, entrypoint, pubkey::Pubkey, ProgramResult};
use prop_amm_submission_sdk::{set_return_data_bytes, set_return_data_u64};

const NAME: &str = "WHI-1213 fixture";
const MODEL_USED: &str = "test-fixture";
// === PARAMS BEGIN ===
const MODE: u128 = 0; // range: 0..=1
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
        0 | 1 => {
            let output = compute_swap(instruction_data);
            set_return_data_u64(output);
        }
        2 => {}
        3 => set_return_data_bytes(NAME.as_bytes()),
        4 => set_return_data_bytes(get_model_used().as_bytes()),
        _ => {}
    }

    Ok(())
}

pub fn get_model_used() -> &'static str {
    MODEL_USED
}

/// A fixture for WHI-1213: not a real strategy, just a controllable way to exercise
/// `bench fit`'s handling of a search point that panics `crates/sim`'s runtime shape check.
///
/// `MODE == 0` is an ordinary fixed-fee CPMM payout (concave/monotonic, mirrors
/// `strategies/001-cpmm-fee`) — always shape-check safe. `MODE == 1` deliberately returns a
/// payout that *decreases* as the input size grows, which trips
/// `curve_checks::submission_shape_violation`'s monotonicity check on essentially the first
/// routed order — deterministically, not dependent on input-size flakiness.
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

    let reserve_out = if side == 0 { reserve_y } else { reserve_x };

    if MODE == 1 {
        return reserve_out.saturating_sub(input_amount) as u64;
    }

    let k = reserve_x * reserve_y;
    // Not a §3.1-citeable tunable — this fixture has no fitted parameter, only the frozen
    // `MODE` switch above; any fee that keeps this branch concave/monotonic works equally
    // well as the "safe" side of the fixture.
    const FEE_BPS: u128 = 100;
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
