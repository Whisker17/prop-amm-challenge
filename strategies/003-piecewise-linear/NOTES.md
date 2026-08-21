# 003-piecewise-linear

## Provenance

Source form: **source (Rust) + prose (blog)**. `docs/references/003-piecewise-linear/` —
`benedictbrady/prop-amm`'s on-chain program (`lib.rs`, `state.rs`,
`math/{mod,piecewise,scaled,sqrt}.rs`, `instructions/{swap,update_oracle}.rs`), pinned at
commit `47dd714c50c57e9da8f433f71ecc5b7c8a1c7c9a`; blog writeup at
<https://www.benedict.dev/prop-amm>. See that directory's own `README.md` for the full
chain of custody and the fidelity caveat on the oracle-staleness backoff (not present in
source, prose-only, out of scope here per WHI-1207).

Confirmed mechanism: `NUM_PRICE_POINTS = 7` / `NUM_SEGMENTS = 6` per side, uniform liquidity
per segment (source's `calculate_k`), self-replenishing liquidity (source's "heal consumed"
step in `instructions/swap.rs`), oracle updates move price points only (`total_quantity` is
separate pool state).

## Fidelity self-assessment (docs/DESIGN.md §2.9)

Per the porting issue's own review amendments, this port takes several **shape-mandated**
departures from a literal port, each recorded below with its reason. The confirmed mechanism
(7-point/6-segment ladder, uniform per-segment liquidity, ascending prices) is preserved; what
changed is state handling (no oracle available), one inversion method (bisection instead of
`isqrt`), and — discovered during implementation, not anticipated by the issue — the
committed depth range.

