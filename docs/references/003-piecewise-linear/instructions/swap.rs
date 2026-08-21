//! Swap Instruction
//!
//! Executes buy or sell swaps against the pool using piecewise linear curves.
//! Supports both SPL Token and Token-2022 tokens.
//!
//! ## Price Convention
//! Prices are stored as native-ratio scaled values:
//! `price = (quote_native / base_native) * PRICE_SCALE`
//!
//! Each side has 7 price points creating 6 segments with equal quantity distribution.

use pinocchio::{
    account_info::AccountInfo,
    instruction::Signer,
    log::sol_log_data,
    program_error::ProgramError,
    pubkey::Pubkey,
    seeds, ProgramResult,
};

use crate::{
    error::PropAmmError,
    math::{buy_base_piecewise, sell_base_piecewise},
    pda::POOL_SEED,
    state::Pool,
    token::{get_mint_decimals, transfer_tokens},
};

/// Swap direction
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SwapDirection {
    /// Buy base token with quote token (e.g., USDC -> NVDAX)
    /// Consumes ask side liquidity
    BuyBaseWithQuote = 0,

    /// Sell base token for quote token (e.g., NVDAX -> USDC)
    /// Consumes bid side liquidity
    SellBaseForQuote = 1,
}

impl SwapDirection {
    pub fn try_from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(SwapDirection::BuyBaseWithQuote),
            1 => Some(SwapDirection::SellBaseForQuote),
            _ => None,
        }
    }
}

/// Swap instruction data layout
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SwapData {
    /// Direction of swap
    pub direction: SwapDirection,
    /// Amount input (native token units)
    /// - BuyBaseWithQuote: quote token units to spend
    /// - SellBaseForQuote: base token units to sell
    pub amount_in: u64,
    /// Minimum amount out (native token units) - slippage protection
    pub min_amount_out: u64,
}

impl SwapData {
    pub const SIZE: usize = 1 + 8 + 8; // 17 bytes

    /// Parse from bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }

        let direction = SwapDirection::try_from_u8(data[0])?;
        let amount_in = u64::from_le_bytes(data[1..9].try_into().ok()?);
        let min_amount_out = u64::from_le_bytes(data[9..17].try_into().ok()?);

        Some(Self {
            direction,
            amount_in,
            min_amount_out,
        })
    }
}

