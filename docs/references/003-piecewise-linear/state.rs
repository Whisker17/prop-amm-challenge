//! On-chain Account State
//!
//! Defines the Pool account structure for the Prop AMM.
//! Uses piecewise linear curves with 7 price points per side.

use pinocchio::pubkey::Pubkey;

/// Pool account discriminator (8 bytes)
/// Changed to "propamm2" for new version
pub const POOL_DISCRIMINATOR: [u8; 8] = *b"propamm2";

/// Scaling factor: 10^9 (1 billion)
/// - Prices: native-ratio scaled (quote_native/base_native * 10^9)
/// - Quantities: native token units
pub const SCALE: u64 = 1_000_000_000;

/// Number of price points per side
pub const NUM_PRICE_POINTS: usize = 7;

/// Number of segments per side (NUM_PRICE_POINTS - 1)
pub const NUM_SEGMENTS: usize = 6;

/// Piecewise linear book side representation
///
/// Stores liquidity distributed across 6 segments defined by 7 price points.
/// Equal quantity per segment: Q_segment = total_quantity / 6
///
/// For ask side: prices ascending (P0 < P1 < ... < P6), consumes low to high
/// For bid side: prices ascending (P0 < P1 < ... < P6), consumes high to low
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PiecewiseBookSide {
    /// 7 price levels (scaled by PRICE_SCALE)
    /// Must be in strictly ascending order: P0 < P1 < P2 < P3 < P4 < P5 < P6
    pub prices: [u64; NUM_PRICE_POINTS],

    /// Total quantity of liquidity (native token units)
    /// - Ask side: base token units
    /// - Bid side: quote token units
    pub total_quantity: u64,

    /// Amount consumed by swaps (native token units)
    /// remaining = total_quantity - consumed
    pub consumed: u64,
}

impl PiecewiseBookSide {
    /// Size of PiecewiseBookSide in bytes: 7*8 + 8 + 8 = 72
    pub const SIZE: usize = NUM_PRICE_POINTS * 8 + 8 + 8;

    /// Create a new empty PiecewiseBookSide
    pub const fn empty() -> Self {
        Self {
            prices: [0; NUM_PRICE_POINTS],
            total_quantity: 0,
            consumed: 0,
        }
    }

    /// Get remaining liquidity
    #[inline]
    pub fn remaining(&self) -> u64 {
        self.total_quantity.saturating_sub(self.consumed)
    }

    /// Get quantity per segment (equal distribution)
    #[inline]
    pub fn quantity_per_segment(&self) -> u64 {
        self.total_quantity / NUM_SEGMENTS as u64
    }

    /// Get current segment index based on consumption
    /// Returns 0-5 for valid segments, 6 if fully consumed
    #[inline]
    pub fn current_segment(&self) -> usize {
        let q_seg = self.quantity_per_segment();
        if q_seg == 0 {
            return NUM_SEGMENTS; // No liquidity
        }
        let seg = (self.consumed / q_seg) as usize;
        if seg >= NUM_SEGMENTS {
            NUM_SEGMENTS
        } else {
            seg
        }
    }

    /// Get consumption within current segment
    #[inline]
    pub fn consumed_in_segment(&self) -> u64 {
        let q_seg = self.quantity_per_segment();
        if q_seg == 0 {
            return 0;
        }
        self.consumed % q_seg
    }
}

/// Main Pool account storing AMM state
///
/// Total size: 321 bytes
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Pool {
    /// Account discriminator for type identification (8 bytes)
    pub discriminator: [u8; 8],

    /// PDA bump seed for address derivation (1 byte)
    pub bump: u8,

    /// Pool authority - can update oracle (32 bytes)
    pub authority: Pubkey,

    /// Base token mint (32 bytes)
    pub base_mint: Pubkey,

    /// Quote token mint (32 bytes)
    pub quote_mint: Pubkey,

    /// Pool's base token vault (32 bytes)
    pub base_vault: Pubkey,

    /// Pool's quote token vault (32 bytes)
    pub quote_vault: Pubkey,

    /// Bid side liquidity curve - quote buying base (72 bytes)
    pub bid_side: PiecewiseBookSide,

    /// Ask side liquidity curve - base being sold (72 bytes)
    pub ask_side: PiecewiseBookSide,

    /// Pool is active for trading (1 byte)
    pub is_active: bool,

    /// Reserved padding for alignment and future use (7 bytes)
    pub _padding: [u8; 7],
}

