//! The initialized-tick set, and `TickBitmap.nextInitializedTickWithinOneWord`.
//!
//! Upstream: Uniswap/v3-core `contracts/libraries/TickBitmap.sol`
//! Commit:   e3589b192d0be27e100cd0daaf6c97204fdb1899 (tag `v1.0.0`)
//! sha256:   bd7a17c5134f0718eb7d856ddfc58d8347d32a8f661bed53aa3ad17c9aea09ba
//!
//! ## Why a sorted array instead of a packed bitmap
//!
//! Upstream stores initialized ticks as a `mapping(int16 => uint256)` of packed
//! bits. This port keeps the same *semantics* but a different *lookup
//! structure*: a sorted array of `(tick, liquidityNet)`.
//!
//! The substitution is safe because the word that upstream reads is, by
//! definition, "the set of initialized compressed ticks sharing this `wordPos`".
//! `nextInitializedTickWithinOneWord` then returns the nearest initialized tick
//! **within that word**, or the word's edge if there is none. Searching the
//! sorted array for the nearest initialized tick with a matching `wordPos`
//! answers exactly that question, so both the returned tick and the
//! `initialized` flag are identical.
//!
//! What is deliberately *not* simplified is the one-word limit itself. It is
//! tempting to jump straight to the next initialized tick, but upstream stops at
//! a word boundary and runs another loop iteration from there. That extra
//! iteration splits one `computeSwapStep` into two, and the two roundings do not
//! always sum to the same wei. The word-boundary behaviour is therefore
//! reproduced exactly, and the golden vectors — which come from executing the
//! real pool — are what prove the substitution correct.

/// One initialized tick and the liquidity that crossing it adds (moving up).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickInfo {
    pub tick: i32,
    pub liquidity_net: i128,
}

/// A sorted, deduplicated set of initialized ticks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TickList {
    ticks: Vec<TickInfo>,
}

impl TickList {
    /// Build from `(tick, liquidityNet)` pairs. Entries are sorted and pairs
    /// with the same tick are summed, matching how minting several positions
    /// accumulates `liquidityNet` on a shared boundary.
    pub fn new(entries: impl IntoIterator<Item = (i32, i128)>) -> TickList {
        let mut ticks: Vec<TickInfo> = Vec::new();
        for (tick, liquidity_net) in entries {
            match ticks.iter_mut().find(|entry| entry.tick == tick) {
                Some(entry) => entry.liquidity_net += liquidity_net,
                None => ticks.push(TickInfo {
                    tick,
                    liquidity_net,
                }),
            }
        }
        ticks.retain(|entry| entry.liquidity_net != 0);
        ticks.sort_by_key(|entry| entry.tick);
        TickList { ticks }
    }

    pub fn as_slice(&self) -> &[TickInfo] {
        &self.ticks
    }

    pub fn len(&self) -> usize {
        self.ticks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ticks.is_empty()
    }

    /// `liquidityNet` of an initialized tick, or `None` if it is not initialized.
    pub fn liquidity_net(&self, tick: i32) -> Option<i128> {
        self.ticks
            .iter()
            .find(|entry| entry.tick == tick)
            .map(|entry| entry.liquidity_net)
    }

    /// True when any initialized tick remains strictly beyond `tick` in the
    /// direction of travel. Used only to decide whether a run of empty steps can
    /// be short-circuited; it never changes an amount.
    pub fn has_initialized_beyond(&self, tick: i32, zero_for_one: bool) -> bool {
        if zero_for_one {
            self.ticks.iter().any(|entry| entry.tick <= tick)
        } else {
            self.ticks.iter().any(|entry| entry.tick > tick)
        }
    }

    /// `TickBitmap.nextInitializedTickWithinOneWord(tick, tickSpacing, lte)`
    pub fn next_initialized_tick_within_one_word(
        &self,
        tick: i32,
        tick_spacing: i32,
        lte: bool,
    ) -> (i32, bool) {
        // int24 compressed = tick / tickSpacing;
        // if (tick < 0 && tick % tickSpacing != 0) compressed--;
        let mut compressed = tick / tick_spacing;
        if tick < 0 && tick % tick_spacing != 0 {
            compressed -= 1;
        }

        if lte {
            let (word_pos, bit_pos) = position(compressed);
            // The mask keeps every bit at or to the right of bitPos, i.e. every
            // compressed tick in this word that is <= compressed.
            let best = self
                .compressed_ticks(tick_spacing)
                .filter(|c| *c <= compressed && position(*c).0 == word_pos)
                .max();
            match best {
                Some(next) => (next * tick_spacing, true),
                None => ((compressed - bit_pos as i32) * tick_spacing, false),
            }
        } else {
            // start from the word of the next tick, since the current tick state doesn't matter
            let (word_pos, bit_pos) = position(compressed + 1);
            // `compressed + 1` is upstream's search origin in this branch, the same
            // value `position()` was just given. Rewriting it as `*c > compressed`
            // would be equivalent but would stop matching the pinned source.
            #[allow(clippy::int_plus_one)]
            let best = self
                .compressed_ticks(tick_spacing)
                .filter(|c| *c >= compressed + 1 && position(*c).0 == word_pos)
                .min();
            match best {
                Some(next) => (next * tick_spacing, true),
                None => (
                    (compressed + 1 + (u8::MAX as i32 - bit_pos as i32)) * tick_spacing,
                    false,
                ),
            }
        }
    }

