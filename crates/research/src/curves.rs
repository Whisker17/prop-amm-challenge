//! Adapters that expose each ported curve through the simulation's existing
//! program interface, without changing that interface.
//!
//! The simulation calls a curve as `fn(&[u8]) -> u64`:
//! `side | input_amount | reserve_x | reserve_y | storage[1024]`, all amounts in
//! `nano` (1e9) fixed point (see `prop_amm_shared::instruction`). Optionally it
//! calls `fn(&[u8], &mut [u8])` after a trade, which is the only place a curve
//! may mutate its storage.
//!
//! That gives the research harness everything it needs with **zero** changes to
//! `prop-amm-sim`:
//!
//! * curve parameters (`K`, `concentration`, targets) live in storage,
//! * the oracle publishes a price by overwriting `storage[0..32]` — every curve
//!   here keeps its oracle-published field first, so
//!   `BpfAmm::set_initial_storage` can be used as the publish path,
//!   and
//! * DODO's target / `RState` persistence happens in `after_swap`.
//!
//! Side convention (from `prop_amm_shared::instruction`):
//! `side = 0` spends Y to receive X, `side = 1` spends X to receive Y. X is the
//! base token and Y the quote token, so `side = 1` is DODO's `sellBaseToken`.

use std::cell::Cell;

use prop_amm_shared::instruction::{decode_after_swap, decode_instruction, STORAGE_SIZE};

use crate::dodo::state::DodoPool;
use crate::dodo::RState;
use crate::flashbots::FlashbotsPool;
use crate::u256::U256;
use crate::univ2;
use crate::wad::{nano_to_wad, wad_to_nano};

thread_local! {
    /// Count of curve calls that would have reverted on chain, despite
    /// non-degenerate inputs.
    ///
    /// This is a health signal, not part of any curve: a healthy benchmark run
    /// should report zero. The simulation runs each single-seed run to
    /// completion on one thread, so a thread-local counter attributes reverts to
    /// the right run.
    static REVERT_COUNT: Cell<u64> = const { Cell::new(0) };
}

/// Reset the per-thread revert counter (call before a run).
pub fn reset_revert_count() {
    REVERT_COUNT.with(|count| count.set(0));
}

/// Read the per-thread revert counter (call after a run).
pub fn revert_count() -> u64 {
    REVERT_COUNT.with(|count| count.get())
}

fn record_revert() {
    REVERT_COUNT.with(|count| count.set(count.get().saturating_add(1)));
}

/// Byte offset of the oracle-published field in every research curve's storage.
pub const ORACLE_OFFSET: usize = 0;
/// Size of the oracle-published field.
pub const ORACLE_LEN: usize = 32;

/// Encode a published price for `BpfAmm::set_initial_storage`, which copies from
/// offset 0 and therefore overwrites exactly the oracle field.
pub fn oracle_publish_bytes(price_wad: U256) -> [u8; ORACLE_LEN] {
    price_wad.to_le_bytes()
}

fn read_u256(storage: &[u8], offset: usize) -> U256 {
    if offset >= storage.len() {
        return U256::ZERO;
    }
    let end = (offset + 32).min(storage.len());
    U256::from_le_bytes(&storage[offset..end])
}

fn write_u256(storage: &mut [u8], offset: usize, value: U256) {
    let bytes = value.to_le_bytes();
    let end = (offset + 32).min(storage.len());
    if offset < end {
        storage[offset..end].copy_from_slice(&bytes[..end - offset]);
    }
}

// =============================== DODO ===============================