impl Pool {
    /// Total size of Pool account in bytes
    /// 8 + 1 + 32*5 + 72*2 + 1 + 7 = 321 bytes
    pub const SIZE: usize = 8      // discriminator
        + 1                         // bump
        + 32                        // authority
        + 32                        // base_mint
        + 32                        // quote_mint
        + 32                        // base_vault
        + 32                        // quote_vault
        + PiecewiseBookSide::SIZE   // bid_side (72)
        + PiecewiseBookSide::SIZE   // ask_side (72)
        + 1                         // is_active
        + 7;                        // padding

    /// Create a new Pool with default values
    pub fn new(
        bump: u8,
        authority: Pubkey,
        base_mint: Pubkey,
        quote_mint: Pubkey,
        base_vault: Pubkey,
        quote_vault: Pubkey,
    ) -> Self {
        Self {
            discriminator: POOL_DISCRIMINATOR,
            bump,
            authority,
            base_mint,
            quote_mint,
            base_vault,
            quote_vault,
            bid_side: PiecewiseBookSide::empty(),
            ask_side: PiecewiseBookSide::empty(),
            is_active: false,
            _padding: [0; 7],
        }
    }

    /// Check if the discriminator is valid
    #[inline]
    pub fn is_valid_discriminator(&self) -> bool {
        self.discriminator == POOL_DISCRIMINATOR
    }

    /// Serialize Pool to bytes
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        let mut offset = 0;

        // discriminator
        bytes[offset..offset + 8].copy_from_slice(&self.discriminator);
        offset += 8;

        // bump
        bytes[offset] = self.bump;
        offset += 1;

        // authority
        bytes[offset..offset + 32].copy_from_slice(self.authority.as_ref());
        offset += 32;

        // base_mint
        bytes[offset..offset + 32].copy_from_slice(self.base_mint.as_ref());
        offset += 32;

        // quote_mint
        bytes[offset..offset + 32].copy_from_slice(self.quote_mint.as_ref());
        offset += 32;

        // base_vault
        bytes[offset..offset + 32].copy_from_slice(self.base_vault.as_ref());
        offset += 32;

        // quote_vault
        bytes[offset..offset + 32].copy_from_slice(self.quote_vault.as_ref());
        offset += 32;

        // bid_side prices
        for i in 0..NUM_PRICE_POINTS {
            bytes[offset..offset + 8].copy_from_slice(&self.bid_side.prices[i].to_le_bytes());
            offset += 8;
        }
        bytes[offset..offset + 8].copy_from_slice(&self.bid_side.total_quantity.to_le_bytes());
        offset += 8;
        bytes[offset..offset + 8].copy_from_slice(&self.bid_side.consumed.to_le_bytes());
        offset += 8;

        // ask_side prices
        for i in 0..NUM_PRICE_POINTS {
            bytes[offset..offset + 8].copy_from_slice(&self.ask_side.prices[i].to_le_bytes());
            offset += 8;
        }
        bytes[offset..offset + 8].copy_from_slice(&self.ask_side.total_quantity.to_le_bytes());
        offset += 8;
        bytes[offset..offset + 8].copy_from_slice(&self.ask_side.consumed.to_le_bytes());
        offset += 8;

        // is_active
        bytes[offset] = self.is_active as u8;
        offset += 1;

        // padding
        bytes[offset..offset + 7].copy_from_slice(&self._padding);

