use pinocchio::{account_info::AccountInfo, entrypoint, pubkey::Pubkey, ProgramResult};
use prop_amm_submission_sdk::{set_return_data_bytes, set_return_data_u64};

const NAME: &str = "003b Wider Band x Deeper Book";
// The parameter space widens (W_BPS) and gains resolution (DELTA_RESERVE_BPS replaces
// DELTA_PCT), but the mechanism itself — ladder shape, re-anchor policy, inversion method —
// is untouched from the parent (NOTES.md § Fidelity self-assessment), so this still names
// the model that authored the parent port, per the same convention `004b-floor-subtracted-
// ewma-fee` used for its own single-mechanism-change variant.
const MODEL_USED: &str = "Claude Sonnet 5";

// ============================================================================
// 003b Wider Band x Deeper Book — a docs/DESIGN.md §2.9 variant of
// `003-piecewise-linear` (WHI-1224), ported from `benedictbrady/prop-amm`'s on-chain
// program (docs/references/003-piecewise-linear/): a 7-price-point / 6-segment
// piecewise-linear liquidity ladder per side, uniform liquidity density per segment
// (source's own `calculate_k`), self-replenishing liquidity.
//
// WHAT CHANGED FROM THE PARENT (NOTES.md § Objective): the parent's `W_BPS` search
// converged to its own frozen upper bound (1000), on a still-rising plateau — genuine
// unresolved headroom, not a search artifact. This variant (a) widens `W_BPS`'s own upper
// bound to 2500 (the ridge-collapse argument: past that, further widening is pure
// reparameterization, not more expressiveness) and (b) replaces `DELTA_PCT` (percent
// resolution, `rx*DELTA_PCT/100`) with `DELTA_RESERVE_BPS` (bps resolution,
// `rx*DELTA_RESERVE_BPS/10_000`) — 20x finer, needed to resolve the parent's own
// ~3.25%-interior peak. `S0_BPS` is unchanged; no candidate change touches the near-spot
// price axis. Pre-registered probes (NOTES.md § Pre-registered probes) confirmed the width
// extension ALONE is a net loss (thinner book at fixed depth) and only the width x depth
// interaction pays — this variant's single falsifiable thesis.
//
// RE-ANCHOR POLICY (the parent's own decision, per the issue's review amendment — "the
// decision this issue must demand is the run-long re-anchor policy"): the ladder is
// recomputed fresh, EVERY quote, directly from the LIVE (reserve_x, reserve_y) passed into
// `compute_swap` — never from storage. This is not merely "a" valid re-anchor policy, it is
// mathematically IDENTICAL to the amendment's own literal recommendation ("recompute the
// points from post-trade reserves in every after_swap, reset consumed = 0"): reserves only
// ever change via an EXECUTED trade, and an executed trade is exactly what fires
// `after_swap` — so "prices as of the last after_swap" and "prices derived from the
// currently-live reserves" are the SAME value at every instant a later `compute_swap` could
// observe them. Deriving fresh removes the storage plumbing entirely with no behavioral
// difference. See the parent's own NOTES.md § Re-anchor policy for the full argument,
// including why this makes the source's "heal consumed" step dead code (this port's
// `after_swap` is a no-op) and why `total_quantity` is NOT "left untouched" as the
// amendment's literal wording suggested (a shape-mandated deviation — see the parent's own
// NOTES.md).
//
// SHAPE SAFETY — the curve depends only on (reserve_x, reserve_y) and compile-time
// constants, NEVER on `input_amount` or storage, so each side's quote is a pure function of
// a fixed-state piecewise-linear ladder: marginal price is non-decreasing by construction
// (ascending ladder, uniform density), so output-vs-input is monotone and concave for any
// single quote curve.
//
// COLD START / GARBAGE STATE — trivial by construction: storage is never read, so the very
// first quote of a simulation (zero storage) and a quote under `validate.rs`'s
// randomized-storage probe behave identically to every other quote at the same reserves.
//
// EXACT INVERSION — the sell side (side=1, input is base X) is a direct closed-form
// evaluation (no inversion needed: base consumed IS the input). The buy side (side=0, input
// is quote Y) requires inverting a quadratic cost-in-base function; per the issue's review
// amendment this is done by integer bisection over the exact forward cost function, not by
// `isqrt` (whose floor error, amplified by a large `k/S` coefficient, can exceed the
// checker's 4-nano tolerance by an order of magnitude — see NOTES.md § Do not invert with
// sqrt). An overflow short-circuit (compute the whole-book cost first; if `input` already
// exceeds it, return the exhaustion output directly) means the huge `input` values the
// arbitrageur's bracket search probes (up to `MAX_INPUT_AMOUNT` ~= 1.8e19 nano) are never fed
// into a squaring operation.
// ============================================================================

