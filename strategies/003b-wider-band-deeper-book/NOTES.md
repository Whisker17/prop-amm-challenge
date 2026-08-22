# 003b-wider-band-deeper-book

## Provenance

A `docs/DESIGN.md` §2.9 variant of `strategies/003-piecewise-linear` (WHI-1224), not a
fresh external port — no new `docs/references/` entry is needed; the parent's own
`docs/references/003-piecewise-linear/` remains the mechanism's ultimate source. The
issue's own thesis: the parent's `W_BPS` search converged to its own frozen upper bound
(1000) on a still-rising, non-degenerate plateau — genuine unresolved headroom. This
variant widens `W_BPS`'s own upper bound to 2500 (a ridge-collapse argument, not a round
number — see § Frozen parameter space) and replaces the parent's `DELTA_PCT` (percent
resolution) with `DELTA_RESERVE_BPS` (bps resolution, 20x finer) so the search can resolve
the parent's own ~3.25%-interior peak. Per §2.9 this is a variant, not a new strategy: same
ladder shape, same re-anchor policy, same inversion method — only the width bound and the
depth parameter's unit/resolution change.

`docs/DESIGN.md` §6.2 separately names the oracle-staleness spread-widening backoff (from
`003`'s own blog-post source material, not its confirmed on-chain mechanism) as "at most, a
`003b` variant" — a different candidate thesis than this issue's own parameter-space
widening. WHI-1224 itself is what scoped this variant to width x depth rather than that
alternative; §2.10 step 3 grants exactly one variant per top-three strategy, and that slot
is spent here, not on the staleness-backoff idea.

## Objective (WHI-1224's own framing)

`003b` is a §2.9 variant of `003-piecewise-linear`, opened per §2.10 step 3, the M1 runner-up (validation 432.45,
against `004-ewma-shock-decay-fee`'s own 446.30). Conditionally open per the issue: run five
pre-registered probes first, be prepared to close as "003 stands as measured" if the kill
rule fires. Even a success here was expected to produce a stronger #2, not a new #1 — the
issue's own priority note explicitly ranked this lane behind `WHI-1223` (`004b`), which
landed first (`c4376b5`) and is itself a negative result (its own kill rule fired, so
`004`'s parent point, 446.30 validation, remains the current M1 leader).

## Pre-registered probes (docs/DESIGN.md §2.5, run before the frozen search)

Evaluated via the scratch degenerate-range method (`bench-fit-degenerate-range-evaluates-
one-exact-point` technique: a throwaway copy of this variant's own code shape with every
PARAMS range collapsed to `MIN==MAX`, `bench fit --max-points 1 --no-report`, never
committed). All six points below use the `screening` segment (n=200, native fast-compile
path, common random numbers with the parent's own committed 412.460982), run against the
repo at commit `c4376b5` (`origin/dev` tip at the time), before
`strategies/003b-wider-band-deeper-book/lib.rs` existed as a committed file — so no
`results/*.md` report is written or possible for these points, consistent with
`bench fit --no-report`'s own design (docs/DESIGN.md §2.5/§2.9).

| Probe | Point `(S0_BPS, W_BPS, DELTA_RESERVE_BPS)` | screening avg edge | vs. parent's 412.460982 |
| --- | --- | --- | --- |
| P0 (plumbing) | (56, 1000, 300) | 412.460982 | 0.00 (exact containment) |
| P1 (direct width) | (56, 1500, 300) | 385.810080 | **-26.65** |
| P2 (direct width) | (56, 2500, 300) | 279.984769 | **-132.48** |
| P3 (interaction) | (56, 1500, 500) | 421.673537 | **+9.21** |
| P4 (interaction) | (56, 2500, 600) | 415.876187 | **+3.42** |
| P5 (resolution) | (56, 1000, 325) | 413.680340 | +1.22 |

**P0 reproduces the parent's committed screening number exactly** (`412.460982`), and its
train/validation re-evaluation also matches the parent's own committed numbers exactly
(437.559183 / 432.445900) — the `DELTA_PCT -> DELTA_RESERVE_BPS` rewrite is bit-exact at the
mapped point, confirmed empirically, not just algebraically (see § Containment).

**The direct-width thesis is dead, and not merely exhausted — it is now sharply negative.**
Widening `W_BPS` alone (holding depth fixed at the parent's own 3%) spreads the same fixed
book capacity over a much wider band: `k = total_qty * PRICE_SCALE / width` falls as width
grows, flattening the marginal price near spot and giving away margin on the realistic,
near-the-money trade sizes that make up most of the pool's flow — a materially different
outcome from the issue's own prediction of "+1 to +3 screening units" from extending `W`
alone. This is evidence, not merely reasoning: P1/P2 measure the loss directly.

**Only the width x depth interaction thesis pays** (P3, P4): pairing a wider band with
correspondingly more depth avoids the k-dilution effect above while capturing the wider
band's own survivability benefit. P3 (`W=1500, delta=500`) is the strongest of the five
non-plumbing probes (P1-P5) at +9.21 screening. The resolution-only thesis (P5, finer
`DELTA_RESERVE_BPS` quantization at the parent's own `W=1000`) is real but small (+1.22),
consistent with the issue's own predicted "+1.6 screening units" from resolution alone.

**Kill rule** (issue's own threshold): if `max(P1..P5) < 417`, do not open the variant.
`max(P1..P5) = max(385.81, 279.98, 421.67, 415.88, 413.68) = 421.673537` (P3) — **the hard
kill rule does not fire.**

**Two-tier open rule** (issue's own threshold): 417-425 requires explicit owner approval,
since it "cannot change the winner" (004's own parent point, 446.30 validation, already
exceeds P3's own train/validation re-evaluation of 445.93/439.99, itself only a single
pinned point's out-of-sample check, not the eventual search optimum). 421.67 falls in this
band. **The owner was asked explicitly and approved proceeding** with the full 300-point
search, given the parent's own frozen `W_BPS` bound was a genuine, non-degenerate rising
plateau rather than a converged interior optimum — i.e. the upside case (a materially
stronger #2, informative for any future v0.2.0 work on this family) was judged worth the
search cost even though it cannot change the v1 ranking. This probe table and decision are
also recorded as a WHI-1224 issue comment, so the gate is auditable independently of this
file.

## Pre-search shape-fuzz gate (docs/DESIGN.md §2.9, cross-cutting finding #8)

Per the issue's own gate order ("`bench fuzz` at the parent point and at scratch copies
pinned to the new box's corners... before the search spends budget — this family is the one
that most needs it"), run against this variant's own code shape (the `DELTA_RESERVE_BPS`
rewrite) at five points, all **PASS — zero shape violations** across 324 states x 2 sides
each:

| Point | `(S0_BPS, W_BPS, DELTA_RESERVE_BPS)` | Result |
| --- | --- | --- |
| Parent-mapped (P0) | (56, 1000, 300) | PASS |
| Corner A | (5, 2500, 50) | PASS |
| Corner B | (200, 2500, 1000) | PASS |
| Corner C | (5, 50, 1000) | PASS |
| Corner D | (200, 50, 50) | PASS |

A PASS writes no report (the gate is meant to run repeatedly, before every search, by
design — see `tools/bench/src/commands/fuzz.rs`'s own module doc comment).

## Fidelity self-assessment (docs/DESIGN.md §2.9)

**Minimum changes only**, relative to the parent — every mechanism property the parent's own
`NOTES.md` argues for (re-anchor policy, shape safety, cold start / garbage state, exact
inversion, base-denominated book capacity, the residual tail beyond exhaustion) carries over
unchanged, since none of it depends on `W_BPS`'s magnitude or on `DELTA_PCT` vs.
`DELTA_RESERVE_BPS`'s unit. The two changes are exactly the ones the issue declared:

- `W_BPS`'s frozen upper bound widens from 1000 to 2500 (a ridge-collapse argument — see the
  module doc comment and § Frozen parameter space below — not a search-driven widening
  after the fact).
- `finish_ladder`'s `rx.saturating_mul(DELTA_PCT) / 100` becomes
  `rx.saturating_mul(DELTA_RESERVE_BPS) / 10_000` — one division, as before, at 20x finer
  resolution (1 bp vs. 1 percentage point).

`MODEL_USED` is preserved as `"Claude Sonnet 5"` — unlike `004b`'s own departure from its
parent's constant, this variant's mechanism is not newly authored (the ladder math, re-anchor
policy, and inversion method are all inherited verbatim), so naming the same model as the
parent port is the accurate provenance statement here, not a stylistic default.

## Frozen parameter space (declared before the real search ran — docs/DESIGN.md §2.4)

| Param | Range | Reason |
| --- | --- | --- |
| `S0_BPS` | 5..=200 (**unchanged**) | no candidate change touches the near-spot price axis; the parent's own interior optimum (56) and this variant's own vicinity (57) both sit well inside it. |
| `W_BPS` | 50..=**2500** (widened from the parent's 1000) | the parent's fit converged to its own frozen upper bound on a still-rising, non-degenerate plateau (`[56,941,3] -> 411.80`, `[56,986,3] -> 412.33`, `[56,1000,3] -> 412.46`) — genuine unresolved headroom. 2500 is a ridge-collapse bound: this port's 6 segments collapse to one linear-density span, so once `W` exceeds the largest inter-anchor mispricing the simulation can produce, `(W, DELTA_RESERVE_BPS)` enter the economics only through their ratio and further widening is pure reparameterization. 2500 bps covers a 3.5-sigma, ~100-step arb-free stretch at `gbm_sigma_max=0.007` — a gap that cannot persist un-arbitraged given the arbitrageur re-anchors at >=1 cent profit. **The 100-step/3.5-sigma choice is judgment, not measurement** — committed evidence (this variant's own search) only reaches the fitted `W_BPS=2423`, itself interior to the bound, not resting on it. |
| `DELTA_RESERVE_BPS` | 50..=1000 bps (0.5%-10%) | the UPPER bound (1000 bps = 10%) is exactly the parent's own corrected `DELTA_PCT` ceiling, just re-expressed at 20x finer resolution (the parent's own train sweep measured -4600 at 10% even behind `W=1000`). The LOWER bound (50 bps = 0.5%) is **not** the same as the parent's own `1%` floor — it is a genuine, small widening downward, declared in this issue rather than independently re-derived here. The parent's own flow-share-collapse trend (0.281 at 1% and falling) was measured only down to 1%; 0.5%'s own viability extrapolates that trend's direction rather than resting on a point the parent itself measured. |

**Frozen, recorded as deliberately un-searched (unchanged from the parent):**
`PRICE_SCALE`, `BPS_DENOM`, `NUM_PRICE_POINTS`/`NUM_SEGMENTS`, `MIN_GAP_BPS`,
`RESIDUAL_PRICE_MULT`, `BISECT_ITERS` — none of this variant's two changes touch any of
these constants' own justifications.

## Search (docs/DESIGN.md §2.5)

Run via `cargo run -p prop-amm-bench --release -- fit --strategy
strategies/003b-wider-band-deeper-book` (`config/bench.toml`'s `[search]` budget, 300
points; screening segment `1_000_000..=1_000_199`, common random numbers). Coarse grid
(3-dimensional, over the space above) then coordinate descent. Full curve committed at
`results/2026-08-22-fit-003b-wider-band-deeper-book.md`.

**Converged after 176 of 300 points** (budget never exhausted); **zero invalid points** —
every evaluated parameter vector produced a valid edge (no panic), i.e. no point tripped
`docs/DESIGN.md` §2.9's shape checks or WHI-1213's `Invalid` handling anywhere in the 176
points visited. **This is a distinct claim from "shape-safe" or "not catastrophic" — the
evaluated curve does contain a large, expected cliff region** (e.g. `[5,50,525] -> -20043`,
`[103,50,525] -> -20077`, exactly 23 of the 176 points land in the -19,000 to -20,077 range
— the same thin-book-behind-a-narrow-effective-band failure mode `003`'s own parent
NOTES.md § DELTA_PCT range correction documents): a *valid* (non-panicking) edge, just an
economically bad one, exactly as `results/2026-08-22-fit-003b-wider-band-deeper-book.md`'s
own committed curve shows. The actual shape-safety evidence is the `bench fuzz` PASSes
above, run specifically at the box's four corners (including the thinnest-`k` corner,
Corner A — see § Shape and CU risk below for the computed comparison) and the fitted
point — not this "zero invalid" count, which only rules out panics.

**Winning point: `S0_BPS = 57, W_BPS = 2423, DELTA_RESERVE_BPS = 763`.**

| Segment | n | Avg edge |
| --- | --- | --- |
| screening (search inner loop) | 200 | 426.184965 |
| train (final evaluation) | 1,000 | 452.490208 |
| validation (final evaluation) | 1,000 | 447.224660 |

Against the parent's own committed numbers (screening 412.46, train 437.56, validation
432.45): **+13.72 screening, +14.93 train, +14.78 validation** — a materially stronger #2
than either the issue's own predicted band (435-450, central ~439-441) undersold on the low
end or the probes alone suggested (the strongest probe, P3, reached only +9.21 screening).

**All three searched parameters converged to interior points** — `S0_BPS=57` (of `5..=200`),
`W_BPS=2423` (of `50..=2500`, **not** resting on the widened upper bound), `DELTA_RESERVE_BPS
=763` (of `50..=1000`). No boundary hits to flag this time, unlike the parent's own `W_BPS`
convergence to 1000. This closes the parent's own open question: the true `W_BPS` optimum
was not at infinity, it was at ~2423 bps (~24.2%) once paired with proportionally more depth
— confirming the issue's own single falsifiable thesis (the width x depth interaction, not
either dimension alone).

**Compile timing:** 178 warm compiles (176 search-phase points plus the two final
train/validation builds — the figure `results/2026-08-22-fit-003b-wider-band-deeper-book.md`
itself reports), min=0.385s, mean=0.750s, max=4.184s — exceeds
docs/DESIGN.md §2.6's `<1s` target on the max sample, consistent with the same session-load
explanation `001`/`003`/`005`'s own `NOTES.md` files already document as a standing,
previously-investigated non-issue with the fast path's own mechanism (concurrent build
activity from another session's worktree was active throughout this run — see this issue's
own git-workflow note on a concurrent `WHI-1225` worktree observed during implementation).

## A significant, unpredicted result — flagged prominently, not resolved here

**This variant's validation number (447.224660) is +0.92 above `004`'s own committed
validation (446.30), and its `observation`-segment leaderboard-comparable number
(446.60, see § Parity gate) is +2.85 above `004`'s own committed `observation` number
(443.75, `strategies/004-ewma-shock-decay-fee/NOTES.md` — `004b`'s own kill rule fired, so
`004`'s parent point still stands as the current M1 leader).** Both margins sit inside this
family's own stated noise floor (+/-1 on validation, +/-2-3 on screening at n=200) — this is
NOT a statistically resolved result — but the direction is consistent across all three
independently-sampled held-out segments this issue measured (train, validation, and
`observation`), which a single noisy draw would not be expected to produce.

**This issue's own Prediction section explicitly rated this outcome unlikely** ("the
probability of clearing 446.30 is well under half — the likeliest outcome is a somewhat
stronger runner-up that does not change the winner"). The measured result contradicts that
expectation's confidence, though not necessarily its substance — the margin is small enough
that it may not survive the single-use test-segment comparison docs/DESIGN.md §2.10 step 4
reserves for exactly this kind of top-two question.

**This is explicitly out of scope for WHI-1224 to resolve.** §2.10 step 3 ("rank on
validation") and step 4 ("use the test segment once: winner and runner-up, paired interval,
regime slices") describe a deliberate, later act governing the whole M1 ranking, not
something one strategy's own fitting issue can trigger unilaterally — doing so here would
spend the test segment on a comparison this issue was never scoped to make, and would
revise `docs/DESIGN.md`'s own ranking outside the process that owns it. This `NOTES.md`
records the measured numbers plainly; whether `docs/DESIGN.md` §6.2/§2.10 should be updated
to reflect a new provisional #1, and whether the test segment should now be spent on a
`003b`-vs-`004` comparison, is a decision for the issue owner, flagged here and in the PR
description rather than acted on.

## Containment — bit-exact (docs/DESIGN.md §2.9, cross-cutting finding #10's substitute)

The parent's fitted point maps to `(56, 1000, 300)` under this variant's own code (P0,
above), and is bit-exact: `floor(rx*300/10000) = floor(rx*3/100)` for all `rx`, because a
truncating division's floor is invariant under common scaling of numerator and denominator
by the same integer (100) — the same identity `001`'s own fee rewrite and `003`'s own
parent-point containment relied on. **Measured residual gap: zero** — P0's screening
(412.460982) and its train/validation re-evaluation (437.559183 / 432.445900) match the
parent's own committed numbers to the last printed digit.

This bounds the downside at a tie with `003`'s own **432.45** validation, not merely with
the 0-line. Cross-cutting finding #10's own `001@66` containment remains structurally
unsatisfiable for this family (the curve forms do not intersect at any parameter point,
`003-piecewise-linear/NOTES.md` § Cross-cutting finding #10) — inherited here verbatim,
since neither of this variant's two changes alters the curve family, only its width/depth
parameterization.

## Shape and CU risk — re-verified at the fitted point (docs/DESIGN.md §2.9)

Every expression the issue flagged for re-verification, checked against the actual fitted
point (`S0_BPS=57, W_BPS=2423, DELTA_RESERVE_BPS=763`), not just the box corners:

- **`build_bid_ladder`'s `BPS_DENOM - outer_bps` clamp:** `effective_outer_bps()` at the
  fitted point is `max(2423, 57+6)=2423`, comfortably under `BPS_DENOM-1=9999` — the clamp
  is unreachable at the fitted point itself, as at the parent, but the fuzz gate (pinned to
  Corner A/B at `W_BPS=2500`, the frozen ceiling) confirms it stays safe at the widened
  bound's own edge, not just at this interior winner.
- **`finish_ladder`'s `k` guard:** at the fitted point, `k = 322,485,207` (computed directly,
  see § Validate buy-side check below) — far from the `k==0` degenerate case even at this
  variant's much wider `width` than the parent's. Computed directly at `validate.rs`'s own
  default state, the thinnest-`k` corner in the declared box is actually **Corner A**
  (`S0=5, W=2500, DELTA=50`, `k=20,040,080` — the largest width paired with the smallest
  depth), not Corner C (`S0=5, W=50, DELTA=1000`, `k=22,222,222,222`, the LARGEST `k` of the
  four corners, since it pairs the smallest width with the largest depth). The `bench fuzz`
  PASS at Corner A is what confirms the guard itself, not the fitted point, since the fitted
  point is comfortably interior.
- **`cost_of_base`'s term2 `base^2/(2k)`:** at the fitted point, `base <= total_qty ~=
  7.63e9` nano and `k ~= 3.2e8`, well inside the squaring bound
  (`(1.8e18)^2 ~ 3.4e36 << u128::MAX`) that carries over unchanged from the parent — the
  delta cap (10% of `reserve_x`) is unchanged in percentage terms, only re-expressed in bps.
- **CU:** unchanged — the `/100 -> /10_000` swap is still one division, `BISECT_ITERS=44`
  unchanged, worst case ~95 divisions against the 100k cap, same tightness as the parent
  (docs/DESIGN.md §2.9 cross-cutting finding #9).

## `validate`'s buy-side exhaustion-inside-the-probe-window case — checked explicitly

The issue flagged a new regime risk: at small `DELTA_RESERVE_BPS`, the buy side's full-book
cost could fall inside `validate.rs`'s own fixed 200-quote-token probe window, unlike the
parent (whose full-book cost, ~316 quote tokens at its own committed point, stays just
outside it). **At the actual fitted point this concern does not materialize** — the search
converged to `DELTA_RESERVE_BPS=763` (7.63%), more than double the parent's own 3%, not a
small value. Computed directly at `validate.rs`'s own default state (`reserve_x=100,
reserve_y=10000`, spot=100): `p_low=100.57`, `p_high=124.23`, `total_qty=7.63` base tokens,
`k=322,485,207`, **`full_cost = 857.61` quote tokens** — well past the 200-token probe
ceiling, so the buy side never reaches the residual tail within `validate`'s own synthetic
probe at this point, the same regime the parent was in. `prop-amm validate`'s own PASS
(below) confirms this empirically, not just algebraically: strict monotonicity holds across
all 10 fixed probe sizes on both sides.

**The small-`DELTA_RESERVE_BPS` regime the issue actually asked about is real, and was
checked directly, not inferred from the fitted point alone.** At Corner A (`S0=5, W=2500,
DELTA=50`, the box's own smallest-depth/widest-width corner), the same computation gives
`p_low=100.05`, `p_high=125.00`, `total_qty=0.5` base tokens, `k=20,040,080`, **`full_cost =
56.26` quote tokens** — comfortably *inside* the 200-token probe window, unlike the fitted
point. `prop-amm validate` was run directly against a scratch copy pinned to Corner A and
**passes cleanly**, including strict buy-side monotonicity/concavity across the full probe
and native/BPF parity — confirming the new regime is real within the frozen space (the
concern the issue raised was not a false alarm) but does not compromise correctness, and
does not affect the committed strategy either way, since the search converged to a point far
outside that regime.

## Compute units (docs/DESIGN.md §2.9, cross-cutting finding #9)

Unchanged from the parent's own estimate: ladder construction is 5 divisions; the buy side's
worst case additionally runs `BISECT_ITERS=44` iterations of `cost_of_base` (2 divisions
each) plus one more for `full_cost` = 90 divisions, for **~95 divisions worst case**. The
sell side has no bisection: **~9 divisions worst case**. At finding #9's own cited
worst-case cost (10^2-10^3 CU/division), the buy side's ~95 divisions is a worst-case
estimate of roughly 9,500-95,000 CU — tight but within the 100,000 CU limit, corroborated
empirically by `prop-amm validate`'s own native/BPF parity check (12 sims, 2000 steps) and
the parity gate below (1000 sims, 10,000 steps), both executing the real BPF program
repeatedly with no compute-budget failure.

## `prop-amm validate` (docs/DESIGN.md §2.6)

Run from a detached scratch worktree at commit `b5e374f` (per the nested-worktree cargo
isolation workaround this repo's own tooling notes require for the real BPF compile path —
`crates/cli/src/commands/compile.rs`'s `ensure_build_dir` lacks the `[workspace]`-table fix
`tools/bench`'s fast path already carries):

```
[PASS] ELF loaded and verified
[PASS] Buy X: input_y=10.0 -> output_x=0.099281
[PASS] Sell X: input_x=1.0 -> output_y=97.879541
[PASS] Buy side monotonicity
[PASS] Sell side monotonicity
[PASS] Buy side concavity
[PASS] Sell side concavity
[PASS] Randomized reserve/storage checks
[PASS] Native/BPF parity (12 sims, 2000 steps): delta=0.000000000, tol=0.000001000
All validation checks passed!
```

## Parity gate (docs/DESIGN.md §2.6)

Reproduced via `cargo run -p prop-amm-bench --release -- parity --strategy
strategies/003b-wider-band-deeper-book` against the committed `(S0_BPS=57, W_BPS=2423,
DELTA_RESERVE_BPS=763)` point, from the same detached scratch worktree: `prop-amm validate`
passes; the fast path and the reference path agree on **all 1,000 `observation`-segment
seeds to `0` relative difference** (well inside the `1e-9` gate); the fast-path aggregate
(avg edge 446.60) matches `prop-amm run`'s own 2-decimal output exactly (446.60). See
`results/2026-08-22-parity-003b-wider-band-deeper-book.md` for the committed snapshot.

**Leaderboard-comparable number: avg edge 446.60** (`observation` segment, seeds `0..=999`,
native) — against `003-piecewise-linear`'s own **432.47** (+14.13, +3.3%) and `001-cpmm-
fee`'s own **399.97** (+46.63, +11.7%). Also, per § A significant, unpredicted result above,
+2.85 above `004`'s own committed **443.75** on the same segment — flagged there, not
treated as a ranking change here.

## Grid mode: 27-cell fragility matrix (docs/DESIGN.md §2.3)

`cargo run -p prop-amm-bench --release -- grid --candidate
strategies/003b-wider-band-deeper-book/lib.rs --reference strategies/001-cpmm-fee/lib.rs` —
reference is `001-cpmm-fee` (docs/DESIGN.md §2.8), matching the parent's own comparison.
Full table committed at `results/2026-08-22-grid-003b-wider-band-deeper-book.md`.

23 of 27 cells favor `003b` (up from the parent's 22/27 — see below), several by a wide
margin (up to +216.27 at cell 9); 4 favor `001` significantly (bold negative below): cells
21, 22, 23, 26 — **exactly the same four cells the parent's own grid flagged**, matching the
issue's own prediction ("cells 21 and 22 stay negative... no candidate change touches the
near-spot price").

| cell | fee (bps) | liq mult | sigma | 003b | 001 (reference) | mean diff | vs. parent's own diff |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 14 | 55 | 1.0 | 0.0070 | 46.50 | 32.09 | **+14.41** | parent: -1.82 (CI straddled 0, not real) |
| 21 | 80 | 1.0 | 0.0001 | 807.79 | 838.09 | **-30.30** | parent: **-42.75** |
| 22 | 80 | 1.0 | 0.0010 | 808.42 | 838.72 | **-30.30** | parent: **-43.54** |
| 23 | 80 | 1.0 | 0.0070 | 271.51 | 315.55 | **-44.04** | parent: **-63.49** |
| 26 | 80 | 2.0 | 0.0070 | 138.78 | 152.43 | **-13.65** | parent: **-19.00** |

**Cells 21/22/23/26 are predicted-unchanged in sign, not as a variant failure — and this is
a bonus, not merely a wash:** every one of the four negative cells is materially *less*
negative than the parent's own (e.g. cell 23: -44.04 vs. the parent's -63.49), and cell 14 —
ambiguous for the parent (95% CI `[-5.04, 1.40]` straddling 0) — is now a real, measured
positive effect (CI `[8.47, 20.34]`). The explanation is the same shared mechanism the
parent's own `NOTES.md` gives: every negative cell shares `norm_fee_bps=80` (the most
expensive normalizer level) and `norm_liquidity_mult>=1.0` — at that fee level both venues
win most of the flow regardless of price, so a narrower spread only gives away margin on
flow that was never contested. This variant's wider, deeper book charges a *relatively*
higher effective rate near spot than the parent's thinner one did, which is why the same
structurally-disadvantaged cells lose less here than they did for the parent — a direct,
mechanical consequence of the width x depth interaction this variant's own thesis is built
on, not a separate finding.

None of this is disqualifying: `003b` beats `001` on every aggregate, decision-input segment
(screening/train/validation, all +13 to +15 over the parent's own already-comfortable
margin) by a wide margin, and grid mode is explicitly *not* a ranking input (docs/DESIGN.md
§2.3).

## Transferability caution (inherited from the parent, sharpened)

The parent's own `NOTES.md` already flags that this family's viability window is an
artifact of this harness's arb-as-oracle dynamics, with no live-oracle protection a real
deployment would have. **This variant sharpens that caution rather than resolving it**: the
fitted `W_BPS=2423` (~24.2%) and `DELTA_RESERVE_BPS=763` (7.63%) are further from the
source's own original deployment assumptions than the parent's own `(1000 bps, 3%)` was,
and the upper bound this variant searched to (`W_BPS<=2500`) was itself derived from this
harness's own specific volatility sampling (`gbm_sigma_max=0.007`), not from any general
deployment principle — a second round of range surgery tuning more tightly to this
simulator's specific regime distribution, exactly as the parent's own caution warned a
`003b` variant would do. Since the grader *is* this simulator with these parameters, that is
literally the objective of this issue — but this family's edge remains the least
transferable in the portfolio if upstream ever resamples its parameter distribution, now
more so than before this variant, not less.

## Verification (docs/DESIGN.md §4.4 acceptance criterion, `AGENTS.md`)

- `cargo test --workspace`: green (`strategies/` is excluded from the workspace, so this
  confirms no regression elsewhere, not this file's own tests).
- This file's own `#[cfg(test)] mod tests` (10 tests, same set as the parent, only
  `get_name_and_model_are_nonempty`'s expected `NAME` string differs): run manually via
  `cd .build/fast && cargo test --release` after copying this file's own fitted-point source
  into that scratch build directory (`003-piecewise-linear/NOTES.md`'s own documented
  convention — these tests are not compiled or run by `cargo test --workspace`, nor by
  either compile path, per `strategies/`' exclusion from the workspace). All 10 pass.
- `bench fuzz`: PASS at the parent-mapped point and all four declared box corners (five runs,
  § Pre-search shape-fuzz gate, before the search spent any budget), plus a sixth PASS at the
  actual fitted point after the search converged (this section).
- `cargo fmt`/`clippy` on this file only, per `AGENTS.md`'s merge-gate caveat (not the
  inherited upstream failures logged in `docs/DEFERRED_ISSUES.md`).

## Note on the environmental build workaround used while producing this record

`bench parity`, `bench grid`, and `prop-amm validate` all route through
`crates/cli/src/commands/compile.rs`'s `ensure_build_dir`, which lacks the `[workspace]`-
table fix `tools/bench/src/fast_compile.rs` already carries — running them directly inside
this issue's own `.claude/worktrees/whi-1224-003b` checkout fails with "current package
believes it's in a workspace when it's not" (the primary clone's root `Cargo.toml` is
reached by cargo's ancestor walk before the nested worktree's own build dir is). Worked
around per this repo's own documented pattern: a detached, throwaway scratch worktree at
`/tmp/whi-1224-scratch-worktree` (created via plain `git worktree add --detach <sha>`, not
`EnterWorktree`), pinned to this branch's own commit `b5e374f`, with the resulting
`results/*.md` reports copied back into the real worktree before the scratch one was
removed. `bench fit`/`bench fuzz` (fast path) ran directly inside the real worktree with no
workaround needed.