    fn compressed_ticks<'a>(&'a self, tick_spacing: i32) -> impl Iterator<Item = i32> + 'a {
        self.ticks
            .iter()
            .map(move |entry| entry.tick / tick_spacing)
    }
}

/// `TickBitmap.position(int24 tick)`.
///
/// `wordPos = int16(tick >> 8)` is an arithmetic shift, and
/// `bitPos = uint8(tick % 256)` truncates a possibly-negative remainder to eight
/// bits — which for two's complement is the same as taking the low byte.
pub fn position(tick: i32) -> (i16, u8) {
    let word_pos = (tick >> 8) as i16;
    let bit_pos = (tick & 0xff) as u8;
    (word_pos, bit_pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(entries: &[(i32, i128)]) -> TickList {
        TickList::new(entries.iter().copied())
    }

    #[test]
    fn position_decomposes_negative_ticks_toward_negative_infinity() {
        assert_eq!(position(0), (0, 0));
        assert_eq!(position(255), (0, 255));
        assert_eq!(position(256), (1, 0));
        // -1 lives in word -1 at bit 255, not word 0.
        assert_eq!(position(-1), (-1, 255));
        assert_eq!(position(-256), (-1, 0));
        assert_eq!(position(-257), (-2, 255));
    }

    #[test]
    fn entries_are_sorted_summed_and_pruned() {
        let list = list(&[(10, 5), (-3, 7), (10, -5), (4, 2)]);
        // tick 10 nets to zero and disappears.
        assert_eq!(
            list.as_slice(),
            &[
                TickInfo {
                    tick: -3,
                    liquidity_net: 7
                },
                TickInfo {
                    tick: 4,
                    liquidity_net: 2
                },
            ]
        );
        assert_eq!(list.liquidity_net(4), Some(2));
        assert_eq!(list.liquidity_net(10), None);
    }

    #[test]
    fn finds_the_nearest_initialized_tick_in_the_same_word() {
        let list = list(&[(0, 1), (100, 1), (200, -1)]);
        assert_eq!(
            list.next_initialized_tick_within_one_word(150, 1, true),
            (100, true)
        );
        assert_eq!(
            list.next_initialized_tick_within_one_word(150, 1, false),
            (200, true)
        );
        // Starting exactly on an initialized tick: lte includes it, gt does not.
        assert_eq!(
            list.next_initialized_tick_within_one_word(100, 1, true),
            (100, true)
        );
        assert_eq!(
            list.next_initialized_tick_within_one_word(100, 1, false),
            (200, true)
        );
    }

    #[test]
    fn stops_at_the_word_edge_when_nothing_is_initialized_in_range() {
        // Only tick 0 is initialized; from tick 700 (word 2) searching left must
        // return the word edge 512, NOT tick 0, and report uninitialized.
        let list = list(&[(0, 1)]);
        let (next, initialized) = list.next_initialized_tick_within_one_word(700, 1, true);
        assert_eq!((next, initialized), (512, false));
        // Searching right from 700 with nothing above: edge of word 2 is 767.
        let (next, initialized) = list.next_initialized_tick_within_one_word(700, 1, false);
        assert_eq!((next, initialized), (767, false));
    }

    #[test]
    fn word_edges_are_respected_for_negative_ticks() {
        let list = list(&[(-1000, 1)]);
        // tick -1 is in word -1, whose ticks run -256..=-1.
        let (next, initialized) = list.next_initialized_tick_within_one_word(-1, 1, true);
        assert_eq!((next, initialized), (-256, false));
        // From -300 (word -2, ticks -512..=-257) searching left finds nothing.
        let (next, initialized) = list.next_initialized_tick_within_one_word(-300, 1, true);
        assert_eq!((next, initialized), (-512, false));
    }

    #[test]
    fn tick_spacing_compresses_the_search() {
        let list = list(&[(600, 1), (1200, -1)]);
        // With spacing 60 the compressed ticks are 10 and 20, both in word 0.
        assert_eq!(
            list.next_initialized_tick_within_one_word(900, 60, true),
            (600, true)
        );
        assert_eq!(
            list.next_initialized_tick_within_one_word(900, 60, false),
            (1200, true)
        );
    }

    #[test]
    fn full_range_bounds_land_in_different_words() {
        let list = list(&[(-887_272, 1), (887_272, -1)]);
        // From tick 46054 there is nothing initialized within one word either way.
        let (_, initialized) = list.next_initialized_tick_within_one_word(46_054, 1, true);
        assert!(!initialized);
        let (_, initialized) = list.next_initialized_tick_within_one_word(46_054, 1, false);
        assert!(!initialized);
        assert!(list.has_initialized_beyond(46_054, true));
        assert!(list.has_initialized_beyond(46_054, false));
    }
}