const PRICE_SCALE: u128 = 1_000_000_000;
const BPS_DENOM: u128 = 10_000;

// 7 price points / 6 segments (docs/references/003-piecewise-linear/state.rs:18,21) — kept as
// named constants for documentation and for the equivalence proof in NOTES.md / this file's
// own tests, not used as a literal 7-element array in the hot path: given fixed-linear point
// spacing (this port's own ladder-shape decision, matching the source's own tests) plus
// uniform per-segment quantity (the source's own `calculate_k`), all 6 segments share the
// same width and the same liquidity constant `k`, so the whole ladder collapses algebraically
// into ONE linear-density span from P0 to P6 (proof: `tests::six_segment_walk_matches_closed_form`).
// Evaluating that closed form directly is fewer divisions (docs/DESIGN.md §2.9 cross-cutting
// finding #9) and less accumulated rounding than iterating 6 segments for the same answer.
const NUM_PRICE_POINTS: u128 = 7;
const NUM_SEGMENTS: u128 = 6;
// Documents the invariant these two constants encode (7 points bound 6 segments) — the
// only "use" `NUM_PRICE_POINTS` needs, since the collapsed closed form below only ever
// reads `NUM_SEGMENTS` directly.
const _: () = assert!(NUM_PRICE_POINTS == NUM_SEGMENTS + 1);

// === PARAMS BEGIN ===
const S0_BPS: u128 = 57; // range: 5..=200
const W_BPS: u128 = 2423; // range: 50..=2500
const DELTA_RESERVE_BPS: u128 = 763; // range: 50..=1000
                                     // === PARAMS END ===

// `S0_BPS`'s range (5..=200) is unchanged from the parent's own pre-freeze correction
// (`strategies/003-piecewise-linear/NOTES.md` § DELTA_PCT range correction): the parent's
// exhaustive coarse-grid search over its issue's original `25..=100` depth range measured
// catastrophic loss (-17027 to -20053 screening) everywhere in it, and the corrected
// `1..=10` percent depth is what this variant's `DELTA_RESERVE_BPS` re-expresses at 20x
// finer resolution. The UPPER bound is exactly the parent's own corrected ceiling (`1000`
// bps = 10%, unchanged). The LOWER bound is NOT the same as the parent's: this variant's
// own frozen space (declared in the issue, not derived here) drops it to `50` bps (0.5%),
// below the parent's own `1..=10` percent floor — a genuine, if small, widening downward,
// not merely a bps re-expression of the parent's own `1%` minimum. See this variant's own
// NOTES.md § Frozen parameter space for why: the parent's own measured flow-share-collapse
// trend (0.281 at 1% and falling) was measured only down to 1%, so 0.5%'s own viability is
// judgment extrapolated from that trend's direction, not a point the parent itself measured.

// `W_BPS`'s upper bound widens to 2500 from the parent's own frozen 1000, which the
// parent's fit converged to on a still-rising, non-degenerate plateau (`[56,941,3] ->
// 411.80`, `[56,986,3] -> 412.33`, `[56,1000,3] -> 412.46`) — genuine unresolved headroom,
// not a search artifact. 2500 is a ridge-collapse bound (this variant's own NOTES.md §
// Frozen parameter space), not a round number: this port's 6 segments collapse
// algebraically to one linear-density span, so once `W` exceeds the largest inter-anchor
// mispricing the simulation can produce, `(W, DELTA_RESERVE_BPS)` enter the economics only
// through their ratio and further widening is pure reparameterization. 2500 bps covers a
// 3.5-sigma, ~100-step arb-free stretch at `gbm_sigma_max = 0.007` — a gap that cannot
// persist un-arbitraged given the arbitrageur re-anchors at >=1 cent profit. That 100-step /
// 3.5-sigma choice is judgment, not measurement (NOTES.md, verbatim).

// `S0_BPS`'s range (5..=200) and `W_BPS`'s range (50..=2500) overlap, so a raw (S0, W) pair
// the search visits could have W <= S0, collapsing the ladder to zero/negative width. Force
// the effective outer offset strictly past the inner one regardless of the raw pair (the
// same "range-clamp any derived value" spirit as cross-cutting finding #5, applied to this
// derived width rather than to a storage byte) — the frozen ranges reaching the search are
// unchanged; only the internal derivation from a raw (S0, W) pair to a ladder is defended.
const MIN_GAP_BPS: u128 = NUM_SEGMENTS;