/// Process Swap instruction
///
/// Accounts:
/// 0. `[signer]` User - Trader
/// 1. `[writable]` Pool - Pool account
/// 2. `[writable]` User Base Account - User's base token account
/// 3. `[writable]` User Quote Account - User's quote token account
/// 4. `[writable]` Pool Base Vault - Pool's base token account
/// 5. `[writable]` Pool Quote Vault - Pool's quote token account
/// 6. `[]` Base Mint - Base token mint
/// 7. `[]` Quote Mint - Quote token mint
/// 8. `[]` Base Token Program - SPL Token or Token-2022
/// 9. `[]` Quote Token Program - SPL Token or Token-2022
pub fn process_swap(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    // Parse instruction data
    let ix_data = SwapData::from_bytes(data).ok_or(ProgramError::InvalidInstructionData)?;

    // Validate accounts
    if accounts.len() < 10 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    let user = &accounts[0];
    let pool_account = &accounts[1];
    let user_base_account = &accounts[2];
    let user_quote_account = &accounts[3];
    let pool_base_vault = &accounts[4];
    let pool_quote_vault = &accounts[5];
    let base_mint = &accounts[6];
    let quote_mint = &accounts[7];
    let base_token_program = &accounts[8];
    let quote_token_program = &accounts[9];

    // User must be signer
    if !user.is_signer() {
        return Err(PropAmmError::NotSigner.into());
    }

    // Validate writable accounts
    if !pool_account.is_writable()
        || !user_base_account.is_writable()
        || !user_quote_account.is_writable()
        || !pool_base_vault.is_writable()
        || !pool_quote_vault.is_writable()
    {
        return Err(PropAmmError::NotWritable.into());
    }

    // Pool must be owned by this program
    if unsafe { pool_account.owner() } != program_id {
        return Err(PropAmmError::InvalidOwner.into());
    }

    // Validate amount
    if ix_data.amount_in == 0 {
        return Err(PropAmmError::ZeroAmount.into());
    }

    // Load pool
    let pool_data = pool_account.try_borrow_data()?;
    if pool_data.len() < Pool::SIZE {
        return Err(PropAmmError::AccountDataTooSmall.into());
    }

    let mut pool = Pool::from_bytes(&pool_data).ok_or(PropAmmError::InvalidDiscriminator)?;

    if !pool.is_valid_discriminator() {
        return Err(PropAmmError::InvalidDiscriminator.into());
    }

    // Check pool is active
    if !pool.is_active {
        return Err(PropAmmError::PoolNotActive.into());
    }

    // Validate mints match pool
    if base_mint.key() != &pool.base_mint {
        return Err(PropAmmError::InvalidMint.into());
    }
    if quote_mint.key() != &pool.quote_mint {
        return Err(PropAmmError::InvalidMint.into());
    }

    // Validate vaults match pool
    if pool_base_vault.key() != &pool.base_vault {
        return Err(PropAmmError::InvalidTokenAccount.into());
    }
    if pool_quote_vault.key() != &pool.quote_vault {
        return Err(PropAmmError::InvalidTokenAccount.into());
    }

    // Get decimals for transfer_checked (required by SPL Token)
    let base_decimals = get_mint_decimals(base_mint)?;
    let quote_decimals = get_mint_decimals(quote_mint)?;

    drop(pool_data);

    // Execute swap using piecewise math
    let (transfer_in_amount, transfer_out_amount) = match ix_data.direction {
        SwapDirection::BuyBaseWithQuote => {
            // User spends quote to buy base (consumes ask side)
            let (base_bought, quote_used, new_consumed) = buy_base_piecewise(
                ix_data.amount_in,
                &pool.ask_side.prices,
                pool.ask_side.total_quantity,
                pool.ask_side.consumed,
            )
            .ok_or(PropAmmError::MathOverflow)?;

            // Check slippage
            if base_bought < ix_data.min_amount_out {
                return Err(PropAmmError::SlippageExceeded.into());
            }

            // Update pool state
            pool.ask_side.consumed = new_consumed;

            // Credit incoming USDC to bid side
            // FIRST: heal consumed (move curve back toward spread)
            // THEN: add remainder to total only after consumed hits 0
            if quote_used > 0 {
                if pool.bid_side.consumed >= quote_used {
                    // All incoming heals consumed
                    pool.bid_side.consumed -= quote_used;
                } else {
                    // Partial heal + add remainder to total
                    let remainder = quote_used - pool.bid_side.consumed;
                    pool.bid_side.consumed = 0;
                    pool.bid_side.total_quantity += remainder;
                }
            }

            // Transfer: User sends quote, receives base
            (quote_used, base_bought)
        }
        SwapDirection::SellBaseForQuote => {
            // User sells base for quote (consumes bid side)
            let (quote_received, base_used, new_consumed) = sell_base_piecewise(
                ix_data.amount_in,
                &pool.bid_side.prices,
                pool.bid_side.total_quantity,
                pool.bid_side.consumed,
            )
            .ok_or(PropAmmError::MathOverflow)?;

            // Check slippage
            if quote_received < ix_data.min_amount_out {
                return Err(PropAmmError::SlippageExceeded.into());
            }

            // Update pool state
            pool.bid_side.consumed = new_consumed;

            // Credit incoming NVDAX to ask side
            // FIRST: heal consumed (move curve back toward spread)
            // THEN: add remainder to total only after consumed hits 0
            if base_used > 0 {
                if pool.ask_side.consumed >= base_used {
                    // All incoming heals consumed
                    pool.ask_side.consumed -= base_used;
                } else {
                    // Partial heal + add remainder to total
                    let remainder = base_used - pool.ask_side.consumed;
                    pool.ask_side.consumed = 0;
                    pool.ask_side.total_quantity += remainder;
                }
            }

            // Transfer: User sends base, receives quote
            (base_used, quote_received)
        }
    };

    // Write updated pool state
    let pool_bytes = pool.to_bytes();
    let mut pool_data = pool_account.try_borrow_mut_data()?;
    pool_data[..Pool::SIZE].copy_from_slice(&pool_bytes);
    drop(pool_data);

    // Perform token transfers
    let bump_seed = [pool.bump];
    let pool_signer_seeds = seeds!(
        POOL_SEED,
        pool.base_mint.as_ref(),
        pool.quote_mint.as_ref(),
        &bump_seed
    );

    match ix_data.direction {
        SwapDirection::BuyBaseWithQuote => {
            // User sends quote to pool (user signs)
            transfer_tokens(
                user_quote_account,
                pool_quote_vault,
                user,
                quote_mint,
                quote_token_program,
                transfer_in_amount,
                quote_decimals,
                &[],
            )?;

            // Pool sends base to user (PDA signs)
            transfer_tokens(
                pool_base_vault,
                user_base_account,
                pool_account,
                base_mint,
                base_token_program,
                transfer_out_amount,
                base_decimals,
                &[Signer::from(&pool_signer_seeds)],
            )?;
        }
        SwapDirection::SellBaseForQuote => {
            // User sends base to pool (user signs)
            transfer_tokens(
                user_base_account,
                pool_base_vault,
                user,
                base_mint,
                base_token_program,
                transfer_in_amount,
                base_decimals,
                &[],
            )?;

            // Pool sends quote to user (PDA signs)
            transfer_tokens(
                pool_quote_vault,
                user_quote_account,
                pool_account,
                quote_mint,
                quote_token_program,
                transfer_out_amount,
                quote_decimals,
                &[Signer::from(&pool_signer_seeds)],
            )?;
        }
    }

    // Emit SwapEvent via sol_log_data for trade logging
    // Data layout: discriminator(1) + user(32) + direction(1) + amount_in(8) + amount_out(8) = 50 bytes
    let mut event_data = [0u8; 50];
    event_data[0] = 6; // SwapEvent discriminator
    event_data[1..33].copy_from_slice(user.key().as_ref());
    event_data[33] = ix_data.direction as u8;
    event_data[34..42].copy_from_slice(&transfer_in_amount.to_le_bytes());
    event_data[42..50].copy_from_slice(&transfer_out_amount.to_le_bytes());

    sol_log_data(&[&event_data]);

    Ok(())
}
