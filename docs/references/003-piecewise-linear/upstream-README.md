# Prop AMM

> **Warning:** This code was written entirely by AI (Claude) and has not been audited. Do not trust it with significant funds. Use at your own risk.

A piecewise linear AMM for Solana implementing the [Prop AMM model](https://benedict.dev/prop-amm) with 7-point price curves.

**Mainnet Program:** `13NnzndP9KHoqygdU6Bba1cy4EayKN3F3AX6F3PSALnK`

## Features

- **7-point piecewise linear curves** - 6 segments per side for expressive pricing
- **Self-replenishing liquidity** - Incoming tokens auto-credit to opposite side
- **Oracle sets prices only** - Quantities determined by on-chain state (deposits, withdrawals, swaps)
- **Consumed-based tracking** - Proportional scaling maintains curve position when liquidity changes
- **u64 with 10^9 scaling** - Efficient native operations
- **Zero dependencies** - Pinocchio framework
- **Ultra-low CU oracle updates** - ~150 CUs

## On-Chain Data Structure

### Pool Account (321 bytes)

| Field | Type | Size | Description |
|-------|------|------|-------------|
| discriminator | [u8; 8] | 8 | Account type identifier ("propamm2") |
| bump | u8 | 1 | PDA bump seed |
| authority | Pubkey | 32 | Oracle update authority |
| base_mint | Pubkey | 32 | Base token mint (e.g., NVDAX) |
| quote_mint | Pubkey | 32 | Quote token mint (e.g., USDC) |
| base_vault | Pubkey | 32 | Pool's base token account |
| quote_vault | Pubkey | 32 | Pool's quote token account |
| bid_side | PiecewiseBookSide | 72 | Bid liquidity curve |
| ask_side | PiecewiseBookSide | 72 | Ask liquidity curve |
| is_active | bool | 1 | Trading enabled flag |
| _padding | [u8; 7] | 7 | Reserved for alignment |

### PiecewiseBookSide (72 bytes)

| Field | Type | Size | Description |
|-------|------|------|-------------|
| prices | [u64; 7] | 56 | 7 price points (P0 < P1 < ... < P6) |
| total_quantity | u64 | 8 | Total liquidity (native token units) |
| consumed | u64 | 8 | Amount consumed by swaps |

**Key properties:**
- `remaining = total_quantity - consumed`
- `segment_quantity = total_quantity / 6`
- Ask side consumes P0 → P6 (low to high)
- Bid side consumes P6 → P0 (high to low)

## Price Convention

Prices are stored as **native-ratio scaled values**:

```
price = (quote_native_units / base_native_units) × PRICE_SCALE
```

Where `PRICE_SCALE = 10^9`.

**Example for NVDAX(8 dec)/USDC(6 dec) at $200:**
- 1 NVDAX = 10^8 base native units
- 200 USDC = 2×10^8 quote native units
- Native ratio = 2×10^8 / 10^8 = 2
- Stored price = 2 × 10^9 = 2,000,000,000

## Instructions

### Initialize (discriminator: 0)

Creates a new pool with empty book sides.

**Accounts:**
- `[signer]` authority
- `[writable]` pool (PDA)
- `[]` base_mint
- `[]` quote_mint
- `[]` system_program

**Data (2 bytes):** discriminator + bump

### UpdateOracle (discriminator: 1)

Sets prices and resets consumed. **Does NOT modify quantities.**

**Accounts:**
- `[signer]` authority
- `[writable]` pool

**Data (113 bytes):** discriminator + bid_prices[7] + ask_prices[7]

### Swap (discriminator: 2)

Execute buy or sell against the pool.

**Accounts:**
- `[signer]` user
- `[writable]` pool
- `[writable]` user_base_account
- `[writable]` user_quote_account
- `[writable]` pool_base_vault
- `[writable]` pool_quote_vault
- `[]` base_mint
- `[]` quote_mint
- `[]` base_token_program (SPL Token or Token-2022)
- `[]` quote_token_program

**Data (18 bytes):** discriminator + direction (0=Buy, 1=Sell) + amount_in + min_amount_out

### SetVaults (discriminator: 3)

One-time setup to configure vault addresses.

**Accounts:**
- `[signer]` authority
- `[writable]` pool
- `[]` base_vault
- `[]` quote_vault

### Deposit (discriminator: 4)

Deposit tokens to increase liquidity.

**Accounts:**
- `[signer]` authority
- `[writable]` pool
- `[writable]` authority_token_account
- `[writable]` pool_vault
- `[]` mint
- `[]` token_program

**Data (10 bytes):** discriminator + side (0=Base, 1=Quote) + amount

### Withdraw (discriminator: 5)

Withdraw tokens from pool.

Same accounts as Deposit.

## Self-Replenishing Liquidity

When a swap executes:
1. Incoming tokens are **auto-credited to the opposite side**
2. The `consumed` value is **proportionally scaled** to maintain curve position

**Example (Buy NVDAX with USDC):**
- User sends USDC → Pool receives USDC
- USDC credited to bid_side.total_quantity
- bid_side.consumed scaled: `new_consumed = old_consumed × new_total / old_total`

This means:
- Without oracle updates, liquidity cycles between sides indefinitely
- No "lost" tokens - everything stays in the pool
- Curve position is preserved when liquidity changes

## Building

```bash
# Build for Solana
cargo build-sbf

# Run tests
cargo test

# Deploy to mainnet
solana program deploy target/deploy/prop_amm.so \
  --program-id 13NnzndP9KHoqygdU6Bba1cy4EayKN3F3AX6F3PSALnK \
  --url mainnet-beta
```

## Client Usage

```typescript
import {
  createUpdateOracleInstruction,
  createSwapInstruction,
  humanToNativePrice,
  SWAP_BUY_EXACT_IN,
} from "./instructions";

// Update oracle (prices only, no quantities)
const updateOracleIx = createUpdateOracleInstruction(
  PROGRAM_ID,
  authority,
  poolPda,
  {
    bidPrices: [100, 120, 140, 160, 170, 178, 180].map(p =>
      humanToNativePrice(p, BASE_DECIMALS, QUOTE_DECIMALS)
    ),
    askPrices: [200, 205, 215, 230, 250, 280, 320].map(p =>
      humanToNativePrice(p, BASE_DECIMALS, QUOTE_DECIMALS)
    ),
  }
);

// Execute swap
const swapIx = createSwapInstruction(
  PROGRAM_ID,
  user,
  poolPda,
  userBaseAccount,
  userQuoteAccount,
  poolBaseVault,
  poolQuoteVault,
  baseMint,
  quoteMint,
  baseTokenProgram,
  quoteTokenProgram,
  {
    direction: SWAP_BUY_EXACT_IN,
    amountIn: toNativeUnits(100, QUOTE_DECIMALS),
    minAmountOut: 0n,
  }
);
```

## Project Structure

```
prop-amm/
├── Cargo.toml
├── README.md
├── .env.example
├── client/
│   ├── package.json
│   └── src/
│       ├── instructions.ts      # Instruction builders + pool decoder
│       ├── pda.ts               # PDA derivation
│       ├── init-piecewise-pool.ts  # Pool initialization script
│       └── update-pool.ts       # Oracle update script
└── programs/prop-amm/
    └── src/
        ├── lib.rs               # Entrypoint
        ├── error.rs             # PropAmmError enum
        ├── state.rs             # Pool, PiecewiseBookSide structs
        ├── pda.rs               # PDA derivation
        ├── token.rs             # Token transfer helpers
        ├── instructions/
        │   ├── mod.rs           # Instruction discriminators
        │   ├── initialize.rs
        │   ├── update_oracle.rs # Prices only, no quantities
        │   ├── swap.rs          # With auto-crediting
        │   ├── set_vaults.rs
        │   ├── deposit.rs
        │   └── withdraw.rs
        └── math/
            ├── mod.rs           # Core AMM formulas
            ├── piecewise.rs     # 7-point curve algorithms
            ├── scaled.rs        # Scaled arithmetic
            └── sqrt.rs          # Newton-Raphson sqrt
```

## License

MIT
