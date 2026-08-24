//! Simulation adapter for the Uniswap V3 port.
//!
//! Exposes the V3 pool through the challenge's existing program interface
//! (`fn(&[u8]) -> u64` for a quote, `fn(&[u8], &mut [u8])` after a trade), the
//! same way [`crate::curves`] does for DODO, Flashbots and Uniswap V2.
//!
//! ## Why V3 owns its inventory
//!
//! The other three curves are functions of the reserves the simulation hands
//! them. V3 is not: its price and active liquidity cannot be recovered from
//! `(reserve_x, reserve_y)` once liquidity is concentrated. So the pool's state
//! (`sqrtPriceX96`, `tick`, `liquidity`) lives in the 1024-byte storage blob and
//! is advanced by `after_swap`, which is the only hook allowed to write.
//!
//! The consequence is stated plainly rather than hidden: the simulation's `f64`
//! ledger and the pool's exact integer state are two views of the same trades,
//! and they can drift by up to one nano per executed trade through the ledger's
//! quantisation — the same bounded drift the DODO adapter already carries, and
//! the same revert counter watches for it.
//!
//! ## Direction mapping
//!
//! `side = 0` spends Y to receive X, `side = 1` spends X to receive Y. X is
//! token0 and Y is token1, so `side = 1` is `zeroForOne = true`.
//!
//! ## Fill-or-kill
//!
//! Uniswap V3 is the only curve here that can consume *less* than the offered
//! input. The simulation's interface cannot express a partial fill, so this
//! adapter is **fill-or-kill**: an order the pool cannot take in full is
//! rejected whole, quoting zero. A rejection is not a revert and is never
//! counted as one.
//!
//! Rejections are not silent. Every one records how much the pool *could* have
//! taken, so the cost of the policy is reported rather than hidden — see
//! [`CapacityStats`].

use std::cell::{Cell, RefCell};

use prop_amm_shared::instruction::{decode_after_swap, decode_instruction, STORAGE_SIZE};

use crate::u256::U256;
use crate::univ3::{
    pool::{self, PoolConfig, PoolState},
    tick_math, TickList, I256,
};
use crate::wad::{nano_to_wad, wad_to_nano};

/// Who is asking for a quote. The adapter cannot tell on its own, so the
/// simulation loop tags the phase it is in before calling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caller {
    Retail,
    Arbitrage,
    /// Probes, static quote matrices and tests — anything that is not order
    /// flow and must not pollute the retail/arbitrage split.
    Other,
}

impl Caller {
    fn index(self) -> usize {
        match self {
            Caller::Retail => 0,
            Caller::Arbitrage => 1,
            Caller::Other => 2,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Caller::Retail => "retail",
            Caller::Arbitrage => "arbitrage",
            Caller::Other => "other",
        }
    }
}

/// Capacity accounting for one `(caller, direction)` bucket.
///
/// **These are quote-level counters, not order-level ones.** The router and the
/// arbitrageur evaluate many candidate sizes per order, and the adapter cannot
/// see order boundaries, so `reject_count` counts refused *quotes*. Order-level
/// figures are derived by the simulation loop, which does know when an order
/// begins and ends, from [`last_rejection`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CapacityBucket {
    pub reject_count: u64,
    /// Total input asked for across refused quotes (WAD, in the input token).
    pub requested_input: U256,
    /// Total input the pool could actually have absorbed on those quotes.
    pub fillable_input: U256,
    /// Total output the taker would have received on the fillable part. Kept so
    /// the report can say what the refused flow was worth, not merely that it
    /// was refused.
    pub fillable_output: U256,
}

impl CapacityBucket {
    /// `requested - fillable`: the part of the flow this policy turned away.
    pub fn canonical_unfilled_input(&self) -> U256 {
        self.requested_input
            .checked_sub(self.fillable_input)
            .unwrap_or(U256::ZERO)
    }
}

/// Capacity rejections, split by caller and by direction.
///
/// Index order is `[caller][side]` with `side = 0` meaning "spend Y to buy X"
/// and `side = 1` meaning "spend X to sell for Y".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CapacityStats {
    buckets: [[CapacityBucket; 2]; 3],
}

impl CapacityStats {
    pub fn bucket(&self, caller: Caller, side: u8) -> CapacityBucket {
        self.buckets[caller.index()][(side & 1) as usize]
    }

    pub fn total_reject_count(&self) -> u64 {
        self.buckets
            .iter()
            .flatten()
            .map(|bucket| bucket.reject_count)
            .sum()
    }

    fn record(&mut self, caller: Caller, side: u8, rejection: &Rejection) {
        let bucket = &mut self.buckets[caller.index()][(side & 1) as usize];
        bucket.reject_count += 1;
        bucket.requested_input = bucket
            .requested_input
            .wrapping_add(rejection.requested_input);
        bucket.fillable_input = bucket.fillable_input.wrapping_add(rejection.fillable_input);
        bucket.fillable_output = bucket
            .fillable_output
            .wrapping_add(rejection.fillable_output);
    }
}

