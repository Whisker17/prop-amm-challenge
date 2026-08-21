//! Update Oracle Instruction
//!
//! Ultra-optimized oracle update for piecewise linear curves.
//! Updates 7 price points per side, resets consumed. Does NOT touch quantities.
//! Quantities are determined by on-chain state only (deposits, withdrawals, swaps).
//! Target: < 1000 CUs

use pinocchio::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey, ProgramResult};

use crate::{error::PropAmmError, state::{POOL_DISCRIMINATOR, NUM_PRICE_POINTS, PiecewiseBookSide}};

// Byte offsets in Pool account (computed from struct layout)
const DISCRIMINATOR_OFFSET: usize = 0;
const AUTHORITY_OFFSET: usize = 9; // 8 (discriminator) + 1 (bump)

// Oracle-updatable fields: bid_side + ask_side (PiecewiseBookSide each)
// Offset 169 = 8 (disc) + 1 (bump) + 32*5 (authority + mints + vaults)
const BID_SIDE_OFFSET: usize = 169;
const ASK_SIDE_OFFSET: usize = BID_SIDE_OFFSET + PiecewiseBookSide::SIZE; // 169 + 72 = 241

/// Update Oracle instruction data layout (112 bytes) - PRICES ONLY
///
/// Layout:
/// - bid_prices: [u64; 7] (56 bytes) - Must be strictly ascending
/// - ask_prices: [u64; 7] (56 bytes) - Must be strictly ascending
///
/// Note: Quantities are NOT set by oracle. They come from on-chain state only.
#[repr(C)]
pub struct UpdateOracleData {
    pub bid_prices: [u64; NUM_PRICE_POINTS],
    pub ask_prices: [u64; NUM_PRICE_POINTS],
}

impl UpdateOracleData {
    /// Total size: 7*8 + 7*8 = 112 bytes (prices only, no quantities)
    pub const SIZE: usize = NUM_PRICE_POINTS * 8 * 2;

    /// Parse from instruction data bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }

        let mut offset = 0;

        // Bid prices
        let mut bid_prices = [0u64; NUM_PRICE_POINTS];
        for i in 0..NUM_PRICE_POINTS {
            bid_prices[i] = u64::from_le_bytes(data[offset..offset + 8].try_into().ok()?);
            offset += 8;
        }

        // Ask prices
        let mut ask_prices = [0u64; NUM_PRICE_POINTS];
        for i in 0..NUM_PRICE_POINTS {
            ask_prices[i] = u64::from_le_bytes(data[offset..offset + 8].try_into().ok()?);
            offset += 8;
        }

        Some(Self {
            bid_prices,
            ask_prices,
        })
    }
}

/// Process UpdateOracle instruction
///
/// Accounts:
/// 0. `[signer]` Authority - Must match pool.authority
/// 1. `[writable]` Pool - Pool account
///
/// Behavior:
/// - Replaces all 7 price points for each side
/// - Resets consumed to 0 (fresh curve position)
/// - Does NOT touch total_quantity (determined by on-chain state only)
///
/// Optimizations:
/// - Minimal validation for trusted oracle
/// - Direct byte manipulation where possible
#[inline(always)]
pub fn process_update_oracle(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    // Bounds check
    if accounts.len() < 2 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    // Parse instruction data
    let ix_data = UpdateOracleData::from_bytes(data)
        .ok_or(ProgramError::InvalidInstructionData)?;

    let authority = &accounts[0];
    let pool_account = &accounts[1];

    // Signer check
    if !authority.is_signer() {
        return Err(PropAmmError::NotSigner.into());
    }

    // Owner check
    if unsafe { pool_account.owner() } != program_id {
        return Err(PropAmmError::InvalidOwner.into());
    }

    // Single mutable borrow for all operations
    let mut pool_data = pool_account.try_borrow_mut_data()?;

    // Validate discriminator
    if pool_data[DISCRIMINATOR_OFFSET..DISCRIMINATOR_OFFSET + 8] != POOL_DISCRIMINATOR {
        return Err(PropAmmError::InvalidDiscriminator.into());
    }

    // Validate authority
    if &pool_data[AUTHORITY_OFFSET..AUTHORITY_OFFSET + 32] != authority.key().as_ref() {
        return Err(PropAmmError::Unauthorized.into());
    }

    // Optional: Validate prices are strictly ascending
    // (Can be removed if oracle is trusted)
    for i in 0..NUM_PRICE_POINTS - 1 {
        if ix_data.ask_prices[i] >= ix_data.ask_prices[i + 1] {
            return Err(PropAmmError::InvalidPrices.into());
        }
        if ix_data.bid_prices[i] >= ix_data.bid_prices[i + 1] {
            return Err(PropAmmError::InvalidPrices.into());
        }
    }

    // Write bid side - PRICES ONLY, skip total_quantity, reset consumed
    let mut offset = BID_SIDE_OFFSET;

    // Bid prices
    for i in 0..NUM_PRICE_POINTS {
        pool_data[offset..offset + 8].copy_from_slice(&ix_data.bid_prices[i].to_le_bytes());
        offset += 8;
    }

    // Skip total_quantity (don't modify it - determined by on-chain state)
    offset += 8;

    // Bid consumed = 0 (reset to start of curve)
    pool_data[offset..offset + 8].copy_from_slice(&0u64.to_le_bytes());
    offset += 8;

    // Write ask side (offset should now be ASK_SIDE_OFFSET)
    debug_assert_eq!(offset, ASK_SIDE_OFFSET);

    // Ask prices
    for i in 0..NUM_PRICE_POINTS {
        pool_data[offset..offset + 8].copy_from_slice(&ix_data.ask_prices[i].to_le_bytes());
        offset += 8;
    }

    // Skip total_quantity (don't modify it - determined by on-chain state)
    offset += 8;

    // Ask consumed = 0 (reset to start of curve)
    pool_data[offset..offset + 8].copy_from_slice(&0u64.to_le_bytes());

    Ok(())
}