/// DODO PMM storage layout (little-endian `uint256` unless noted).
///
/// | Offset | Size | Field |
/// | --- | --- | --- |
/// | 0 | 32 | `i` — oracle-published guide price (WAD) |
/// | 32 | 32 | `K` — curve parameter |
/// | 64 | 32 | `B0` — persistent target base |
/// | 96 | 32 | `Q0` — persistent target quote |
/// | 128 | 32 | `B` — base reserve as of the last commit |
/// | 160 | 32 | `Q` — quote reserve as of the last commit |
/// | 192 | 32 | `lpFeeRate` (pinned to 0 in this benchmark) |
/// | 224 | 1 | `RState` (0 = ONE, 1 = ABOVE_ONE, 2 = BELOW_ONE) |
pub mod dodo_storage {
    pub const I: usize = 0;
    pub const K: usize = 32;
    pub const TARGET_BASE: usize = 64;
    pub const TARGET_QUOTE: usize = 96;
    pub const RESERVE_BASE_COMMITTED: usize = 128;
    pub const RESERVE_QUOTE_COMMITTED: usize = 160;
    pub const LP_FEE_RATE: usize = 192;
    pub const R_STATE: usize = 224;
    pub const USED: usize = 225;
}

/// Build the initial DODO storage blob.
///
/// `B = B0 = reserve_base` and `Q = Q0 = reserve_quote` with `R = ONE`, which is
/// the benchmark's unified initial state.
pub fn dodo_initial_storage(
    price_wad: U256,
    k: U256,
    reserve_base_wad: U256,
    reserve_quote_wad: U256,
    lp_fee_rate: U256,
) -> Vec<u8> {
    let mut storage = vec![0u8; STORAGE_SIZE];
    write_u256(&mut storage, dodo_storage::I, price_wad);
    write_u256(&mut storage, dodo_storage::K, k);
    write_u256(&mut storage, dodo_storage::TARGET_BASE, reserve_base_wad);
    write_u256(&mut storage, dodo_storage::TARGET_QUOTE, reserve_quote_wad);
    write_u256(
        &mut storage,
        dodo_storage::RESERVE_BASE_COMMITTED,
        reserve_base_wad,
    );
    write_u256(
        &mut storage,
        dodo_storage::RESERVE_QUOTE_COMMITTED,
        reserve_quote_wad,
    );
    write_u256(&mut storage, dodo_storage::LP_FEE_RATE, lp_fee_rate);
    storage[dodo_storage::R_STATE] = RState::One.as_u8();
    storage
}

fn dodo_pool_from_storage(storage: &[u8]) -> Option<DodoPool> {
    Some(DodoPool {
        i: read_u256(storage, dodo_storage::I),
        k: read_u256(storage, dodo_storage::K),
        target_base: read_u256(storage, dodo_storage::TARGET_BASE),
        target_quote: read_u256(storage, dodo_storage::TARGET_QUOTE),
        r_state: RState::from_u8(*storage.get(dodo_storage::R_STATE)?)?,
        lp_fee_rate: read_u256(storage, dodo_storage::LP_FEE_RATE),
    })
}

/// `compute_swap` for the DODO PMM curve.
pub fn dodo_compute_swap(data: &[u8]) -> u64 {
    if data.len() < 25 + dodo_storage::USED {
        return 0;
    }
    let (side, input_amount, reserve_x, reserve_y) = decode_instruction(data);
    if input_amount == 0 || reserve_x == 0 || reserve_y == 0 {
        return 0;
    }
    let storage = &data[25..];
    let Some(pool) = dodo_pool_from_storage(storage) else {
        return 0;
    };
    let sell_base = match side {
        0 => false, // Y in, X out -> sellQuoteToken
        1 => true,  // X in, Y out -> sellBaseToken
        _ => return 0,
    };
    let Some(result) = pool.quote(
        nano_to_wad(reserve_x),
        nano_to_wad(reserve_y),
        sell_base,
        nano_to_wad(input_amount),
    ) else {
        record_revert(); // the on-chain call would have reverted
        return 0;
    };
    wad_to_nano(result.amount_out)
}