/// The most recent capacity rejection: `(caller, side, requested, fillable)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rejection {
    pub caller: Caller,
    pub side: u8,
    pub requested_input: U256,
    pub fillable_input: U256,
    /// What the taker would have received for `fillable_input`.
    pub fillable_output: U256,
}

impl Rejection {
    pub fn canonical_unfilled_input(&self) -> U256 {
        self.requested_input
            .checked_sub(self.fillable_input)
            .unwrap_or(U256::ZERO)
    }
}

thread_local! {
    static CALLER: Cell<Caller> = const { Cell::new(Caller::Other) };
    static STATS: RefCell<CapacityStats> = const { RefCell::new(CapacityStats {
        buckets: [[CapacityBucket {
            reject_count: 0,
            requested_input: U256::ZERO,
            fillable_input: U256::ZERO,
            fillable_output: U256::ZERO,
        }; 2]; 3],
    }) };
    static LAST_REJECTION: Cell<Option<Rejection>> = const { Cell::new(None) };
}

/// Tag the phase the simulation is in, so rejections land in the right bucket.
pub fn set_caller(caller: Caller) {
    CALLER.with(|current| current.set(caller));
}

pub fn caller() -> Caller {
    CALLER.with(|current| current.get())
}

pub fn reset_capacity_stats() {
    STATS.with(|stats| *stats.borrow_mut() = CapacityStats::default());
    clear_last_rejection();
}

pub fn capacity_stats() -> CapacityStats {
    STATS.with(|stats| *stats.borrow())
}

/// The last capacity rejection seen.
///
/// **Never use this to decide whether an order was served.** It is sticky: a
/// subsequent successful quote does not clear it, so after the router refuses a
/// large candidate and then fills a smaller split, this still reports the
/// refusal. Deriving "the order went unfilled" from it over-counts every order
/// whose search happened to overshoot — see
/// `a_refused_large_quote_is_followed_by_a_smaller_one_that_fills`.
///
/// For order-level capacity, probe the pre-route state once per order with
/// [`probe_capacity`], which is deterministic and independent of the search.
pub fn last_rejection() -> Option<Rejection> {
    LAST_REJECTION.with(|last| last.get())
}

pub fn clear_last_rejection() {
    LAST_REJECTION.with(|last| last.set(None));
}

fn record_rejection(side: u8, requested: U256, fillable_input: U256, fillable_output: U256) {
    let caller = caller();
    let rejection = Rejection {
        caller,
        side,
        requested_input: requested,
        fillable_input,
        fillable_output,
    };
    STATS.with(|stats| stats.borrow_mut().record(caller, side, &rejection));
    LAST_REJECTION.with(|last| last.set(Some(rejection)));
}

/// Uniswap V3 storage layout inside the 1024-byte blob.
///
/// | Offset | Size | Field |
/// | --- | --- | --- |
/// | 0 | 32 | reserved for the oracle publish slot (V3 is passive and never reads it) |
/// | 32 | 32 | `sqrtPriceX96` |
/// | 64 | 4 | `tick` (`i32`) |
/// | 68 | 16 | `liquidity` (`u128`) |
/// | 84 | 4 | `tickSpacing` (`i32`) |
/// | 88 | 4 | `feePips` (`u32`) |
/// | 92 | 4 | initialized tick count `n` (`u32`) |
/// | 96 | 20·n | per tick: `tick` (`i32`) then `liquidityNet` (`i128`) |
pub mod univ3_storage {
    pub const SQRT_PRICE: usize = 32;
    pub const TICK: usize = 64;
    pub const LIQUIDITY: usize = 68;
    pub const TICK_SPACING: usize = 84;
    pub const FEE_PIPS: usize = 88;
    pub const TICK_COUNT: usize = 92;
    pub const TICKS: usize = 96;
    pub const BYTES_PER_TICK: usize = 20;
    /// Ticks that fit in the remaining space.
    pub const MAX_TICKS: usize = (super::STORAGE_SIZE - TICKS) / BYTES_PER_TICK;
    /// Bytes used by a pool with `n` initialized ticks.
    pub const fn used(tick_count: usize) -> usize {
        TICKS + tick_count * BYTES_PER_TICK
    }
}

fn read_u256(storage: &[u8], offset: usize) -> U256 {
    let end = (offset + 32).min(storage.len());
    if offset >= end {
        return U256::ZERO;
    }
    U256::from_le_bytes(&storage[offset..end])
}

fn write_u256(storage: &mut [u8], offset: usize, value: U256) {
    let bytes = value.to_le_bytes();
    let end = (offset + 32).min(storage.len());
    if offset < end {
        storage[offset..end].copy_from_slice(&bytes[..end - offset]);
    }
}

fn read_i32(storage: &[u8], offset: usize) -> i32 {
    let mut buf = [0u8; 4];
    buf.copy_from_slice(&storage[offset..offset + 4]);
    i32::from_le_bytes(buf)
}