// `crates/cli/src/commands/validate.rs`'s own monotonicity gate probes 10 FIXED sizes
// {0.1..200} at ITS OWN fixed default state (reserve_x=100, reserve_y=10000) and requires
// STRICTLY increasing output across every pair, with no tolerance for a flat/exhausted
// plateau (unlike `crates/sim/src/curve_checks.rs`'s runtime check, which explicitly
// tolerates a flat tail beyond book exhaustion — cross-cutting finding #6). The sell side's
// `total_qty` is base-denominated and probed in base units (input_x), so at
// `DELTA_RESERVE_BPS`'s own frozen max (1000 bps = 10%) it always exhausts at <=10 tokens
// at the default state — inside that 200-token probe range for EVERY value in
// `DELTA_RESERVE_BPS`'s frozen range, so `validate`'s sell side would fail without something
// past the flat cap regardless of what the search picks.
// (The buy side's exhaustion point is in QUOTE units — roughly `total_qty * avg_price` — so
// whether it falls inside the 200-quote-token probe depends on `S0_BPS`/`W_BPS` too; this
// variant's own NOTES.md § `validate`'s buy-side exhaustion-inside-the-probe-window case
// checks this explicitly, since at small `DELTA_RESERVE_BPS` the buy side's full-book cost
// CAN fall inside the 200-quote-token probe window at states the parent never reached it at
// — a new regime, though NOT the one this variant's own fitted point (`DELTA_RESERVE_BPS=
// 763`, well above that small-delta range) actually lands in; see that section for the
// worked numbers at the committed point. The residual tail below is applied symmetrically
// to both sides as one uniform mechanism rather than conditionally, since it is cheap and
// harmless when never reached.)
//
// An earlier version of this port "fixed" this by multiplying the whole book depth by 10x —
// technically satisfies `validate`, but catastrophic in the real 10,000-step simulation
// (measured screening avg edge: -16268), because it also shrinks price impact 10x for every
// REALISTIC trade size, not just the pathological >100-token probe: the curve becomes nearly
// flat-priced against real retail/arb flow, captures almost no spread, and gets picked apart
// by the arbitrageur. Reverted. Instead: keep the real, delta-sized book depth (meaningful
// price impact at realistic sizes) and extend the curve past exhaustion with one more,
// much-steeper virtual segment (`RESIDUAL_PRICE_MULT`, `buy_base_with_quote`/
// `sell_base_for_quote`'s tail branch) — negligible for real trades, but strictly monotone
// for `validate`'s own synthetic 200-token probe. See NOTES.md § Residual tail beyond
// exhaustion.
// The residual tail's marginal price is the primary ladder's own boundary price scaled by
// this factor — `p_high * RESIDUAL_PRICE_MULT` (buy side: much MORE expensive) / `p_low /
// RESIDUAL_PRICE_MULT` (sell side: much LESS valuable) — so it is always strictly worse than
// the ladder's own ending price, which is what keeps the extended curve concave (marginal
// price only ever moves the "correct" direction, matching the piecewise-linear mechanism's
// own kink-direction guarantee). 1000x is large enough that the tail's contribution stays
// small relative to a genuine trade beyond the book while staying easily large enough to
// clear `validate`'s coarse discrete probe (see the parent's own NOTES.md for the worked
// numbers).
const RESIDUAL_PRICE_MULT: u128 = 1_000;

// Bisection halvings for the buy-side partial-fill inversion (see module doc comment and
// the parent's own NOTES.md § Do not invert with sqrt). `total_qty` is
// `reserve_x * DELTA_RESERVE_BPS/10_000`, and
// `crates/cli/src/commands/validate.rs`'s own randomized probe draws `reserve_x` up to
// ~2e12 nano, so at `DELTA_RESERVE_BPS`'s own frozen max (1000 bps = 10%) the worst case
// bisected range is on the order of `2e12 * 1000/10_000 = 2e11` — numerically identical to
// the parent's own bound, since the frozen max percentage (10%) is unchanged, only its unit
// and resolution are. 44 halvings resolve any integer target up to 2^44 (~1.76e13,
// comfortably past that), leaving a residual granularity of `upper/2^44` — far under the
// checker's 4-nano tolerance even well past that worst case (`2e11 / 2^44 ~= 1.1e-2` nano).
const BISECT_ITERS: u32 = 44;

#[cfg(not(feature = "no-entrypoint"))]
entrypoint!(process_instruction);

