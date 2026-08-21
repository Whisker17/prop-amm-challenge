//! Prop AMM - Simple Linear AMM for Solana
//!
//! A minimal AMM implementation using linear liquidity curves with
//! u64 scaled arithmetic (10^9 scaling).

#![no_std]

pub mod error;
pub mod instructions;
pub mod math;
pub mod pda;
pub mod state;
pub mod token;

use instructions::{
    deposit::process_deposit, initialize::process_initialize, set_vaults::process_set_vaults,
    swap::process_swap, swap_event::process_swap_event, update_oracle::process_update_oracle,
    withdraw::process_withdraw, Instruction,
};
use pinocchio::{
    account_info::AccountInfo,
    default_allocator, nostd_panic_handler, program_entrypoint,
    program_error::ProgramError, pubkey::Pubkey,
    ProgramResult,
};

// For no_std, use individual macros instead of entrypoint!
program_entrypoint!(process_instruction);
default_allocator!();
nostd_panic_handler!();

/// Program entrypoint
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    // Instruction data must have at least 1 byte for discriminator
    if instruction_data.is_empty() {
        return Err(ProgramError::InvalidInstructionData);
    }

    // Parse instruction discriminator (first byte)
    let instruction = Instruction::try_from_u8(instruction_data[0])
        .ok_or(ProgramError::InvalidInstructionData)?;

    // Route to instruction handler (skip discriminator byte)
    let data = &instruction_data[1..];

    match instruction {
        Instruction::Initialize => process_initialize(program_id, accounts, data),
        Instruction::UpdateOracle => process_update_oracle(program_id, accounts, data),
        Instruction::Swap => process_swap(program_id, accounts, data),
        Instruction::SetVaults => process_set_vaults(program_id, accounts, data),
        Instruction::Deposit => process_deposit(program_id, accounts, data),
        Instruction::Withdraw => process_withdraw(program_id, accounts, data),
        Instruction::SwapEvent => process_swap_event(program_id, accounts, data),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instruction_discriminators() {
        assert_eq!(Instruction::try_from_u8(0), Some(Instruction::Initialize));
        assert_eq!(Instruction::try_from_u8(1), Some(Instruction::UpdateOracle));
        assert_eq!(Instruction::try_from_u8(2), Some(Instruction::Swap));
        assert_eq!(Instruction::try_from_u8(3), Some(Instruction::SetVaults));
        assert_eq!(Instruction::try_from_u8(4), Some(Instruction::Deposit));
        assert_eq!(Instruction::try_from_u8(5), Some(Instruction::Withdraw));
        assert_eq!(Instruction::try_from_u8(6), None);
    }
}