/// `after_swap` for the DODO PMM curve.
///
/// Re-evaluates the executed trade from the reserves recorded at the previous
/// commit — which are exactly the pre-trade reserves, because the simulation only
/// changes reserves through an executed swap — and then applies the on-chain
/// persistence rule from `MantlePropAmmPool.swap`.
pub fn dodo_after_swap(data: &[u8], storage: &mut [u8]) {
    if data.len() < 42 || storage.len() < dodo_storage::USED {
        return;
    }
    let (side, input_amount, _output_amount, reserve_x, reserve_y, _step, _) =
        decode_after_swap(data);
    let Some(mut pool) = dodo_pool_from_storage(storage) else {
        return;
    };
    let sell_base = match side {
        0 => false,
        1 => true,
        _ => return,
    };

    let pre_base = read_u256(storage, dodo_storage::RESERVE_BASE_COMMITTED);
    let pre_quote = read_u256(storage, dodo_storage::RESERVE_QUOTE_COMMITTED);
    if let Some(result) = pool.quote(pre_base, pre_quote, sell_base, nano_to_wad(input_amount)) {
        pool.commit(sell_base, &result);
        write_u256(storage, dodo_storage::TARGET_BASE, pool.target_base);
        write_u256(storage, dodo_storage::TARGET_QUOTE, pool.target_quote);
        storage[dodo_storage::R_STATE] = pool.r_state.as_u8();
    } else {
        record_revert();
    }

    // The simulation ledger owns the reserves; record what it now holds.
    write_u256(
        storage,
        dodo_storage::RESERVE_BASE_COMMITTED,
        nano_to_wad(reserve_x),
    );
    write_u256(
        storage,
        dodo_storage::RESERVE_QUOTE_COMMITTED,
        nano_to_wad(reserve_y),
    );
}

// ============================= Flashbots =============================

/// Flashbots `ExamplePropAmm` storage layout (little-endian `uint256`).
///
/// | Offset | Size | Field |
/// | --- | --- | --- |
/// | 0 | 32 | `multX` — oracle-published price multiplier for X (WAD) |
/// | 32 | 32 | `multY` — price multiplier for Y (1e18 here) |
/// | 64 | 32 | `concentration` |
/// | 96 | 32 | `targetX` |
pub mod flashbots_storage {
    pub const MULT_X: usize = 0;
    pub const MULT_Y: usize = 32;
    pub const CONCENTRATION: usize = 64;
    pub const TARGET_X: usize = 96;
    pub const USED: usize = 128;
}

pub fn flashbots_initial_storage(
    price_wad: U256,
    mult_y: U256,
    concentration: U256,
    target_x_wad: U256,
) -> Vec<u8> {
    let mut storage = vec![0u8; STORAGE_SIZE];
    write_u256(&mut storage, flashbots_storage::MULT_X, price_wad);
    write_u256(&mut storage, flashbots_storage::MULT_Y, mult_y);
    write_u256(
        &mut storage,
        flashbots_storage::CONCENTRATION,
        concentration,
    );
    write_u256(&mut storage, flashbots_storage::TARGET_X, target_x_wad);
    storage
}

fn flashbots_pool_from_storage(storage: &[u8]) -> FlashbotsPool {
    FlashbotsPool {
        concentration: read_u256(storage, flashbots_storage::CONCENTRATION),
        mult_x: read_u256(storage, flashbots_storage::MULT_X),
        mult_y: read_u256(storage, flashbots_storage::MULT_Y),
        target_x: read_u256(storage, flashbots_storage::TARGET_X),
    }
}

/// `compute_swap` for the Flashbots prop-AMM curve. Natively fee-free; no fee
/// wrapper is applied.
pub fn flashbots_compute_swap(data: &[u8]) -> u64 {
    if data.len() < 25 + flashbots_storage::USED {
        return 0;
    }
    let (side, input_amount, reserve_x, reserve_y) = decode_instruction(data);
    if input_amount == 0 || reserve_x == 0 || reserve_y == 0 {
        return 0;
    }
    let pool = flashbots_pool_from_storage(&data[25..]);
    let reserve_x_wad = nano_to_wad(reserve_x);
    let amount_in = nano_to_wad(input_amount);
    let quote = match side {
        0 => pool.quote_y_to_x(reserve_x_wad, amount_in),
        1 => pool.quote_x_to_y(reserve_x_wad, amount_in),
        _ => None,
    };
    match quote {
        Some(amount_out) => wad_to_nano(amount_out),
        None => {
            record_revert();
            0
        }
    }
}