pub fn process_instruction(_pid: &Pubkey, _a: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.is_empty() {
        return Ok(());
    }
    match data[0] {
        0 | 1 => set_return_data_u64(compute_swap(data)),
        // tag 2 = after_swap: no-op. The ladder is re-derived fresh from live reserves on
        // every quote (see module doc comment's § Re-anchor policy) — there is no state to
        // persist, so this mirrors `001-cpmm-fee`'s own no-op arm exactly.
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

#[inline]
fn rd8(b: &[u8], o: usize) -> u128 {
    u64::from_le_bytes([
        b[o],
        b[o + 1],
        b[o + 2],
        b[o + 3],
        b[o + 4],
        b[o + 5],
        b[o + 6],
        b[o + 7],
    ]) as u128
}

pub fn compute_swap(data: &[u8]) -> u64 {
    if data.len() < 25 {
        return 0;
    }
    let side = data[0];
    if side != 0 && side != 1 {
        return 0;
    }
    let input = rd8(data, 1);
    let rx = rd8(data, 9);
    let ry = rd8(data, 17);
    if rx == 0 || ry == 0 || input == 0 {
        return 0;
    }
    // Storage (data[25..]) is deliberately never read — see module doc comment's
    // § Cold start / garbage state.
    match side {
        0 => buy_base_with_quote(input, rx, ry),
        1 => sell_base_for_quote(input, rx, ry),
        _ => 0,
    }
}

/// One side's ladder, collapsed to its equivalent single linear-density span [`p_low`,
/// `p_high`] (see this file's module doc comment for the collapse proof). `k` is the
/// source's own `calculate_k(total_qty, p_low, p_high)`.
struct Ladder {
    p_low: u128,
    p_high: u128,
    total_qty: u128,
    k: u128,
}

/// The effective outer offset for a given half-spread, guaranteed strictly greater than it
/// (see `MIN_GAP_BPS`'s doc comment).
#[inline]
fn effective_outer_bps() -> u128 {
    W_BPS.max(S0_BPS + MIN_GAP_BPS)
}

/// Native-ratio spot price (PRICE_SCALE-scaled): `reserve_y/reserve_x`. Shared by both
/// ladder builders so they always agree on where "spot" sits for a given `(rx, ry)`.
#[inline]
fn spot_price(rx: u128, ry: u128) -> u128 {
    ry.saturating_mul(PRICE_SCALE) / rx
}

/// Ask-side ladder (side=0, buying base X with quote Y): P0 = spot*(1+s0), P6 =
/// spot*(1+outer), ascending above spot. Book capacity is base-denominated (native X units)
/// — a deliberate deviation from the source's own quote-denominated `ask_side`/`bid_side`
/// convention (see the parent's own NOTES.md § Base-denominated book capacity), sized as
/// `DELTA_RESERVE_BPS` of the LIVE `reserve_x` so it can never structurally exceed the
/// reserve regardless of how far reserves have drifted over a 10,000-step simulation.
fn build_ask_ladder(rx: u128, ry: u128) -> Option<Ladder> {
    let spot = spot_price(rx, ry);
    let outer_bps = effective_outer_bps();
    let p_low = spot.saturating_mul(BPS_DENOM + S0_BPS) / BPS_DENOM;
    let p_high = spot.saturating_mul(BPS_DENOM + outer_bps) / BPS_DENOM;
    finish_ladder(rx, p_low, p_high)
}

/// Bid-side ladder (side=1, selling base X for quote Y): P0 = spot*(1-outer), P6 =
/// spot*(1-s0), ascending below spot (mirrors the ask side). Book capacity is also sized off
/// `reserve_x` (base units) — see `build_ask_ladder`'s doc comment; at `spot = ry/rx` this
/// converts to approximately `DELTA_RESERVE_BPS` of `reserve_y` in quote-payout terms as
/// well, since `reserve_x * spot ~= reserve_y` by definition of spot.
fn build_bid_ladder(rx: u128, ry: u128) -> Option<Ladder> {
    let spot = spot_price(rx, ry);
    // Unlike `build_ask_ladder` (which only ever ADDS bps to spot, so it can never underflow
    // regardless of magnitude), this side SUBTRACTS bps from `BPS_DENOM` below — a plain `-`
    // that panics on underflow. These two clamps keep that subtraction in range; they remain
    // unreachable under this variant's own widened `outer_bps` (max 2500, still far under
    // `BPS_DENOM`=10000) but are 3.75x closer to live than under the parent's max of 1000 —
    // re-verified, not merely inherited, per this variant's own NOTES.md § Shape and CU risk
    // — and are load-bearing defense-in-depth, not speculative: the ask side genuinely needs
    // no counterpart because its own arithmetic has no underflow to guard.
    let outer_bps = effective_outer_bps().min(BPS_DENOM - 1);
    let half_spread_bps = S0_BPS.min(outer_bps.saturating_sub(1));
    let p_low = spot.saturating_mul(BPS_DENOM - outer_bps) / BPS_DENOM;
    let p_high = spot.saturating_mul(BPS_DENOM - half_spread_bps) / BPS_DENOM;
    finish_ladder(rx, p_low, p_high)
}

#[inline]
fn finish_ladder(rx: u128, p_low: u128, p_high: u128) -> Option<Ladder> {
    let width = p_high.checked_sub(p_low)?;
    if width == 0 {
        return None;
    }
    // The one mechanism change of this variant (NOTES.md § Frozen parameter space): the
    // parent's `rx.saturating_mul(DELTA_PCT) / 100` (percent resolution) becomes bps
    // resolution here — bit-exact at the parent's own mapped point (`DELTA_RESERVE_BPS=300`
    // vs `DELTA_PCT=3`, since `floor(rx*300/10_000) = floor(rx*3/100)` for all `rx`: a
    // truncating division's floor is invariant under common scaling of numerator and
    // denominator), verified directly by this variant's own containment probe (NOTES.md §
    // Containment). Still one division, as before.
    let total_qty = rx.saturating_mul(DELTA_RESERVE_BPS) / BPS_DENOM;
    if total_qty == 0 {
        return None;
    }
    // calculate_k (docs/references/003-piecewise-linear/math/mod.rs:30), evaluated once over
    // the whole collapsed [p_low, p_high] span rather than once per (identical) segment.
    // At this variant's own worst corner (`DELTA_RESERVE_BPS=50` and `W_BPS=2500`, box
    // Corner A — the smallest-`k` of the four declared corners), `total_qty` is 1/6 of the
    // parent's own COMMITTED value (50 vs the parent's fitted 300, i.e. 0.5% vs 3% of
    // `reserve_x`) and `width` can be 2.5x the parent's own committed `W_BPS` (2500 vs
    // 1000), so `k` there can be ~15x smaller than
    // at the PARENT'S OWN fitted point — a materially different, and more informative,
    // comparison than against the parent's own frozen-range floor (50 vs the parent's own
    // range minimum of 100 bps is only a 2x difference). The `k == 0` guard below is
    // therefore more reachable at tiny-spot drift states than it was for the parent,
    // confirmed still uniform (monotone-safe) by `bench fuzz` PASSing at exactly that corner
    // (NOTES.md § Shape and CU risk).
    let k = total_qty.saturating_mul(PRICE_SCALE) / width;
    if k == 0 {
        return None;
    }
    Some(Ladder {
        p_low,
        p_high,
        total_qty,
        k,
    })
}

/// Exact cost (in quote units) to deliver `base` units starting from a ladder's price floor
/// `p_low`: `cost(base) = p_low*base/SCALE + base^2/(2k)` — the source's own quadratic
/// cost-of-linear-density form (`docs/references/003-piecewise-linear/math/piecewise.rs`'s
/// `calculate_quote_for_base`), monotone increasing and convex in `base`, so its inverse
/// (below) is monotone increasing and concave — exactly what the runtime shape check
/// requires.
#[inline]
fn cost_of_base(base: u128, p_low: u128, k: u128) -> u128 {
    let term1 = p_low.saturating_mul(base) / PRICE_SCALE;
    let term2 = base.saturating_mul(base) / k.saturating_mul(2);
    term1.saturating_add(term2)
}

/// Largest `base` in `[0, upper]` with `cost_of_base(base, p_low, k) <= target`, found by
/// integer bisection against the exact forward function (NOTES.md § Do not invert with
/// sqrt) rather than an `isqrt`-based closed form.
#[inline]
fn invert_cost_bisect(target: u128, p_low: u128, k: u128, upper: u128) -> u128 {
    let mut lo: u128 = 0;
    let mut hi: u128 = upper;
    for _ in 0..BISECT_ITERS {
        if lo >= hi {
            break;
        }
        let mid = lo + (hi - lo).div_ceil(2);
        if cost_of_base(mid, p_low, k) <= target {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

/// side=0: spend `input` quote units buying base off the ask ladder.
fn buy_base_with_quote(input: u128, rx: u128, ry: u128) -> u64 {
    let ladder = match build_ask_ladder(rx, ry) {
        Some(l) => l,
        None => return 0,
    };
    // Overflow short-circuit (mandatory — the parent's own NOTES.md § Overflow
    // short-circuit): compute the cost of exhausting the WHOLE book first, entirely from
    // bounded reserve-derived values, before `input` (which can be as large as ~1.8e19 nano
    // from the arbitrageur's bracket search) is ever compared against a squared term.
    let full_cost = cost_of_base(ladder.total_qty, ladder.p_low, ladder.k);
    let base_out = if full_cost == 0 {
        0
    } else if input >= full_cost {
        // Residual tail beyond exhaustion (the parent's own NOTES.md § Residual tail beyond
        // exhaustion): extend at a price `RESIDUAL_PRICE_MULT` times WORSE than the ladder's own P6, so
        // the extra base bought per extra quote spent is small — strictly positive once
        // `extra_quote` clears roughly `residual_price/PRICE_SCALE` nano (integer division
        // floors below that, to 0, which is a tie against the exhaustion point, never a
        // decrease, so it still never violates monotonicity — just not what `validate.rs`'s
        // own STRICT `>` probe would need at those exact, vanishingly narrow input pairs;
        // its 10 fixed probe sizes are spaced far coarser than that window and never land
        // inside it). A real trade this large already means the primary book is long
        // exhausted.
        let extra_quote = input - full_cost;
        let residual_price = ladder.p_high.saturating_mul(RESIDUAL_PRICE_MULT).max(1);
        let extra_base = extra_quote.saturating_mul(PRICE_SCALE) / residual_price;
        ladder.total_qty.saturating_add(extra_base)
    } else {
        invert_cost_bisect(input, ladder.p_low, ladder.k, ladder.total_qty)
    };
    // Defensive margin below the live reserve (finding #6): never let a quote reach the
    // reserve exactly, the same margin `001-cpmm-fee`/`005-vol-adaptive-cpmm-fee`'s own
    // `saturating_sub`-bounded outputs carry structurally.
    base_out.min(rx.saturating_sub(1)) as u64
}

/// side=1: sell `input` base units into the bid ladder for quote.
fn sell_base_for_quote(input: u128, rx: u128, ry: u128) -> u64 {
    let ladder = match build_bid_ladder(rx, ry) {
        Some(l) => l,
        None => return 0,
    };
    // No inversion needed: `input` (base) IS the state variable this side integrates over,
    // so the quote received is a direct forward evaluation.
    let base_consumed = input.min(ladder.total_qty);
    // quote_received(base_consumed) = p_high*base_consumed/SCALE - base_consumed^2/(2k):
    // price starts at p_high (nearest spot) and falls toward p_low as more base is sold.
    let term1 = ladder.p_high.saturating_mul(base_consumed) / PRICE_SCALE;
    let term2 = base_consumed.saturating_mul(base_consumed) / ladder.k.saturating_mul(2);
    let primary_quote = term1.saturating_sub(term2);
    let quote_out = if input > ladder.total_qty {
        // Residual tail beyond exhaustion, symmetric to the buy side: extend at a price
        // `RESIDUAL_PRICE_MULT` times WORSE than the ladder's own P0 (i.e. much less quote
        // per extra base sold) — see the buy side's own doc comment above for the same
        // integer-division floor caveat right at the exhaustion boundary (a tie there, never
        // a decrease). Always strictly worse than the primary ladder's own ending price, so
        // concavity holds across the transition.
        let extra_base = input - ladder.total_qty;
        let residual_price = (ladder.p_low / RESIDUAL_PRICE_MULT).max(1);
        let extra_quote = extra_base.saturating_mul(residual_price) / PRICE_SCALE;
        primary_quote.saturating_add(extra_quote)
    } else {
        primary_quote
    };
    quote_out.min(ry.saturating_sub(1)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn six_segment_walk_buy(input: u128, rx: u128, ry: u128) -> u128 {
        // A literal port of the source's segment-by-segment structure (NUM_PRICE_POINTS=7 /
        // NUM_SEGMENTS=6, uniform quantity per segment), used only to prove the closed form
        // above is exactly equivalent to it — not part of the runtime path.
        let ladder = match build_ask_ladder(rx, ry) {
            Some(l) => l,
            None => return 0,
        };
        let width = ladder.p_high - ladder.p_low;
        let seg_width = width / NUM_SEGMENTS;
        let q_seg = ladder.total_qty / NUM_SEGMENTS;
        if seg_width == 0 || q_seg == 0 {
            return 0;
        }
        let k_seg = q_seg.saturating_mul(PRICE_SCALE) / seg_width;
        let mut remaining_input = input;
        let mut base_out: u128 = 0;
        for seg in 0..NUM_SEGMENTS {
            if remaining_input == 0 {
                break;
            }
            let seg_p_low = ladder.p_low + seg * seg_width;
            let seg_full_cost = cost_of_base(q_seg, seg_p_low, k_seg);
            if remaining_input >= seg_full_cost {
                base_out += q_seg;
                remaining_input -= seg_full_cost;
            } else {
                base_out += invert_cost_bisect(remaining_input, seg_p_low, k_seg, q_seg);
                remaining_input = 0;
            }
        }
        base_out
    }

    #[test]
    fn six_segment_walk_matches_closed_form() {
        // Uniform per-segment quantity + fixed-linear spacing collapses the 6-segment walk
        // to the single closed form (module doc comment's claim) — verified directly rather
        // than merely asserted, across many (reserve, input) pairs. Every case here stays
        // within the primary (non-exhausted) book: the residual tail beyond exhaustion
        // (`RESIDUAL_PRICE_MULT`) is this port's own addition on top of the ported
        // mechanism and is covered separately by
        // `residual_tail_is_strictly_monotone_and_negligible`.
        let cases: &[(u128, u128, u128)] = &[
            (100_000_000_000, 10_000_000_000_000, 1_000_000_000),
            (100_000_000_000, 10_000_000_000_000, 50_000_000_000),
            (100_000_000_000, 10_000_000_000_000, 100_000_000_000),
            (2_000_000_000_000, 200_000_000_000_000, 999_999_999),
            (5_000_000_000, 1_000_000_000_000, 3_000_000_000),
        ];
        for &(rx, ry, input) in cases {
            let closed = buy_base_with_quote(input, rx, ry) as u128;
            let segmented = six_segment_walk_buy(input, rx, ry);
            // Segment-boundary integer-division rounding can differ from the whole-span
            // closed form by a handful of nanos per boundary crossed (<=6 segments); assert
            // agreement well inside the checker's own 4-nano tolerance times that bound.
            let diff = closed.abs_diff(segmented);
            assert!(
                diff <= 30,
                "closed form {closed} vs segmented {segmented} diverge by {diff} nanos \
                 for rx={rx}, ry={ry}, input={input}"
            );
        }
    }

    #[test]
    fn residual_tail_is_strictly_monotone_and_negligible() {
        // Beyond book exhaustion, output must keep strictly increasing (validate.rs's own
        // discrete probe requires it) but stay small relative to a real trade of the same
        // size against the primary book.
        let rx = 100_000_000_000u128;
        let ry = 10_000_000_000_000u128;
        let mut prev = buy_base_with_quote(20_000_000_000_000, rx, ry); // well past exhaustion
        for extra in 1..=5u128 {
            let input = 20_000_000_000_000 + extra * 1_000_000_000_000;
            let out = buy_base_with_quote(input, rx, ry);
            assert!(out > prev, "residual tail did not strictly increase");
            prev = out;
        }
        // The residual tail's own marginal rate should be far thinner than the primary
        // ladder's: 1 extra token of quote well past exhaustion should buy much less base
        // than 1 token bought from a fresh, unexhausted book at the same reserves.
        let fresh = buy_base_with_quote(1_000_000_000, rx, ry);
        let residual_step = buy_base_with_quote(21_000_000_000_000, rx, ry)
            - buy_base_with_quote(20_000_000_000_000, rx, ry);
        assert!(
            residual_step < fresh,
            "residual tail rate ({residual_step}) should be thinner than a fresh-book rate ({fresh})"
        );
    }

    #[test]
    fn cold_start_and_garbage_storage_are_identical() {
        // compute_swap never reads storage at all, so a zeroed buffer and a garbage-filled
        // one must produce byte-identical instruction encodings past offset 25.
        let mut data_zero = vec![0u8; 25 + 1024];
        data_zero[0] = 0;
        data_zero[1..9].copy_from_slice(&1_000_000_000u64.to_le_bytes());
        data_zero[9..17].copy_from_slice(&100_000_000_000u64.to_le_bytes());
        data_zero[17..25].copy_from_slice(&10_000_000_000_000u64.to_le_bytes());

        let mut data_garbage = data_zero.clone();
        for (i, b) in data_garbage[25..].iter_mut().enumerate() {
            *b = (i * 37 + 11) as u8;
        }

        assert_eq!(compute_swap(&data_zero), compute_swap(&data_garbage));
    }

    #[test]
    fn buy_output_never_reaches_reserve_x() {
        let rx = 100_000_000_000u128;
        let ry = 10_000_000_000_000u128;
        for &input in &[
            1u128,
            1_000_000_000,
            1_000_000_000_000,
            10_000_000_000_000_000,
            1_800_000_000_000_000_000,
        ] {
            let mut data = vec![0u8; 25];
            data[0] = 0;
            data[1..9].copy_from_slice(&(input as u64).to_le_bytes());
            data[9..17].copy_from_slice(&(rx as u64).to_le_bytes());
            data[17..25].copy_from_slice(&(ry as u64).to_le_bytes());
            let out = compute_swap(&data) as u128;
            assert!(out < rx, "output {out} reached or exceeded reserve {rx}");
        }
    }

    #[test]
    fn sell_output_never_reaches_reserve_y() {
        let rx = 100_000_000_000u128;
        let ry = 10_000_000_000_000u128;
        for &input in &[
            1u128,
            1_000_000_000,
            1_000_000_000_000,
            10_000_000_000_000_000,
            1_800_000_000_000_000_000,
        ] {
            let mut data = vec![0u8; 25];
            data[0] = 1;
            data[1..9].copy_from_slice(&(input as u64).to_le_bytes());
            data[9..17].copy_from_slice(&(rx as u64).to_le_bytes());
            data[17..25].copy_from_slice(&(ry as u64).to_le_bytes());
            let out = compute_swap(&data) as u128;
            assert!(out < ry, "output {out} reached or exceeded reserve {ry}");
        }
    }

    #[test]
    fn monotone_and_concave_across_a_dense_buy_sweep() {
        let rx = 100_000_000_000u128;
        let ry = 10_000_000_000_000u128;
        let mut prev_out: i128 = -1;
        let mut prev_slope: Option<f64> = None;
        let mut prev_in = 0u128;
        for i in 1..=400u128 {
            let input = i * 5_000_000_000;
            let mut data = vec![0u8; 25];
            data[0] = 0;
            data[1..9].copy_from_slice(&(input as u64).to_le_bytes());
            data[9..17].copy_from_slice(&(rx as u64).to_le_bytes());
            data[17..25].copy_from_slice(&(ry as u64).to_le_bytes());
            let out = compute_swap(&data) as i128;
            assert!(out >= prev_out, "monotonicity violated at input {input}");
            if prev_in > 0 {
                let slope = (out - prev_out) as f64 / (input - prev_in) as f64;
                if let Some(ps) = prev_slope {
                    assert!(
                        slope <= ps + 1e-6,
                        "concavity violated at input {input}: slope {slope} > prev {ps}"
                    );
                }
                prev_slope = Some(slope);
            }
            prev_out = out;
            prev_in = input;
        }
    }

    #[test]
    fn monotone_and_concave_across_a_dense_sell_sweep() {
        let rx = 100_000_000_000u128;
        let ry = 10_000_000_000_000u128;
        let mut prev_out: i128 = -1;
        let mut prev_slope: Option<f64> = None;
        let mut prev_in = 0u128;
        for i in 1..=400u128 {
            let input = i * 250_000_000;
            let mut data = vec![0u8; 25];
            data[0] = 1;
            data[1..9].copy_from_slice(&(input as u64).to_le_bytes());
            data[9..17].copy_from_slice(&(rx as u64).to_le_bytes());
            data[17..25].copy_from_slice(&(ry as u64).to_le_bytes());
            let out = compute_swap(&data) as i128;
            assert!(out >= prev_out, "monotonicity violated at input {input}");
            if prev_in > 0 {
                let slope = (out - prev_out) as f64 / (input - prev_in) as f64;
                if let Some(ps) = prev_slope {
                    assert!(
                        slope <= ps + 1e-6,
                        "concavity violated at input {input}: slope {slope} > prev {ps}"
                    );
                }
                prev_slope = Some(slope);
            }
            prev_out = out;
            prev_in = input;
        }
    }

    #[test]
    fn zero_amount_and_zero_reserves_return_zero() {
        assert_eq!(compute_swap(&[]), 0);
        let mut data = vec![0u8; 25];
        data[0] = 0;
        // input=0
        assert_eq!(compute_swap(&data), 0);
        data[1..9].copy_from_slice(&1_000_000_000u64.to_le_bytes());
        // rx=0, ry=0
        assert_eq!(compute_swap(&data), 0);
    }

    #[test]
    fn invalid_side_returns_zero() {
        let mut data = vec![0u8; 25];
        data[0] = 7;
        data[1..9].copy_from_slice(&1_000_000_000u64.to_le_bytes());
        data[9..17].copy_from_slice(&100_000_000_000u64.to_le_bytes());
        data[17..25].copy_from_slice(&10_000_000_000_000u64.to_le_bytes());
        assert_eq!(compute_swap(&data), 0);
    }

    #[test]
    fn get_name_and_model_are_nonempty() {
        assert_eq!(NAME, "003b Wider Band x Deeper Book");
        assert!(!get_model_used().is_empty());
    }
}