        bytes
    }

    /// Deserialize Pool from bytes
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::SIZE {
            return None;
        }

        let mut offset = 0;

        // discriminator
        let mut discriminator = [0u8; 8];
        discriminator.copy_from_slice(&bytes[offset..offset + 8]);
        offset += 8;

        // bump
        let bump = bytes[offset];
        offset += 1;

        // authority
        let authority: Pubkey = bytes[offset..offset + 32].try_into().ok()?;
        offset += 32;

        // base_mint
        let base_mint: Pubkey = bytes[offset..offset + 32].try_into().ok()?;
        offset += 32;

        // quote_mint
        let quote_mint: Pubkey = bytes[offset..offset + 32].try_into().ok()?;
        offset += 32;

        // base_vault
        let base_vault: Pubkey = bytes[offset..offset + 32].try_into().ok()?;
        offset += 32;

        // quote_vault
        let quote_vault: Pubkey = bytes[offset..offset + 32].try_into().ok()?;
        offset += 32;

        // bid_side
        let mut bid_prices = [0u64; NUM_PRICE_POINTS];
        for i in 0..NUM_PRICE_POINTS {
            bid_prices[i] = u64::from_le_bytes(bytes[offset..offset + 8].try_into().ok()?);
            offset += 8;
        }
        let bid_total_quantity = u64::from_le_bytes(bytes[offset..offset + 8].try_into().ok()?);
        offset += 8;
        let bid_consumed = u64::from_le_bytes(bytes[offset..offset + 8].try_into().ok()?);
        offset += 8;

        // ask_side
        let mut ask_prices = [0u64; NUM_PRICE_POINTS];
        for i in 0..NUM_PRICE_POINTS {
            ask_prices[i] = u64::from_le_bytes(bytes[offset..offset + 8].try_into().ok()?);
            offset += 8;
        }
        let ask_total_quantity = u64::from_le_bytes(bytes[offset..offset + 8].try_into().ok()?);
        offset += 8;
        let ask_consumed = u64::from_le_bytes(bytes[offset..offset + 8].try_into().ok()?);
        offset += 8;

        // is_active
        let is_active = bytes[offset] != 0;
        offset += 1;

        // padding
        let mut _padding = [0u8; 7];
        _padding.copy_from_slice(&bytes[offset..offset + 7]);

        Some(Self {
            discriminator,
            bump,
            authority,
            base_mint,
            quote_mint,
            base_vault,
            quote_vault,
            bid_side: PiecewiseBookSide {
                prices: bid_prices,
                total_quantity: bid_total_quantity,
                consumed: bid_consumed,
            },
            ask_side: PiecewiseBookSide {
                prices: ask_prices,
                total_quantity: ask_total_quantity,
                consumed: ask_consumed,
            },
            is_active,
            _padding,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_size() {
        assert_eq!(Pool::SIZE, 321);
    }

    #[test]
    fn test_bookside_size() {
        assert_eq!(PiecewiseBookSide::SIZE, 72);
    }

    #[test]
    fn test_pool_serialization_roundtrip() {
        let zero_pubkey: Pubkey = [0u8; 32];
        let mut pool = Pool::new(
            255,
            zero_pubkey,
            zero_pubkey,
            zero_pubkey,
            zero_pubkey,
            zero_pubkey,
        );

        // Set some prices
        pool.ask_side.prices = [100, 150, 200, 250, 300, 350, 400];
        pool.ask_side.total_quantity = 1_000_000;
        pool.ask_side.consumed = 100_000;

        pool.bid_side.prices = [50, 75, 100, 125, 150, 175, 200];
        pool.bid_side.total_quantity = 500_000;
        pool.bid_side.consumed = 50_000;

        let bytes = pool.to_bytes();
        let deserialized = Pool::from_bytes(&bytes).unwrap();

        assert_eq!(pool.discriminator, deserialized.discriminator);
        assert_eq!(pool.bump, deserialized.bump);
        assert_eq!(pool.is_active, deserialized.is_active);
        assert_eq!(pool.ask_side.prices, deserialized.ask_side.prices);
        assert_eq!(pool.ask_side.total_quantity, deserialized.ask_side.total_quantity);
        assert_eq!(pool.ask_side.consumed, deserialized.ask_side.consumed);
        assert_eq!(pool.bid_side.prices, deserialized.bid_side.prices);
        assert_eq!(pool.bid_side.total_quantity, deserialized.bid_side.total_quantity);
        assert_eq!(pool.bid_side.consumed, deserialized.bid_side.consumed);
    }

    #[test]
    fn test_segment_tracking() {
        let side = PiecewiseBookSide {
            prices: [100, 200, 300, 400, 500, 600, 700],
            total_quantity: 600_000, // 100k per segment
            consumed: 250_000,       // 2 full segments + 50k
        };

        assert_eq!(side.quantity_per_segment(), 100_000);
        assert_eq!(side.current_segment(), 2); // In segment 2
        assert_eq!(side.consumed_in_segment(), 50_000);
        assert_eq!(side.remaining(), 350_000);
    }
}