// ============================== Uniswap V2 ==============================

/// `compute_swap` for the zero-fee Uniswap V2 baseline. Stateless.
pub fn univ2_compute_swap(data: &[u8]) -> u64 {
    if data.len() < 25 {
        return 0;
    }
    let (side, input_amount, reserve_x, reserve_y) = decode_instruction(data);
    if input_amount == 0 || reserve_x == 0 || reserve_y == 0 {
        return 0;
    }
    let amount_in = nano_to_wad(input_amount);
    let rx = nano_to_wad(reserve_x);
    let ry = nano_to_wad(reserve_y);
    let quote = match side {
        0 => univ2::get_amount_out(amount_in, ry, rx),
        1 => univ2::get_amount_out(amount_in, rx, ry),
        _ => None,
    };
    match quote {
        Some(amount_out) => wad_to_nano(amount_out),
        None => {
            record_revert();
            0
        }
    }
}

// ========================= Null competitor =========================

/// A curve that never quotes. Used as a competitor in "solo" mode so that all
/// retail flow reaches the strategy under test without changing the router.
pub fn null_compute_swap(_data: &[u8]) -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dodo::decimal_math::ONE;
    use prop_amm_shared::instruction::{encode_after_swap, encode_swap_instruction};

    const NANO: u64 = 1_000_000_000;

    fn wad(units: u128) -> U256 {
        ONE.checked_mul(U256::from_u128(units)).unwrap()
    }

    fn dodo_storage_blob(k: U256) -> Vec<u8> {
        dodo_initial_storage(wad(100), k, wad(100), wad(10_000), U256::ZERO)
    }

    #[test]
    fn storage_round_trips_through_the_blob() {
        let storage = dodo_storage_blob(ONE);
        let pool = dodo_pool_from_storage(&storage).unwrap();
        assert_eq!(pool.i, wad(100));
        assert_eq!(pool.k, ONE);
        assert_eq!(pool.target_base, wad(100));
        assert_eq!(pool.target_quote, wad(10_000));
        assert_eq!(pool.r_state, RState::One);
        assert_eq!(pool.lp_fee_rate, U256::ZERO);
    }

    #[test]
    fn oracle_publish_overwrites_only_the_first_field() {
        let mut storage = dodo_storage_blob(ONE);
        let before_k = read_u256(&storage, dodo_storage::K);
        let bytes = oracle_publish_bytes(wad(137));
        storage[..ORACLE_LEN].copy_from_slice(&bytes);
        assert_eq!(read_u256(&storage, dodo_storage::I), wad(137));
        assert_eq!(read_u256(&storage, dodo_storage::K), before_k);
    }

    #[test]
    fn dodo_quotes_both_sides_at_the_published_price() {
        let storage = dodo_storage_blob(ONE);
        let rx = 100 * NANO;
        let ry = 10_000 * NANO;

        // Sell 1 X: expect a little under 100 Y.
        let sell_x = encode_swap_instruction(1, NANO, rx, ry, &storage);
        let out_y = dodo_compute_swap(&sell_x);
        assert!(out_y > 99 * NANO && out_y < 100 * NANO, "got {out_y}");

        // Spend 100 Y: expect a little under 1 X.
        let buy_x = encode_swap_instruction(0, 100 * NANO, rx, ry, &storage);
        let out_x = dodo_compute_swap(&buy_x);
        assert!(out_x > NANO / 2 && out_x < NANO, "got {out_x}");
    }

    #[test]
    fn dodo_after_swap_persists_target_and_r_state_on_a_transition() {
        let mut storage = dodo_storage_blob(ONE);
        let rx = 100 * NANO;
        let ry = 10_000 * NANO;
        let input = NANO; // sell 1 X
        let output = dodo_compute_swap(&encode_swap_instruction(1, input, rx, ry, &storage));
        assert!(output > 0);

        let post_rx = rx + input;
        let post_ry = ry - output;
        let data = encode_after_swap(1, input, output, post_rx, post_ry, 0, &storage);
        dodo_after_swap(&data, &mut storage);

        let pool = dodo_pool_from_storage(&storage).unwrap();
        assert_eq!(pool.r_state, RState::BelowOne, "R must fall below one");
        assert_eq!(
            pool.target_base,
            wad(100),
            "sell-base leaves the base target"
        );
        assert_eq!(
            read_u256(&storage, dodo_storage::RESERVE_BASE_COMMITTED),
            nano_to_wad(post_rx)
        );
        assert_eq!(
            read_u256(&storage, dodo_storage::RESERVE_QUOTE_COMMITTED),
            nano_to_wad(post_ry)
        );
    }

    #[test]
    fn dodo_after_swap_leaves_state_alone_when_r_does_not_change() {
        let mut storage = dodo_storage_blob(ONE);
        // Move to BELOW_ONE first.
        let rx = 100 * NANO;
        let ry = 10_000 * NANO;
        let input = NANO;
        let output = dodo_compute_swap(&encode_swap_instruction(1, input, rx, ry, &storage));
        let data = encode_after_swap(1, input, output, rx + input, ry - output, 0, &storage);
        dodo_after_swap(&data, &mut storage);
        let after_first = dodo_pool_from_storage(&storage).unwrap();
        assert_eq!(after_first.r_state, RState::BelowOne);

        // Another sell-base keeps R below one, so no target may be written.
        let rx2 = rx + input;
        let ry2 = ry - output;
        let output2 = dodo_compute_swap(&encode_swap_instruction(1, input, rx2, ry2, &storage));
        let data2 = encode_after_swap(1, input, output2, rx2 + input, ry2 - output2, 1, &storage);
        dodo_after_swap(&data2, &mut storage);
        let after_second = dodo_pool_from_storage(&storage).unwrap();
        assert_eq!(after_second.r_state, RState::BelowOne);
        assert_eq!(after_second.target_base, after_first.target_base);
        assert_eq!(after_second.target_quote, after_first.target_quote);
    }

    #[test]
    fn flashbots_and_univ2_agree_with_their_ports_through_the_adapter() {
        let storage = flashbots_initial_storage(wad(100), ONE, U256::from_u64(1), wad(100));
        let rx = 100 * NANO;
        let ry = 10_000 * NANO;
        let data = encode_swap_instruction(1, NANO, rx, ry, &storage);
        let adapter_out = flashbots_compute_swap(&data);
        let direct = FlashbotsPool {
            concentration: U256::from_u64(1),
            mult_x: wad(100),
            mult_y: ONE,
            target_x: wad(100),
        }
        .quote_x_to_y(wad(100), ONE)
        .unwrap();
        assert_eq!(adapter_out, wad_to_nano(direct));

        let univ2_data = encode_swap_instruction(1, NANO, rx, ry, &[0u8; 0]);
        let univ2_out = univ2_compute_swap(&univ2_data);
        assert_eq!(
            univ2_out,
            wad_to_nano(univ2::get_amount_out(ONE, wad(100), wad(10_000)).unwrap())
        );
    }

    #[test]
    fn degenerate_instructions_return_zero() {
        let storage = dodo_storage_blob(ONE);
        assert_eq!(dodo_compute_swap(&[0u8; 10]), 0);
        assert_eq!(
            dodo_compute_swap(&encode_swap_instruction(
                1,
                0,
                100 * NANO,
                100 * NANO,
                &storage
            )),
            0
        );
        assert_eq!(
            dodo_compute_swap(&encode_swap_instruction(1, NANO, 0, 100 * NANO, &storage)),
            0
        );
        assert_eq!(
            dodo_compute_swap(&encode_swap_instruction(9, NANO, NANO, NANO, &storage)),
            0
        );
        assert_eq!(
            null_compute_swap(&encode_swap_instruction(1, NANO, NANO, NANO, &storage)),
            0
        );
    }
}