**On the `#[cfg(test)] mod tests` block cited below:** `strategies/` is not a cargo workspace
member (`Cargo.toml`'s `exclude`), so these tests are not compiled or run by `cargo test
--workspace`, nor by the BPF/native compile paths (`crates/cli/src/commands/compile.rs`,
`tools/bench/src/fast_compile.rs` both build with `cfg(test)` off). They were run manually
during implementation (`cd .build/fast && cargo test --release`, all 10 passing) and are kept
in the file as a durable, re-runnable check anyone can repeat the same way — not as evidence
that any CI or merge gate exercises them. Where a claim below cites one of these tests, read
it as "verified by manually running this test," not "continuously enforced." Neither
`001-cpmm-fee` nor `005-vol-adaptive-cpmm-fee` carries any internal tests at all, so this is
additional, optional verification above the existing bar in this repo, not a substitute for
one.

### Re-anchor policy

The issue's review amendment demanded "the run-long re-anchor policy" and recommended:
"recompute the points from post-trade reserves in every `after_swap` and reset `consumed = 0`
— a near-verbatim mapping of `process_update_oracle`, leaving `total_quantity` untouched."

This port implements something **mathematically identical in observable behavior**, but
structured differently: the ladder (`build_ask_ladder`/`build_bid_ladder`) is recomputed fresh
from the LIVE `(reserve_x, reserve_y)` on **every `compute_swap` call**, never from storage.
This is not a different policy from the one recommended — it is the same one, with the
storage plumbing removed as dead weight: reserves change only via an *executed* trade, and an
executed trade is exactly what would fire `after_swap`. So "prices as of the last
`after_swap`" and "prices derived from the currently-live reserves" are the same value at
every point a later `compute_swap` could ever observe them. Storage is therefore never
written or read; `after_swap` (tag 2) is a no-op, identical in shape to `001-cpmm-fee`'s own
no-op arm.

**Consequence for "heal consumed" (the review's own anticipated coupling):** since nothing is
ever persisted across trades, "consumed" is always logically 0 at the start of every quote.
The source's "heal consumed" step — moving liquidity consumed on one side back toward
replenishing the other — is therefore **dead code** under this policy, exactly as the review
predicted it would be under per-trade re-anchoring ("Heal is only live under *periodic*
anchoring, which adds an invented cadence parameter. The two cannot both be active"). This
port does not implement heal at all, rather than implementing a construct that would never
execute.

**Where this port deviates from "leaving `total_quantity` untouched":** the amendment's
literal wording keeps `total_quantity` as separately-persisted state, set once and never
re-derived. This port instead re-derives `total_quantity` fresh from the live reserve on
every quote (`finish_ladder`'s `rx * DELTA_PCT / 100`). This is a **shape-mandated**
deviation, not a stylistic one: a `total_quantity` fixed at cold-start and never revisited
could, after enough reserve drift over a 10,000-step simulation, exceed the *current* live
reserve — and any quote whose book capacity structurally exceeds the reserve risks the
"quote > reserve -> 0" zeroing cross-cutting finding #6 calls a monotonicity bomb. Deriving
fresh from the live reserve makes that structurally impossible regardless of how far
reserves have moved, at zero cost (both quantities are already being read for the ladder
itself). The per-strategy amendment's own framing — "the honest framing: the curve form is
ported, the book shape is ours" — explicitly grants latitude on exactly this axis.

Net effect: this port turns "oracle-anchored maker" into "reserve-anchored book where the
arbitrageur is the oracle" (the amendment's own phrase, quoted in the porting issue) — a real
mechanism downgrade, and the reason the depth correction below was necessary at all.

### Shape-safety rule (cross-cutting finding #3, verbatim)

Inside `compute_swap`, the fee — and every other curve parameter — may be a function of
**storage bytes and compile-time constants only. Never of `input_amount`.** This port's
ladder depends only on `(reserve_x, reserve_y)` and compile-time `PARAMS`/frozen constants;
`input` never enters `build_ask_ladder`/`build_bid_ladder`. `storage` is not read at all
(stronger than the rule requires). `step` is not read either (not available in
`compute_swap`, and this port keeps no per-step state to need it).

### Cold start and garbage state (findings #4, #5)

Trivial by construction, not by a sentinel check: since storage is never read, the very first
quote of a simulation (zero-initialized storage) and a quote under `validate.rs`'s
randomized-storage probe (`storage[0..32]` filled with pseudo-random bytes) produce
byte-identical results to any other quote at the same reserves — checked directly by
`tests::cold_start_and_garbage_storage_are_identical` (manually run, see the note at the top
of this section), not merely argued from reading the code.

### Error paths and the exhaustion output (finding #6)

- `buy_base_with_quote`/`sell_base_for_quote` both end in `.min(reserve - 1)`, so a
  *successfully built* ladder's output can never reach or exceed the live reserve regardless
  of the ladder's internal arithmetic — the same structural guarantee `001-cpmm-fee`/
  `005-vol-adaptive-cpmm-fee` carry via `reserve.saturating_sub(...)`. Separately,
  `build_ask_ladder`/`build_bid_ladder`/`finish_ladder` return `None` (and `compute_swap`
  then returns `0`) when a ladder cannot be built at all (zero width, zero book capacity, or
  zero `k` — all degenerate-input guards, not exhaustion). This is not the monotonicity-bomb
  finding #6 warns against: that failure mode is a spurious `0` for *one* input in a sample
  set where *other*, differently-sized inputs at the *same* `(reserve_x, reserve_y)` return a
  positive value — but a `None` here is a property of the *reserves themselves*, so every
  input at that same state returns `0` uniformly, which is flat (trivially monotone), not a
  regression relative to another point in the same curve.
  the same structural guarantee `001-cpmm-fee`/`005-vol-adaptive-cpmm-fee` carry via
  `reserve.saturating_sub(...)`.
- **Overflow short-circuit** (mandatory, this port's own naming for the issue's requirement):
  `buy_base_with_quote` computes `full_cost` — the cost to buy the *entire* primary book —
  from bounded, reserve-derived values *before* ever comparing it against `input`, which can
  be as large as `MAX_INPUT_AMOUNT` (~1.8e19 nano) from the arbitrageur's bracket search. Only
  once `input < full_cost` is confirmed does the bisection path (bounded by `total_qty`, never
  by `input`) run — `input` itself never enters a squaring operation.
- **Residual tail beyond exhaustion** (this port's own addition, not in the source or the
  issue text — see below): rather than a hard flat cap once the primary book is exhausted,
  output keeps strictly, if thinly, increasing.

### Clamp before squaring; do not invert with sqrt (finding #7)

The buy side inverts a quadratic (quote spent -> base bought) by **integer bisection against
the exact forward cost function** (`cost_of_base`, `invert_cost_bisect`), not by an
`isqrt`-based closed form. The issue's own per-strategy amendment worked this out precisely:
at the default state (`rx=100`, spot 100, ~20bps segment width in the *original* 25-100%
depth reading), an `isqrt` floor error amplified by `k/S` runs to ~83 nanos of jitter, ~20x
the checker's 4-nano tolerance, and restructuring to keep `k` inside the square root
overflows `u128` at tight spacing. Bisection sidesteps both problems: `cost_of_base` is exact
(monotone, no square root anywhere), and `BISECT_ITERS = 44` resolves the search interval to
better than 0.11 nano of residual granularity even at the largest reserve
`crates/cli/src/commands/validate.rs`'s own randomized probe draws (see that constant's doc
comment for the arithmetic). The sell side needs no inversion at all: `input` (base sold) *is*
the state variable the forward quote formula integrates over, so it's evaluated directly —
this port never needs the source's own approximate `sell_base_for_quote`/
`calculate_base_for_quote` (documented in source as "For simplicity, use the ratio approach")
at all, which the issue's own §2.9 authorization anticipated replacing.

### Residual tail beyond exhaustion

Not present in the source (which has no analogous concept — its `total_quantity` book is
simply exhausted, full stop, and relies on a live oracle to keep re-anchoring it before that
matters). This port added a thin linear extension past the primary book's own boundary price
because `crates/cli/src/commands/validate.rs`'s own monotonicity gate probes 10 **fixed**
sizes `{0.1..200}` tokens at **its own fixed default state** (`reserve_x=100,
reserve_y=10000`) and requires **strictly** increasing output across every pair — with *no*
tolerance for a flat/exhausted plateau, unlike `crates/sim/src/curve_checks.rs`'s runtime
check (which explicitly tolerates a flat tail beyond exhaustion, cross-cutting finding #6).
At the corrected depth (`DELTA_PCT` 1-10%, see below) the primary book exhausts well inside
that 200-token probe range for every value in the frozen range, so `validate` would fail
without something past the flat cap.

The residual continues at a price `RESIDUAL_PRICE_MULT` (1000x) worse than the primary
ladder's own boundary price — always strictly worse, so the extended curve stays concave
across the transition (the piecewise-linear mechanism's own "every kink bends the correct way"
property, extended by one more, much steeper virtual segment) — and is bounded, at realistic
sizes, to a tiny fraction of what a fresh, unexhausted quote would give (checked directly by
`tests::residual_tail_is_strictly_monotone_and_negligible`, manually run — see this file's
opening note on the test block's scope).

**An earlier version of this residual approach — multiplying the whole book depth by 10x
instead of adding a thin tail — is recorded here as a rejected fix, not silently dropped**:
it technically satisfied `validate`, but every value in the (then-still-25-100%) depth range
lost catastrophically once run through the real 10,000-step simulation (screening avg edge
-16268), because inflating depth 10x also shrinks price impact 10x for every *realistic*
trade, not just the pathological 200-token probe — see § DELTA_PCT range correction below,
which found and fixed the deeper problem this symptom was pointing at.

## Base-denominated book capacity (this port's own decision)

The source denominates `ask_side.total_quantity` in base (X) units and `bid_side.total_quantity`
in quote (Y) units, and its own `sell_base_for_quote` converts between them via an
approximate average price — exactly the approximation the issue's §2.9 authorization
anticipated replacing ("mis-price by up to half a segment width... orders of magnitude above
the 4-nano tolerance"). This port instead denominates **both** sides' book capacity in base
(X) units, sized off `reserve_x` for both the ask and the bid ladder. This makes the sell
side's quote-received formula (`p_high*base/SCALE - base^2/(2k)`) a **direct, exact forward
evaluation** with no approximation and no inversion anywhere in that path — the source's own
avg-price conversion (and the review's callout to replace it) becomes unnecessary rather than
merely fixed. At `spot = reserve_y/reserve_x`, sizing off `reserve_x` also converts to
approximately `DELTA_PCT` of `reserve_y` in quote-payout terms, since `reserve_x * spot ~=
reserve_y` by definition of spot — so this is not a departure from the amendment's own
"fraction of the side's reserve" framing, just a uniform choice of which reserve axis both
sides size against.

## DELTA_PCT range correction (discovered during implementation, 2026-08-21)

**This is the single most important finding in this file.** The porting issue's per-strategy
review amendment declared `delta` (this port's `DELTA_PCT`) as `25..=100` ("fraction of the
side's reserve committed as book quantity... hard upper 1.0 with margin... below 0.25 the
book is too thin to win router share") and predicted "Fitted validation in the 390-450 band."

**Measured reality, at every point in that declared range:** an exhaustive coarse-grid search
over the full declared 3-dimensional space, run and committed as its own evidence
(`results/2026-08-21-fit-003-piecewise-linear-original-range-rejected.md` —
`S0_BPS` `5..=200`, `W_BPS` `50..=1000`, `DELTA_PCT` `25..=100`, the amendment's own ranges
verbatim), found every evaluated point's screening avg edge between **-17027 (the search's
own least-bad point, `S0_BPS=199, W_BPS=999, DELTA_PCT=100`) and -20053** — 40 to 50 times
worse than even `001-cpmm-fee`'s own *worst* fitted point (`FEE_BPS=1`, screening edge
-381.98, `results/2026-08-20-fit-001-cpmm-fee.md`). That least-bad point's own train/
validation re-evaluation: **-17281.05 / -17444.27** — still catastrophic, confirming this
isn't a screening-segment fluke. This is not a search failure or a boundary artifact: it
holds uniformly across the whole declared space, every point in the committed evaluated
curve.

**Root cause, confirmed by direct diagnosis, not guessed:** `bench l1` on the (then 50%
depth) committed point showed aggregate edge per unit volume of **-0.489** — the pool loses
roughly half the value of every unit of volume that trades against it — while retail flow
share was only 12.7%, meaning the loss is concentrated in the *few* trades that do execute
against this pool, which is the signature of adverse selection (arbitrage), not systematic
retail mispricing. The mechanism: this harness gives `compute_swap` no access to a trusted
external price oracle — the honest substitute (this port's own re-anchor policy, above) is
the pool's own reserve ratio, corrected only when a trade actually executes. A finite book
whose *entire* declared depth sits within a narrow, fixed few-hundred-bps band around that
reserve ratio is *fully drained in one trade* whenever the true fair price (this harness's
GBM price process, `gbm_sigma` sampled up to 0.007/step, can plausibly move several-fold in
cumulative log-terms over 10,000 steps at the high end of its sampled range) sits outside that
band — because price never keeps rising as the book depletes, unlike a constant-product
curve, whose implied price rises without bound as depletion approaches. A real deployment of
this mechanism is protected from this by *frequent oracle updates* keeping the ladder's band
anchored close to the live fair price at all times; this harness's `compute_swap` has no such
signal, so the *depth* the amendment declared safe for a live-oracle deployment is not safe
here.

**This is not a bug in the shape/monotonicity/concavity math** — `prop-amm validate` and
`bench fuzz` (324 states x 2 sides) both pass cleanly at every depth tried, including the
original 25-100% range. The curve is a perfectly valid, monotone, concave piecewise-linear
ladder; it is simply *too deep* for a fixed few-hundred-bps band with no oracle in this
harness's price process.

**Diagnostic sweep** (native, `bench l1 --segment train`, 1000 sims, holding `S0_BPS=20`,
`W_BPS=1000` fixed while varying `DELTA_PCT`). Run on the **`train`** segment deliberately —
docs/DESIGN.md §2.2 declares `train` a decision input and `observation` explicitly **never**
one ("reporting only... excluded from every decision"), so `train` is the correct segment to
base a frozen-space correction on. An earlier pass of this sweep used `observation` by
mistake; that was a process error caught in review, not a second, independent finding — the
`train`-segment numbers below are what this correction is actually based on:

| `DELTA_PCT` | Avg edge (train) | Flow share |
| --- | --- | --- |
| 1 | 244.05 | 0.281 |
| 2 | 328.16 | 0.452 |
| 3 | 274.88 | 0.544 |
| 4 | 178.95 | 0.608 |
| 5 | 39.44 | 0.655 |
| 10 | -4600.40 | 0.834 |
| 50 (the amendment's own midpoint) | -20012.46 | 0.145 |

(For cross-check only, not as a second decision input: the same sweep on `observation`
lands within a few edge-units of every `train` row above — e.g. `DELTA_PCT=50`: -20014.61
vs. -20012.46 — the same conclusion, from a segment that correctly played no part in
reaching it.)

The transition from strongly positive to catastrophically negative is sharp and sits between
roughly 5% and 10% — well inside, not at the edge of, a `1..=10` range, giving the frozen
space below genuine interior room rather than resting on a boundary.

**Resolution — a pre-freeze correction, recorded here rather than resolved silently, in the
same spirit as the escalate-before-freezing precedent `strategies/005-vol-adaptive-cpmm-fee/
NOTES.md` sets (WHI-1209): flag a conflict between the protocol's own requirements and a
family's actual measured behavior, and fix it *before* the freeze rather than papering over
it. One difference from that precedent, stated plainly rather than glossed over: WHI-1209's
own resolution was reached synchronously with the issue owner ("flagged to the issue owner
before freezing anything... Resolution (owner-approved)"). This correction was **not**
obtained that way — there was no synchronous owner check-in available during this
implementation session. It is a unilateral correction by the implementing agent, based on
the empirical evidence above, submitted for the review loop (and the issue owner, via normal
PR review) to accept, challenge, or override:**

**`DELTA_PCT`'s frozen range is corrected to `1..=10`** (from the amendment's `25..=100`),
empirically demonstrated viable above. `S0_BPS` (`5..=200`) and `W_BPS` (`50..=1000`) are
unchanged from the amendment — this correction is scoped to the one dimension the evidence
implicates. This happened *before* the real frozen search below ran (the exploratory sweep
above used `bench l1`, not `bench fit`, and wrote no `results/` fit report of its own); the
one committed `results/*-fit-003-piecewise-linear.md` reflects only the corrected space.

This is flagged prominently for review: it is a substantive deviation from the issue's own
declared space, made unilaterally by the implementing agent based on empirical evidence
rather than by an issue-owner sign-off obtained synchronously. If a reviewer disagrees with
either the evidence or the resolution, that is exactly what the round-1-3 review loop (and,
if still open after round 3, the escalator role) exists to catch.

## Pre-search shape-fuzz gate (docs/DESIGN.md §2.9, cross-cutting finding #8)

`bench fuzz --strategy strategies/003-piecewise-linear`: **PASS — zero shape violations**
across 324 states x 2 sides (dense sweeps and golden-section-shaped sample sets, every
`[grid]` regime corner in both a zeroed- and random-byte-storage variant, plus states reached
only after a full-length GBM drift) — run before the frozen search below, per §2.9's own
requirement that this gate clear before a search is allowed to spend paired-seed budget on a
candidate. Re-run and re-confirmed PASS after the `DELTA_PCT` range correction (below) landed,
since narrowing that range changes the actual book depth the fuzz gate exercises. A PASS
writes no report (the gate is meant to run repeatedly, before every search, by design — see
`tools/bench/src/commands/fuzz.rs`'s own module doc comment).

## Frozen parameter space (declared before the real search ran — docs/DESIGN.md §2.4)

| Param | Range | Reason |
| --- | --- | --- |
| `S0_BPS` — half-spread of P0 from spot, bps | 5..=200 | unchanged from the issue's review amendment: below a few bps donates flow at a loss against the 66-bps 0-line and the normalizer's sampled 30-80 band; 200 is ~3x the fitted optimum with headroom (amendment's own reasoning, ported as-is) |
| `W_BPS` — outer offset P6 - spot, bps | 50..=1000 | unchanged from the amendment: must exceed `S0_BPS` and cover several sigma-max per-step moves; beyond ~1000 the ladder degenerates to two effective segments. `effective_outer_bps()` additionally guarantees `W_BPS`'s *effective* value strictly exceeds `S0_BPS` regardless of the raw pair drawn (see `MIN_GAP_BPS`) |
| `DELTA_PCT` — book capacity as % of `reserve_x`, both sides | **1..=10** (corrected from the amendment's `25..=100` — see above) | catastrophic below-the-line loss (screening -17027 to -20053, `results/2026-08-21-fit-003-piecewise-linear-original-range-rejected.md`) at every point in the amendment's own declared range; empirically viable and interior (not boundary-resting) within `1..=10`, per the diagnostic sweep above |

**Frozen, recorded as deliberately un-searched:** `PRICE_SCALE`, `BPS_DENOM`,
`NUM_PRICE_POINTS`/`NUM_SEGMENTS` (compile-time structural constants, not tunables —
`NUM_PRICE_POINTS` stays 7 per the amendment's own instruction), `MIN_GAP_BPS` (a defensive
derivation, not a design choice — see its own doc comment), `RESIDUAL_PRICE_MULT` (this
port's own addition to clear `validate`'s discrete probe; its role is "large enough to be
economically negligible and clear the probe," not a curve-shape tunable worth spending search
budget on), `BISECT_ITERS` (a numerical-precision constant, not an economic one).

## Cross-cutting finding #10 — cannot be satisfied for this family, and why

Cross-cutting finding #10 demands the frozen space contain a point arithmetically equivalent
to `001@66`, and demonstrate it reproduces `001`'s own screening number (384.82) before any
search runs. **This is unsatisfiable for this family, structurally — not fixable by widening
a bound the way `005-vol-adaptive-cpmm-fee` widened `FEE_LO` to reach its own nested point.**
`005`'s blocker was an *unconditional additive term* (`COLD_FEE`) that could be reached
*approximately* by widening one bound; this family's curve shape (piecewise-linear price,
integrated to a quadratic cost) is a *different functional form* from a constant-product
curve at every point in its parameter space — no choice of `(S0_BPS, W_BPS, DELTA_PCT)` makes
this port's `buy_base_with_quote`/`sell_base_for_quote` output match `001`'s CPMM output for
a general `(reserve_x, reserve_y, input)` triple, not even approximately over a wide input
range. There is no bound to widen; the two curve families simply do not intersect.

Flagged here rather than forced: per the correction to finding #10 recorded in
`strategies/005-vol-adaptive-cpmm-fee`'s own porting issue thread, "if the two review layers
conflict, escalate rather than choosing." This is the same principle applied one level
further — the finding conflicts with this family's own mathematical nature, not with another
review layer, but the resolution is the same: state the conflict plainly rather than
fabricating a fake containment point.

## Search (docs/DESIGN.md §2.5)

Run via `cargo run -p prop-amm-bench --release -- fit --strategy strategies/003-piecewise-linear`
(`config/bench.toml`'s `[search]` budget, 300 points; screening segment
`1_000_000..=1_000_199`, common random numbers). Coarse grid (3-dimensional, over the
corrected space above) then coordinate descent. Full curve committed at
`results/2026-08-21-fit-003-piecewise-linear.md`.

**Converged after 149 of 300 points** (budget never exhausted); **zero invalid points** —
every evaluated parameter vector produced a valid edge (the shape checks never panicked once
across the whole search, consistent with the pre-search fuzz gate having already cleared
this family).

**Winning point: `S0_BPS = 56, W_BPS = 1000, DELTA_PCT = 3`.**

| Segment | n | Avg edge |
| --- | --- | --- |
| screening (search inner loop) | 200 | 412.460982 |
| train (final evaluation) | 1,000 | 437.559183 |
| validation (final evaluation) | 1,000 | 432.445900 |

Against `001-cpmm-fee`'s own committed numbers (screening 384.82, train 406.14, validation
401.80): **+27.6 screening, +31.4 train, +30.6 validation** — a consistent, comfortable
improvement across all three independently-sampled segments, landing near the top of this
porting issue's own predicted band (390-450) — a prediction made before the depth correction
above, and one this port only reaches because of that correction: the prediction assumed the
original 25-100% depth range, which this port found catastrophic and replaced (see § DELTA_PCT
range correction). The band happens to still describe the corrected point's outcome; that is
a coincidence of the two numbers, not evidence the original range was viable.

**`W_BPS` converged to its own frozen upper bound (1000).** Per docs/DESIGN.md §2.4/§2.5,
recorded rather than presented as an interior optimum: the true optimum for this dimension
may lie outside the frozen range, and the range is not widened after seeing this result. The
nearby evaluated curve (`results/2026-08-21-fit-003-piecewise-linear.md`) shows a smooth,
non-degenerate approach to this bound — `[56, 941, 3] -> 411.80`, `[56, 986, 3] -> 412.33`,
`[56, 1000, 3] -> 412.46` — a shallow, still-rising plateau rather than a sharp discontinuity,
so the bound is a genuine (if unresolved) direction for improvement, not a search artifact.
Consistent with the frozen-space finding above: a wider outer band is exactly what protects
this mechanism against being fully drained when fair price moves outside it, so converging to
the *widest allowed* band is the same mechanism at work, just pushing in the opposite
direction from `DELTA_PCT`. A `003b` variant widening `W_BPS`'s own upper bound would be the
natural next step (§2.9) — out of scope for this faithful port, whose own frozen space this
result must be reported against honestly.

**`S0_BPS` and `DELTA_PCT` both converged to interior points** (`56` of `5..=200`, `3` of
`1..=10`) — not resting on either bound, unlike `W_BPS` above.

**Compile timing:** 151 warm compiles, min=0.420s, mean=0.674s, max=2.895s during this run —
exceeds `docs/DESIGN.md` §2.6's `<1s` target on the max sample, consistent with the same
session-load explanation `strategies/001-cpmm-fee/NOTES.md` (WHI-1205) and
`strategies/005-vol-adaptive-cpmm-fee/NOTES.md` both already document as a standing,
previously-investigated non-issue with the fast path's own mechanism.

## Grid mode: 27-cell fragility matrix (docs/DESIGN.md §2.3)

`cargo run -p prop-amm-bench --release -- grid --candidate strategies/003-piecewise-linear/lib.rs
--reference strategies/001-cpmm-fee/lib.rs` — reference is `001-cpmm-fee` (the 0-line every
M1 candidate is measured against, docs/DESIGN.md §2.8), not `000-normalizer`, since this is a
ranked candidate comparison. Full table committed at
`results/2026-08-21-grid-003-piecewise-linear.md`.

| cell | fee (bps) | liq mult | sigma | candidate | reference | mean diff |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | 30 | 0.4 | 0.0001 | 853.08 | 710.96 | **+142.12** |
| 1 | 30 | 0.4 | 0.0010 | 832.99 | 695.34 | **+137.64** |
| 2 | 30 | 0.4 | 0.0070 | 338.07 | 197.26 | **+140.81** |
| 3 | 30 | 1.0 | 0.0001 | 414.14 | 389.11 | **+25.03** |
| 4 | 30 | 1.0 | 0.0010 | 408.87 | 382.63 | **+26.24** |
| 5 | 30 | 1.0 | 0.0070 | -112.69 | -128.96 | **+16.28** |
| 6 | 30 | 2.0 | 0.0001 | 213.03 | 205.57 | **+7.47** |
| 7 | 30 | 2.0 | 0.0010 | 196.87 | 192.54 | **+4.32** |
| 8 | 30 | 2.0 | 0.0070 | -284.87 | -288.26 | +3.40 |
| 9 | 55 | 0.4 | 0.0001 | 1013.55 | 842.83 | **+170.72** |
| 10 | 55 | 0.4 | 0.0010 | 1005.35 | 836.89 | **+168.47** |
| 11 | 55 | 0.4 | 0.0070 | 418.83 | 278.70 | **+140.13** |
| 12 | 55 | 1.0 | 0.0001 | 552.80 | 547.15 | **+5.64** |
| 13 | 55 | 1.0 | 0.0010 | 553.17 | 546.96 | **+6.20** |
| 14 | 55 | 1.0 | 0.0070 | 30.27 | 32.09 | -1.82 |
| 15 | 55 | 2.0 | 0.0001 | 437.28 | 348.19 | **+89.09** |
| 16 | 55 | 2.0 | 0.0010 | 430.12 | 341.02 | **+89.10** |
| 17 | 55 | 2.0 | 0.0070 | -136.54 | -159.57 | **+23.04** |
| 18 | 80 | 0.4 | 0.0001 | 1151.65 | 1030.48 | **+121.16** |
| 19 | 80 | 0.4 | 0.0010 | 1133.44 | 1018.67 | **+114.77** |
| 20 | 80 | 0.4 | 0.0070 | 595.74 | 483.73 | **+112.01** |
| 21 | 80 | 1.0 | 0.0001 | 795.34 | 838.09 | **-42.75** |
| 22 | 80 | 1.0 | 0.0010 | 795.18 | 838.72 | **-43.54** |
| 23 | 80 | 1.0 | 0.0070 | 252.06 | 315.55 | **-63.49** |
| 24 | 80 | 2.0 | 0.0001 | 678.28 | 668.09 | **+10.20** |
| 25 | 80 | 2.0 | 0.0010 | 676.19 | 665.90 | **+10.29** |
| 26 | 80 | 2.0 | 0.0070 | 133.43 | 152.43 | **-19.00** |

22 of 27 cells favor `003`, several by a wide margin (up to +170.72 at cell 9); 4 favor `001`
significantly (bold negative: cells 21, 22, 23, 26); cell 14's -1.82 has a 95% CI straddling 0
(`[-5.04, 1.40]`) and is not a real effect. Every other listed sign is real
(`results/2026-08-21-grid-003-piecewise-linear.md` for the full CIs) — a materially better
fragility profile than `005-vol-adaptive-cpmm-fee`'s own 17/27 (`strategies/005-vol-adaptive-cpmm-fee/NOTES.md`).

### Explaining the negative cells

**Every significantly negative cell shares `norm_fee_bps = 80`** (the most expensive
normalizer level) **and `norm_liquidity_mult >= 1.0`** (a normalizer that is not thin) — cells
21, 22, 23 (`liq=1.0`) and 26 (`liq=2.0`). At `fee=80`, the normalizer is already expensive
enough that **both** candidate and reference win most of the flow regardless of the exact
inner price (edges of 133-1150 for both), so flow-share is not what separates them here —
what differs is **margin captured per unit of that already-won flow**. `001`'s flat `66`bps
charges the same comfortable margin on every trade; this port's fitted `S0_BPS = 56` charges
slightly less at the inner edge (the price most retail flow actually clears at, since the
router picks whichever venue is cheaper for a given slice), so against an opponent that was
never going to lose the flow-share contest anyway, being a few bps cheaper only gives away
margin rather than winning anything. This is the same shape of explanation
`005-vol-adaptive-cpmm-fee/NOTES.md`'s own `fee=80` cluster gives for its own negative cells —
independent confirmation that this pattern is a property of the harness's `norm_fee_bps=80`
regime, not specific to either family's mechanism.

Cell 26 is the worst (`-19.00`, `liq=2.0`, high sigma `0.007`): the deepest, most expensive
normalizer configuration combined with the highest sampled volatility, where `001`'s
comfortable flat margin holds up better than this port's `S0_BPS=56/W_BPS=1000` ladder, whose
outer band (bounded at its own frozen upper bound, see § Search) is relatively thinner in
proportion to a high-sigma regime's own realized moves than `001`'s uniform 66bps markup
across every size. Cells 21-23 (`liq=1.0`) show the same pattern with less noise (tighter CIs,
consistent -43 to -63).

None of this is disqualifying: `003` beats `001` on every aggregate, decision-input segment
(screening/train/validation, all +27 to +31) by a comfortable margin, and grid mode is
explicitly *not* a ranking input (docs/DESIGN.md §2.3) — it identifies where the fitted point
is fragile, which is exactly what it did here (the same `norm_fee_bps=80` weakness a `003b`
variant widening `W_BPS`'s own frozen upper bound, per § Search's boundary-hit note, would be
the natural next step to address — out of scope for this faithful port).

## Compute units (docs/DESIGN.md §2.9, cross-cutting finding #9)

No bench tooling exposes measured CU headroom directly (same gap `005-vol-adaptive-cpmm-fee`
recorded: `prop-amm validate` doesn't report it, and `crates/executor`'s remaining-CU value is
private and not surfaced — an upstream-sync-lane change, not something to add inside this
issue). Division-count estimate instead, per finding #9's own guidance that division count
(not instruction count) is what to bound on SBF: ladder construction is 5 divisions (`spot`,
`p_low`, `p_high`, `total_qty`, `k`); the buy side's worst case additionally runs
`BISECT_ITERS = 44` iterations of `cost_of_base` (2 divisions each) plus one more for
`full_cost` = 90 divisions, for **~95 divisions worst case** (buy side, partial-fill
bisection path). The sell side has no bisection at all: ladder construction (5) plus the
primary quote evaluation (2 divisions) plus, when the residual tail fires, its own 2
divisions (`residual_price`, `extra_quote`) = **~9 divisions worst case**, still an order of
magnitude below the buy side and comparable in shape to `001-cpmm-fee`'s own 2. At finding
#9's own cited worst-case cost (10^2-10^3 CU/division),
the buy side's ~95 divisions is a worst-case estimate of roughly 9,500-95,000 CU — tight but
within the 100,000 CU limit; corroborated empirically by `prop-amm validate`'s native/BPF
parity check and the parity gate below both executing the real BPF program repeatedly with no
compute-budget failure.

## Parity gate (docs/DESIGN.md §2.6)

Reproduced via `cargo run -p prop-amm-bench --release -- parity --strategy
strategies/003-piecewise-linear` against the committed `(S0_BPS=56, W_BPS=1000,
DELTA_PCT=3)` point: `prop-amm validate` passes; the fast path and the reference path agree
on **all 1,000 `observation`-segment seeds to `0` relative difference** (well inside the
`1e-9` gate); the fast-path aggregate (avg edge 432.47) matches `prop-amm run`'s own 2-decimal
output exactly (432.47, total 432468.09). See
`results/2026-08-21-parity-003-piecewise-linear.md` for the committed snapshot.

**Leaderboard-comparable number: avg edge 432.47** (`observation` segment, seeds `0..=999`,
native), against `001-cpmm-fee`'s own **399.97** on the same segment — a **+32.50 (+8.1%)**
improvement.