fn write_i32(storage: &mut [u8], offset: usize, value: i32) {
    storage[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn read_u32(storage: &[u8], offset: usize) -> u32 {
    let mut buf = [0u8; 4];
    buf.copy_from_slice(&storage[offset..offset + 4]);
    u32::from_le_bytes(buf)
}

fn write_u32(storage: &mut [u8], offset: usize, value: u32) {
    storage[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn read_u128(storage: &[u8], offset: usize) -> u128 {
    let mut buf = [0u8; 16];
    buf.copy_from_slice(&storage[offset..offset + 16]);
    u128::from_le_bytes(buf)
}

fn write_u128(storage: &mut [u8], offset: usize, value: u128) {
    storage[offset..offset + 16].copy_from_slice(&value.to_le_bytes());
}

fn read_i128(storage: &[u8], offset: usize) -> i128 {
    let mut buf = [0u8; 16];
    buf.copy_from_slice(&storage[offset..offset + 16]);
    i128::from_le_bytes(buf)
}

fn write_i128(storage: &mut [u8], offset: usize, value: i128) {
    storage[offset..offset + 16].copy_from_slice(&value.to_le_bytes());
}

/// Serialise a pool into a fresh storage blob.
pub fn initial_storage(config: &PoolConfig, state: &PoolState) -> Vec<u8> {
    let ticks = config.ticks.as_slice();
    assert!(
        ticks.len() <= univ3_storage::MAX_TICKS,
        "a V3 pool with {} initialized ticks does not fit in {STORAGE_SIZE} bytes (max {})",
        ticks.len(),
        univ3_storage::MAX_TICKS
    );

    let mut storage = vec![0u8; STORAGE_SIZE];
    write_u256(
        &mut storage,
        univ3_storage::SQRT_PRICE,
        state.sqrt_price_x96,
    );
    write_i32(&mut storage, univ3_storage::TICK, state.tick);
    write_u128(&mut storage, univ3_storage::LIQUIDITY, state.liquidity);
    write_i32(
        &mut storage,
        univ3_storage::TICK_SPACING,
        config.tick_spacing,
    );
    write_u32(&mut storage, univ3_storage::FEE_PIPS, config.fee_pips);
    write_u32(&mut storage, univ3_storage::TICK_COUNT, ticks.len() as u32);
    for (index, entry) in ticks.iter().enumerate() {
        let base = univ3_storage::TICKS + index * univ3_storage::BYTES_PER_TICK;
        write_i32(&mut storage, base, entry.tick);
        write_i128(&mut storage, base + 4, entry.liquidity_net);
    }
    storage
}

fn decode_pool(storage: &[u8]) -> Option<(PoolConfig, PoolState)> {
    if storage.len() < univ3_storage::TICKS {
        return None;
    }
    let tick_count = read_u32(storage, univ3_storage::TICK_COUNT) as usize;
    if tick_count > univ3_storage::MAX_TICKS || storage.len() < univ3_storage::used(tick_count) {
        return None;
    }
    let tick_spacing = read_i32(storage, univ3_storage::TICK_SPACING);
    if tick_spacing <= 0 {
        return None;
    }

    let mut entries = Vec::with_capacity(tick_count);
    for index in 0..tick_count {
        let base = univ3_storage::TICKS + index * univ3_storage::BYTES_PER_TICK;
        entries.push((read_i32(storage, base), read_i128(storage, base + 4)));
    }

    Some((
        PoolConfig {
            tick_spacing,
            fee_pips: read_u32(storage, univ3_storage::FEE_PIPS),
            ticks: TickList::new(entries),
        },
        PoolState {
            sqrt_price_x96: read_u256(storage, univ3_storage::SQRT_PRICE),
            tick: read_i32(storage, univ3_storage::TICK),
            liquidity: read_u128(storage, univ3_storage::LIQUIDITY),
        },
    ))
}

fn write_state(storage: &mut [u8], state: &PoolState) {
    write_u256(storage, univ3_storage::SQRT_PRICE, state.sqrt_price_x96);
    write_i32(storage, univ3_storage::TICK, state.tick);
    write_u128(storage, univ3_storage::LIQUIDITY, state.liquidity);
}

/// What quoting the stored pool produced.
enum Quote {
    /// The pool took the whole input and paid out.
    Traded(pool::SwapOutcome),
    /// The pool cannot serve this trade in full: its price is already at the end
    /// of the tick domain in this direction, or its liquidity runs out before
    /// the whole input is consumed. Ordinary behaviour for a position whose
    /// range the price has left, **not** an error, so it is never counted as a
    /// revert.
    ///
    /// The outcome the pool *would* have produced is retained rather than
    /// discarded, so the adapter can report how much of the order was fillable
    /// instead of merely that it was refused.
    NoRoom {
        /// The partial swap the pool could have done, when it could do any.
        outcome: Option<pool::SwapOutcome>,
        /// How much of the requested input the pool could have absorbed.
        fillable_input: U256,
    },
    /// The pool would have reverted.
    Reverted,
}

/// Run one exact-input swap against the stored pool.
///
/// ## Why a partially-consumed swap is refused
///
/// Uniswap V3 is the only curve here that can consume *less* than the offered
/// input: once its liquidity is exhausted the remaining input simply cannot be
/// used, and the pool returns the portion it took. The simulation's interface
/// has no way to express that — `execute_sell_x(amount)` moves the whole
/// `amount` into the pool's reserve — so accepting such a quote would credit the
/// pool with input the curve never received and drive the ledger and the pool
/// state apart.
///
/// The adapter therefore only quotes when the pool consumes the entire input,
/// and reports [`Quote::NoRoom`] otherwise. That is an adapter policy about
/// which trades are offered, not a change to the curve: every quote it does
/// return is the pool's own arithmetic, unmodified.
fn quote(storage: &[u8], side: u8, input_wad: U256) -> Quote {
    let Some((config, state)) = decode_pool(storage) else {
        return Quote::Reverted;
    };
    let zero_for_one = match side {
        0 => false, // token1 in, token0 out
        1 => true,  // token0 in, token1 out
        _ => return Quote::Reverted,
    };
    if state.sqrt_price_x96 < tick_math::MIN_SQRT_RATIO
        || state.sqrt_price_x96 >= tick_math::MAX_SQRT_RATIO
    {
        return Quote::Reverted;
    }

    // `UniswapV3Pool.swap` requires the limit to be strictly on the far side of
    // the current price ('SPL'). When the price has already reached the end of
    // the domain in this direction there is simply nowhere left to move — that
    // is an exhausted position, not a failure.
    let limit = pool::no_limit(zero_for_one);
    let has_room = if zero_for_one {
        limit < state.sqrt_price_x96
    } else {
        limit > state.sqrt_price_x96
    };
    if !has_room {
        // Not even one wei can move: the price is parked at the domain edge.
        return Quote::NoRoom {
            outcome: None,
            fillable_input: U256::ZERO,
        };
    }

    let Some(amount) = I256::to_int256(input_wad) else {
        return Quote::Reverted;
    };
    match pool::swap(&config, state, zero_for_one, amount, limit) {
        Some(outcome) => {
            let consumed = if zero_for_one {
                outcome.amount0
            } else {
                outcome.amount1
            };
            if consumed == amount {
                Quote::Traded(outcome)
            } else {
                Quote::NoRoom {
                    fillable_input: consumed.magnitude(),
                    outcome: Some(outcome),
                }
            }
        }
        None => Quote::Reverted,
    }
}

fn output_of(outcome: &pool::SwapOutcome, side: u8) -> U256 {
    if side == 1 {
        outcome.amount1.magnitude()
    } else {
        outcome.amount0.magnitude()
    }
}

/// `compute_swap` for the Uniswap V3 curve.
pub fn univ3_compute_swap(data: &[u8]) -> u64 {
    if data.len() < 25 + univ3_storage::TICKS {
        return 0;
    }
    let (side, input_amount, reserve_x, reserve_y) = decode_instruction(data);
    if input_amount == 0 || reserve_x == 0 || reserve_y == 0 {
        return 0;
    }
    let requested = nano_to_wad(input_amount);
    match quote(&data[25..], side, requested) {
        Quote::Traded(outcome) => wad_to_nano(output_of(&outcome, side)),
        Quote::NoRoom {
            fillable_input,
            outcome,
        } => {
            let fillable_output = outcome
                .map(|outcome| output_of(&outcome, side))
                .unwrap_or(U256::ZERO);
            record_rejection(side, requested, fillable_input, fillable_output);
            0
        }
        Quote::Reverted => {
            crate::curves::record_revert();
            0
        }
    }
}

/// `after_swap` for the Uniswap V3 curve.
///
/// The stored state *is* the pre-trade state — nothing else writes it — so the
/// executed trade is replayed from it and the result committed. Post-trade
/// reserves from the simulation are deliberately not used to resynchronise: the
/// pool's integer state is authoritative for pricing, exactly as on chain.
pub fn univ3_after_swap(data: &[u8], storage: &mut [u8]) {
    if data.len() < 42 || storage.len() < univ3_storage::TICKS {
        return;
    }
    let (side, input_amount, _output, _rx, _ry, _step, _) = decode_after_swap(data);
    match quote(storage, side, nano_to_wad(input_amount)) {
        Quote::Traded(outcome) => write_state(storage, &outcome.state),
        // The simulation only executes a trade after a non-zero quote, so
        // neither of these should be reachable here. If one is, the quote and
        // the commit disagreed, which is exactly what the revert counter exists
        // to surface.
        Quote::NoRoom { .. } | Quote::Reverted => crate::curves::record_revert(),
    }
}

/// What the pool could do with an order, without executing anything.
///
/// `filled` is true when the whole input fits. Otherwise `fillable_input` and
/// `fillable_output` describe the partial swap the pool *could* have done, which
/// fill-or-kill declines to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapacityProbe {
    pub filled: bool,
    pub requested_input: U256,
    pub fillable_input: U256,
    pub fillable_output: U256,
    pub reverted: bool,
}

impl CapacityProbe {
    pub fn canonical_unfilled_input(&self) -> U256 {
        self.requested_input
            .checked_sub(self.fillable_input)
            .unwrap_or(U256::ZERO)
    }
}

/// Ask the pool what it could do with `input_nano`, recording nothing.
pub fn probe_capacity(storage: &[u8], side: u8, input_nano: u64) -> CapacityProbe {
    let requested = nano_to_wad(input_nano);
    match quote(storage, side, requested) {
        Quote::Traded(outcome) => CapacityProbe {
            filled: true,
            requested_input: requested,
            fillable_input: requested,
            fillable_output: output_of(&outcome, side),
            reverted: false,
        },
        Quote::NoRoom {
            fillable_input,
            outcome,
        } => CapacityProbe {
            filled: false,
            requested_input: requested,
            fillable_input,
            fillable_output: outcome
                .map(|outcome| output_of(&outcome, side))
                .unwrap_or(U256::ZERO),
            reverted: false,
        },
        Quote::Reverted => CapacityProbe {
            filled: false,
            requested_input: requested,
            fillable_input: U256::ZERO,
            fillable_output: U256::ZERO,
            reverted: true,
        },
    }
}

/// Whether the externally published fair price lies inside the pool's position
/// range.
///
/// This replaces any notion of "the pool state is out of range". A static LP is
/// economically out of range when the *market* has moved past its bounds, and
/// under fill-or-kill the pool's own tick can sit pinned at a boundary without
/// ever formally crossing it — so the pool state is the wrong thing to count.
/// The simulation counts `fairPriceOutOfRangeSteps` from this instead.
pub fn fair_price_in_range(storage: &[u8], fair_price_wad: U256) -> Option<bool> {
    let (config, _) = decode_pool(storage)?;
    let ticks = config.ticks.as_slice();
    if ticks.is_empty() {
        return Some(false);
    }
    let lower = ticks.first()?.tick;
    let upper = ticks.last()?.tick;
    let lower_price = tick_math::get_sqrt_ratio_at_tick(lower)?;
    let upper_price = tick_math::get_sqrt_ratio_at_tick(upper)?;
    // Compare in sqrt-price space so no square root is needed: the fair price is
    // a WAD ratio, and sqrt(P) * 2^96 is monotone in P.
    let fair_sqrt = fair_price_sqrt_x96(fair_price_wad)?;
    Some(fair_sqrt >= lower_price && fair_sqrt < upper_price)
}

/// `floor(sqrt(priceWad / 1e18) * 2^96)`, computed on integers.
///
/// `sqrt(price * 2^192 / 1e18)` keeps everything in one integer square root.
fn fair_price_sqrt_x96(fair_price_wad: U256) -> Option<U256> {
    let shifted =
        crate::u256::mul_div(fair_price_wad, U256::ONE.checked_shl(192)?, crate::wad::WAD)?;
    Some(crate::dodo::dodo_math::sqrt(shifted))
}

/// Active-liquidity diagnostics for the report, read straight from storage.
pub fn active_liquidity(storage: &[u8]) -> Option<u128> {
    decode_pool(storage).map(|(_, state)| state.liquidity)
}

/// The pool's current tick, for out-of-range accounting.
pub fn current_tick(storage: &[u8]) -> Option<i32> {
    decode_pool(storage).map(|(_, state)| state.tick)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::univ3;
    use prop_amm_shared::instruction::{encode_after_swap, encode_swap_instruction};

    const NANO: u64 = 1_000_000_000;

    fn full_range_storage() -> Vec<u8> {
        let (config, state) = univ3::full_range_pool(univ3::FULL_RANGE_LIQUIDITY);
        initial_storage(&config, &state)
    }

    /// The exact scenario the order-level metric must survive: a big candidate
    /// quote is refused for want of room, and a smaller split of the same order
    /// then fills. `last_rejection` is **sticky** across that pair, which is why
    /// it must never be used to decide whether an order was served.
    ///
    /// See `experiment::run_single_traced`, which probes the pre-route state once
    /// per order instead of reading this flag.
    #[test]
    fn a_refused_large_quote_is_followed_by_a_smaller_one_that_fills() {
        // A narrow position, so the pool runs out of room well before the size
        // ladder does.
        let (config, state) = univ3::concentrated_pool(20, univ3::FULL_RANGE_LIQUIDITY);
        let storage = initial_storage(&config, &state);
        let (rx, ry) = (100 * NANO, 10_000 * NANO);

        // Find a size the pool cannot take in full, and one it can.
        let big = 50 * NANO;
        let small = NANO / 1_000;
        let big_probe = probe_capacity(&storage, 1, big);
        let small_probe = probe_capacity(&storage, 1, small);
        assert!(
            !big_probe.filled,
            "the 20-tick position should not absorb 50 X in full"
        );
        assert!(
            small_probe.filled,
            "the same position should still absorb a dust-sized order"
        );

        reset_capacity_stats();
        set_caller(Caller::Retail);

        // The router's search would try the big candidate first...
        let refused = univ3_compute_swap(&encode_swap_instruction(1, big, rx, ry, &storage));
        assert_eq!(refused, 0, "a refused quote returns zero");
        assert!(
            last_rejection().is_some(),
            "the refusal must be recorded for diagnostics"
        );

        // ...then bisect down to one that fits. The order IS served.
        let filled = univ3_compute_swap(&encode_swap_instruction(1, small, rx, ry, &storage));
        assert!(filled > 0, "the smaller split must quote a real amount");

        // And yet the flag is still set. Anything reading it here would conclude
        // the order went unfilled, which is exactly the bug.
        assert!(
            last_rejection().is_some(),
            "last_rejection is sticky: it is NOT cleared by a subsequent success"
        );

        // The quote-level counter meanwhile shows two calls, one refusal — a
        // count of refused CANDIDATES, not of orders.
        let stats = capacity_stats();
        assert_eq!(stats.total_reject_count(), 1);
        assert_eq!(stats.bucket(Caller::Retail, 1).reject_count, 1);
        set_caller(Caller::Other);
    }

    #[test]
    fn storage_round_trips_the_pool() {
        let (config, state) = univ3::full_range_pool(univ3::FULL_RANGE_LIQUIDITY);
        let storage = initial_storage(&config, &state);
        let (decoded_config, decoded_state) = decode_pool(&storage).unwrap();
        assert_eq!(decoded_config, config);
        assert_eq!(decoded_state, state);
    }

    #[test]
    fn the_oracle_slot_is_untouched_by_the_layout() {
        let storage = full_range_storage();
        assert!(
            storage[..32].iter().all(|byte| *byte == 0),
            "bytes 0..32 are reserved for the oracle publish path"
        );
        // Overwriting them must not disturb the pool.
        let mut published = storage.clone();
        published[..32].copy_from_slice(&U256::from_u64(12345).to_le_bytes());
        assert_eq!(
            decode_pool(&published).unwrap(),
            decode_pool(&storage).unwrap()
        );
    }

    #[test]
    fn a_concentrated_pool_fits_and_round_trips() {
        let (config, state) = univ3::concentrated_pool(500, univ3::FULL_RANGE_LIQUIDITY);
        let storage = initial_storage(&config, &state);
        assert_eq!(
            univ3_storage::used(config.ticks.len()),
            univ3_storage::TICKS + 2 * univ3_storage::BYTES_PER_TICK
        );
        assert_eq!(decode_pool(&storage).unwrap(), (config, state));
    }

    #[test]
    fn quotes_both_directions_at_the_opening_state() {
        let storage = full_range_storage();
        let rx = 100 * NANO;
        let ry = 10_000 * NANO;

        // Sell 1 X, expect just under 100 Y.
        let out_y = univ3_compute_swap(&encode_swap_instruction(1, NANO, rx, ry, &storage));
        assert!(out_y > 99 * NANO && out_y < 100 * NANO, "got {out_y}");

        // Spend 100 Y, expect just under 1 X.
        let out_x = univ3_compute_swap(&encode_swap_instruction(0, 100 * NANO, rx, ry, &storage));
        assert!(out_x > NANO / 2 && out_x < NANO, "got {out_x}");
    }

    #[test]
    fn after_swap_advances_the_price_and_leaves_liquidity_alone() {
        let mut storage = full_range_storage();
        let rx = 100 * NANO;
        let ry = 10_000 * NANO;
        let before = decode_pool(&storage).unwrap().1;

        let input = NANO;
        let output = univ3_compute_swap(&encode_swap_instruction(1, input, rx, ry, &storage));
        assert!(output > 0);
        let data = encode_after_swap(1, input, output, rx + input, ry - output, 0, &storage);
        univ3_after_swap(&data, &mut storage);

        let after = decode_pool(&storage).unwrap().1;
        assert!(
            after.sqrt_price_x96 < before.sqrt_price_x96,
            "selling X lowers the price"
        );
        assert!(after.tick <= before.tick);
        assert_eq!(
            after.liquidity, before.liquidity,
            "full range never changes liquidity"
        );
    }

    #[test]
    fn repeated_swaps_move_the_price_monotonically() {
        let mut storage = full_range_storage();
        let mut rx = 100 * NANO;
        let mut ry = 10_000 * NANO;
        let mut previous = decode_pool(&storage).unwrap().1.sqrt_price_x96;

        for step in 0..25u64 {
            let input = NANO / 10;
            let output = univ3_compute_swap(&encode_swap_instruction(1, input, rx, ry, &storage));
            assert!(output > 0, "step {step} produced no output");
            let data = encode_after_swap(1, input, output, rx + input, ry - output, step, &storage);
            univ3_after_swap(&data, &mut storage);
            rx += input;
            ry -= output;
            let now = decode_pool(&storage).unwrap().1.sqrt_price_x96;
            assert!(
                now < previous,
                "price must fall on every sell at step {step}"
            );
            previous = now;
        }
    }

    #[test]
    fn a_concentrated_pool_pins_at_its_boundary_and_refuses_what_it_cannot_fill() {
        // Push a narrow pool toward the bottom of its range with fills it can
        // fully serve, then check what it does with an order it cannot.
        let (config, state) = univ3::concentrated_pool(50, univ3::FULL_RANGE_LIQUIDITY);
        let lower = config.ticks.as_slice()[0].tick;
        let mut storage = initial_storage(&config, &state);
        let mut rx = 100 * NANO;
        let mut ry = 10_000 * NANO;

        crate::curves::reset_revert_count();
        let mut fills = 0u32;
        let mut refusals = 0u32;
        for step in 0..80u64 {
            let input = NANO / 4;
            let output = univ3_compute_swap(&encode_swap_instruction(1, input, rx, ry, &storage));
            if output == 0 {
                refusals += 1;
                continue;
            }
            let data = encode_after_swap(1, input, output, rx + input, ry - output, step, &storage);
            univ3_after_swap(&data, &mut storage);
            rx += input;
            ry -= output;
            fills += 1;
        }

        assert!(fills > 0, "the pool should serve orders that fit");
        assert!(
            refusals > 0,
            "the pool should eventually refuse orders larger than its remaining range"
        );

        let (_, final_state) = decode_pool(&storage).unwrap();
        // The price is pinned just above the lower boundary. The position has
        // NOT formally exited: crossing requires an order sized exactly to the
        // remaining range, and any larger order is refused because the adapter
        // cannot express a partial fill (see `quote`).
        assert!(
            final_state.tick >= lower,
            "price {} should be pinned at or above the boundary {lower}",
            final_state.tick
        );
        assert_eq!(
            final_state.liquidity,
            univ3::FULL_RANGE_LIQUIDITY,
            "liquidity is still active because the boundary was never crossed"
        );

        // Refusals must never be counted as reverts.
        assert_eq!(
            crate::curves::revert_count(),
            0,
            "refusing an order that does not fit is normal behaviour, not a revert"
        );
    }

    #[test]
    fn an_exhausted_pool_quotes_zero_without_recording_a_revert() {
        // Hand-build the state a fully drained position would leave: liquidity
        // gone, price parked at the very bottom of the tick domain.
        let (config, _) = univ3::concentrated_pool(50, univ3::FULL_RANGE_LIQUIDITY);
        let exhausted = PoolState {
            sqrt_price_x96: pool::no_limit(true),
            tick: tick_math::MIN_TICK,
            liquidity: 0,
        };
        let storage = initial_storage(&config, &exhausted);

        crate::curves::reset_revert_count();
        // Selling further has nowhere to go: the price is already at the end of
        // the domain, which is upstream's 'SPL'. Here it is a zero quote.
        let down = univ3_compute_swap(&encode_swap_instruction(
            1,
            NANO,
            100 * NANO,
            NANO,
            &storage,
        ));
        assert_eq!(down, 0);

        // Buying back is different, and the difference is real V3 behaviour
        // rather than an artefact: the swap walks the price up through the empty
        // tick domain, re-enters the position's range, picks its liquidity back
        // up and trades. A static out-of-range position is not dead, it is
        // one-sided.
        let up = univ3_compute_swap(&encode_swap_instruction(
            0,
            10 * NANO,
            100 * NANO,
            10_000 * NANO,
            &storage,
        ));
        assert!(up > 0, "buying back into the range must be servable");

        // A buy larger than the whole range can absorb is refused, for the same
        // reason as any other partial fill.
        let too_big = univ3_compute_swap(&encode_swap_instruction(
            0,
            10_000 * NANO,
            100 * NANO,
            10_000 * NANO,
            &storage,
        ));
        assert_eq!(
            too_big, 0,
            "an order beyond the range's capacity is refused"
        );

        assert_eq!(
            crate::curves::revert_count(),
            0,
            "an exhausted position is a state, not an error"
        );
    }

    #[test]
    fn capacity_rejections_are_recorded_and_split_by_caller_and_direction() {
        // A narrow pool refuses an order it cannot fill in full, and the
        // rejection carries what it could have done.
        let (config, state) = univ3::concentrated_pool(50, univ3::FULL_RANGE_LIQUIDITY);
        let storage = initial_storage(&config, &state);
        let rx = 100 * NANO;
        let ry = 10_000 * NANO;

        reset_capacity_stats();
        crate::curves::reset_revert_count();

        set_caller(Caller::Retail);
        let huge = 500 * NANO; // far more token0 than the range can absorb
        assert_eq!(
            univ3_compute_swap(&encode_swap_instruction(1, huge, rx, ry, &storage)),
            0,
            "an unfillable order is refused whole"
        );

        set_caller(Caller::Arbitrage);
        let huge_y = 50_000 * NANO;
        assert_eq!(
            univ3_compute_swap(&encode_swap_instruction(0, huge_y, rx, ry, &storage)),
            0
        );

        let stats = capacity_stats();
        assert_eq!(stats.total_reject_count(), 2);

        let retail_sell = stats.bucket(Caller::Retail, 1);
        assert_eq!(retail_sell.reject_count, 1);
        assert_eq!(retail_sell.requested_input, nano_to_wad(huge));
        assert!(
            retail_sell.fillable_input > U256::ZERO,
            "the pool could have taken part of it"
        );
        assert!(retail_sell.fillable_input < retail_sell.requested_input);
        assert!(
            retail_sell.fillable_output > U256::ZERO,
            "and would have paid something out for it"
        );
        assert_eq!(
            retail_sell.canonical_unfilled_input(),
            retail_sell
                .requested_input
                .checked_sub(retail_sell.fillable_input)
                .unwrap()
        );

        let arb_buy = stats.bucket(Caller::Arbitrage, 0);
        assert_eq!(arb_buy.reject_count, 1);
        assert_eq!(arb_buy.requested_input, nano_to_wad(huge_y));

        // The buckets must not bleed into one another.
        assert_eq!(stats.bucket(Caller::Retail, 0).reject_count, 0);
        assert_eq!(stats.bucket(Caller::Arbitrage, 1).reject_count, 0);
        assert_eq!(stats.bucket(Caller::Other, 0).reject_count, 0);

        // A capacity rejection is never a revert.
        assert_eq!(crate::curves::revert_count(), 0);

        let last = last_rejection().expect("a rejection was recorded");
        assert_eq!(last.caller, Caller::Arbitrage);
        assert_eq!(last.side, 0);
        set_caller(Caller::Other);
    }

    #[test]
    fn a_filled_order_records_no_rejection() {
        let storage = full_range_storage();
        reset_capacity_stats();
        set_caller(Caller::Retail);
        let out = univ3_compute_swap(&encode_swap_instruction(
            1,
            NANO,
            100 * NANO,
            10_000 * NANO,
            &storage,
        ));
        assert!(out > 0);
        assert_eq!(capacity_stats().total_reject_count(), 0);
        assert!(last_rejection().is_none());
        set_caller(Caller::Other);
    }

    #[test]
    fn the_capacity_probe_reports_what_the_pool_could_take() {
        let (config, state) = univ3::concentrated_pool(50, univ3::FULL_RANGE_LIQUIDITY);
        let storage = initial_storage(&config, &state);

        // Something small fits entirely.
        let small = probe_capacity(&storage, 1, NANO / 100);
        assert!(small.filled);
        assert_eq!(small.fillable_input, small.requested_input);
        assert_eq!(small.canonical_unfilled_input(), U256::ZERO);
        assert!(small.fillable_output > U256::ZERO);

        // Something huge does not, and the probe says how much would have.
        let huge = probe_capacity(&storage, 1, 500 * NANO);
        assert!(!huge.filled);
        assert!(!huge.reverted);
        assert!(huge.fillable_input > U256::ZERO);
        assert!(huge.canonical_unfilled_input() > U256::ZERO);
        assert_eq!(
            huge.fillable_input
                .checked_add(huge.canonical_unfilled_input())
                .unwrap(),
            huge.requested_input
        );

        // Probing must not touch the counters.
        reset_capacity_stats();
        let _ = probe_capacity(&storage, 1, 500 * NANO);
        assert_eq!(capacity_stats().total_reject_count(), 0);
    }

    #[test]
    fn fair_price_range_membership_tracks_the_position_bounds() {
        let (config, state) = univ3::concentrated_pool(500, univ3::FULL_RANGE_LIQUIDITY);
        let storage = initial_storage(&config, &state);
        let at_open = crate::wad::price_to_wad(100.0).unwrap();
        assert_eq!(fair_price_in_range(&storage, at_open), Some(true));

        // 500 ticks is about +/-5%, so 100 stays inside while 120 and 80 do not.
        assert_eq!(
            fair_price_in_range(&storage, crate::wad::price_to_wad(120.0).unwrap()),
            Some(false),
            "a fair price above the upper bound is out of range"
        );
        assert_eq!(
            fair_price_in_range(&storage, crate::wad::price_to_wad(80.0).unwrap()),
            Some(false),
            "and so is one below the lower bound"
        );

        // The full-range pool is never out of range.
        let full = full_range_storage();
        for price in [1.0, 100.0, 10_000.0] {
            assert_eq!(
                fair_price_in_range(&full, crate::wad::price_to_wad(price).unwrap()),
                Some(true),
                "full range must contain {price}"
            );
        }
    }

    #[test]
    fn degenerate_instructions_return_zero() {
        let storage = full_range_storage();
        assert_eq!(univ3_compute_swap(&[0u8; 10]), 0);
        assert_eq!(
            univ3_compute_swap(&encode_swap_instruction(1, 0, NANO, NANO, &storage)),
            0
        );
        assert_eq!(
            univ3_compute_swap(&encode_swap_instruction(9, NANO, NANO, NANO, &storage)),
            0
        );
    }
}
