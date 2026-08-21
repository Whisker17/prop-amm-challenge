# 005-vol-adaptive-cpmm-fee

## Provenance

Source form: **source (Rust), direct submission shape** — the highest-fidelity source in the
frozen list (`docs/DESIGN.md` §6.2). `docs/references/005-vol-adaptive-cpmm-fee/EdgeMax_CumVar.rs`
is `dcccrypto/percolator-perp-liquidity`'s own `EdgeMax_CumVar.rs`, pinned at commit
`0c7170244d695a681f3a4393e20079dfb7dc137b` (the parent of the commit that deleted it as
"unrelated" to that repo's later focus) — the pinned copy is the only surviving source; see
`docs/references/005-vol-adaptive-cpmm-fee/README.md` for the full chain of custody. Apache-2.0
licensed at that commit.

The source is already written against `pinocchio`/`prop_amm_submission_sdk` in this
challenge's exact single-file shape, and its own header comment reasons in this challenge's
own terms (LVR vs. retail-spread capture, per-simulation stationary `gbm_sigma`, native/BPF
parity) — it was written for this exact simulator, not adapted from an unrelated one.

## Fidelity self-assessment (docs/DESIGN.md §2.9)

**Minimum changes only**, per the fidelity contract:

- `compute_swap`, `fee_from_state`, `cp_out`, `after_swap`, `isqrt`, and every byte-offset
  helper are **byte-for-byte identical** to the source. No mechanism change.
- The storage byte layout (`OFF_MAGIC`/`OFF_LAST_STEP`/`OFF_LAST_PRICE`/`OFF_VAR_SUM`/
  `OFF_SAMPLE_COUNT`, `MAGIC` sentinel, 56 of 1024 bytes used) is unchanged — verified against
  `crates/shared/src/instruction.rs`'s current ABI (offsets 18/26/34 for post-trade
  `reserve_x`/`reserve_y`/`step` in the `after_swap` payload; offsets 1/9/17/25 for
  `input_amount`/`reserve_x`/`reserve_y`/`storage` in the `compute_swap` instruction) and they
  line up exactly. The issue's own "one open question" on layout (written before this
  amendment) is stale — this port required **no ABI adaptation**, only wiring.
- What changed, and why each change is within the fidelity contract's bounds:
  - `NAME` — required by the submission interface (§2.9's explicit carve-out).
  - `MODEL_USED` — preserved as `"Claude Opus 4.8"` (the source's own constant): the mechanism
    is untouched, so the field describing which model produced it hasn't changed (same
    reasoning `strategies/001-cpmm-fee/NOTES.md` uses for its own preserved `MODEL_USED`).
  - `FEE_LO`, `A_NUM`, `B_DEN` moved into a `// === PARAMS BEGIN/END ===` block with `// range:`
    comments — mechanically required for `bench fit` to search them at all
    (`tools/bench/src/params.rs`); the *values* inside the block are unchanged from the
    source at commit time (`20`, `7`, `160`) until the search below overwrites them with the
    fitted point. `A_DEN` stays a plain frozen `const` (not searched — see below).
  - `R2_CAP` rewritten from a literal `62_500` to `MOVE_BPS_CAP * MOVE_BPS_CAP` — same value,
    now derived instead of duplicated (the review amendment's explicit instruction: "not an
    independent parameter... searching one without the other breaks the source's own
    invariant"). The source declares `R2_CAP` before `MOVE_BPS_CAP`; this port swaps that
    order so the derived constant textually follows the constant it derives from (Rust does
    not require this — top-level `const`s resolve regardless of declaration order — it's
    purely for a reader), and carries a new 3-line comment stating the derivation and citing
    the issue's own review amendment (not a `docs/DESIGN.md` rule).
  - A one-sentence softening added to the header comment's sampling-point claim (the search
    only "typically" lands on the arbitrageur's correction, not always — see § Estimator bias
    below) and a pointer to that section. No code changed.
  - The four frozen consts below the `PARAMS` block (`FEE_HI`, `A_DEN`, `COLD_FEE`,
    `WARMUP_STEPS`) were reordered to sit together immediately after the block for
    readability, and each gained a trailing note: `FEE_HI`/`A_DEN` say `— frozen, not
    searched`; `COLD_FEE`/`WARMUP_STEPS` keep their own pre-existing descriptive comments
    (which already stated their role) and add only `— frozen`. Cosmetic; no value changed.

No other *code* line differs — every comment/reorder change above is named and none of them
touch `compute_swap`/`fee_from_state`/`cp_out`/`after_swap`/`isqrt` or any byte offset. This
is as close to a zero-fidelity-risk port as the frozen list gets.

## Shape-safety rule (docs/DESIGN.md §2.9, cross-cutting review finding #3)

Inside `compute_swap`, the fee — and every other curve parameter — is a function of **storage
bytes and compile-time constants only, never of `input_amount`**. `fee_from_state` takes
`_rx`/`_ry`/`storage` but not `input`; `cp_out` takes `fee_bps` as an already-computed value.
This holds unchanged from the source. `step` is not available in `compute_swap` (only in the
`after_swap` payload, offset 34) and nothing here reads it from the wrong place.

## Cold start and garbage-state handling (findings #4, #5)

- **Cold start.** Storage is zero-initialized at the start of every simulation. A
  zero-initialized `storage[OFF_MAGIC..OFF_MAGIC+8]` reads as `0`, which never equals `MAGIC`
  (`0x4544_4745_4D41_5832`), so `fee_from_state` falls back to `COLD_FEE` before the router or
  arbitrageur ever fires a real trade — no separate sentinel logic needed beyond the one the
  source already has.
- **Garbage state.** `crates/cli/src/commands/validate.rs`'s randomized probe fills
  `storage[0..32]` (including the 8-byte magic) with pseudo-random bytes. The magic check is
  the sanitization: an 8-byte random value has a `2^-64` chance of colliding with `MAGIC`, so
  every randomized-probe iteration falls back to `COLD_FEE` without ever reading `var_sum` or
  `sample_count` as arbitrary large integers — confirmed by `prop-amm validate`'s "Randomized
  reserve/storage checks" passing with no arithmetic error (see § Parity gate below, which
  records the full `prop-amm validate` PASS).

## Every error path saturates, never returns a spurious 0 (finding #6)

`cp_out`'s output is `reserve.saturating_sub(ceil(k/n))`, which is bounded above by `reserve`
by construction (the same form `001-cpmm-fee` and the source both use) — it can never *exceed*
the reserve passed in, so it never trips `crates/sim/src/amm.rs`'s "quote > reserve -> 0.0"
guard. For a huge `input` (bracketing toward `MAX_INPUT_AMOUNT ~= 1.8e19` nano), `net` grows
without bound and `ceil(k/n) -> 0`, so the output *saturates toward the full reserve*
(exhaustion), never collapses to `0`.

`net == 0` is *not* limited to `input == 0`: `net = input * (10_000 - fee_bps) / 10_000`
also floors to `0` for `input == 1` (a single nano unit) at *any* fee this family can produce
(`fee_bps` ranges `[FEE_LO, FEE_HI] = [5, 130]`, and `9,870..9,995 / 10,000` all floor to `0`
for `input == 1`) — a genuine truncation-to-zero on a *positive* input, not just the trivial
`input == 0` no-op. This is still safe, for the same reason the review's concern doesn't
apply: `input == 1` is always the *smallest* nonzero input in any shape-check sample set, so
its `output == 0` can only sit below a larger input's output (monotonically
consistent, `0 <= anything`), never above one. The failure mode the review warns about is a
*large* input collapsing to `0` and reading as a monotonicity violation against a smaller
input's positive output — that path is closed by the saturate-toward-`reserve` behavior above,
independent of this small-input truncation.

## Clamping before squaring (finding #7)

`after_swap` clamps `move_bps` to `MOVE_BPS_CAP` (250) **before** squaring it into `r2`, so `r2`
is bounded by `R2_CAP = MOVE_BPS_CAP^2 = 62,500` regardless of how extreme `diff`/`last_price`
are — no `saturating_mul` result is ever squared unclamped. `cp_out` has no quadratic
inversion to worry about (it's a direct constant-product output via ceiling division, the same
form as `001-cpmm-fee`), so finding #7's inversion-form guidance (`x = 2y / (sqrt(...) + p)`)
does not apply here — there is no quadratic to invert in the swap output path.

## Pre-search shape-fuzz gate (docs/DESIGN.md §2.9, WHI-1212)

`bench fuzz --strategy strategies/005-vol-adaptive-cpmm-fee`: **PASS — zero shape violations**
across 324 states x 2 sides (dense sweeps + golden-section fair-price sample sets, every
`[grid]` regime corner in both a zeroed- and random-byte-storage variant, plus states reached
only after a full-length GBM drift). No report committed (a PASS writes none, by design).

## Frozen parameter space (declared before any search runs — docs/DESIGN.md §2.4)

**Search 3 dimensions**, per the issue's review amendment (which supersedes the issue's
original eight-constant seed list) — **with one correction against a conflict this port
found in the frozen space itself, resolved before freezing** (see the callout below):

| Param | Range | Reason |
| --- | --- | --- |
| `FEE_LO` | **5..=66** (widened from the issue's `5..=40` — see callout) | Lower bound 5 lets the search counteract the `sigma_hat` inflation bias (§ Estimator bias below) — the low-vol conditional optimum may need an effective fee near 20 despite an inflated estimate; not 1, because a near-zero fee reproduces the normalizer-as-submission failure (`strategies/000-normalizer/lib.rs`'s own 30bps run: 60.6% flow share but only 0.000989 edge/unit-volume against the starter's 0.017422 at 500bps — ~17.6x less edge per unit of flow captured, `results/2026-08-20-l1.md`). Upper bound 66 is `001-cpmm-fee`'s own fitted fee — see callout for why this replaces the issue's original 40. |
| `A_NUM` (with `A_DEN = 10` frozen) | 0..=25 | 0 admits a pure-quadratic response; 25 reaches `FEE_HI` by `sigma_hat ~ 44`. Source anchor is the point `7` only. |
| `B_DEN` | 40..=2000 | 40 = strong quadratic (+122bps at `sigma_hat=70`); 2000 = nearly linear (+2bps at `sigma_hat=70`). Contains "drop the quadratic" as an interior region, so no separate reduced family is needed. Source anchor: `160`. |

**Frozen, recorded as deliberately un-searched** (issue's own reasoning, unchanged):
`FEE_HI = 130`; `COLD_FEE = 55` and `WARMUP_STEPS = 16` (govern <~0.5% of 10,000 steps —
unmeasurable at 200 screening seeds, searching them would be pure budget waste); `MOVE_BPS_CAP
= 250`; `P_SCALE`; `FEE_HARD_MAX`; `A_DEN = 10`. `R2_CAP` is not an independent parameter — it
is derived as `MOVE_BPS_CAP^2` in the source itself (see § Fidelity self-assessment).

### Callout — a conflict in the issue's own frozen space, resolved before any search ran

The issue's cross-cutting review (finding #10) makes containing and demonstrating the `001@66`
0-line a **new acceptance item** for every M1 strategy: "freeze the space so some point in it
is arithmetically equivalent to `001@66`... evaluate that nested point on the 200 screening
seeds before the search runs." But this strategy's own per-strategy amendment froze `FEE_LO`
at `5..=40` — and a flat 66bps fee requires `FEE_LO = 66` exactly (with `A_NUM = 0` and `B_DEN`
large enough that the quadratic term rounds to 0), which `40` cannot reach. The two review
passes conflict with each other; flagged to the issue owner before freezing anything
(2026-08-21) rather than resolved unilaterally.

**Resolution (owner-approved): widen `FEE_LO`'s max to 66.** `A_NUM`/`B_DEN` keep the
amendment's original bounds. At `(FEE_LO=66, A_NUM=0, B_DEN=2000)` the quadratic term
`sigma_hat^2/2000` still floors to `0` for every `sigma_hat <= 44` (`44^2 = 1936 < 2000`), so
the point is *exactly* flat-66 across the estimator's realistic operating range, but not
*globally* exact: at the theoretical worst case `sigma_hat = 250` (the hard cap `R2_CAP`
allows), `quad = 250^2/2000 = 31`, and `COLD_FEE = 55` (frozen, independent of this space)
still governs the first `WARMUP_STEPS = 16` steps of every simulation regardless of `FEE_LO`.
So this widening buys **near-exact, not bit-exact**, containment — see the demonstration
below for the measured gap.

### Demonstration (docs/DESIGN.md §2.5/§2.9, cross-cutting finding #10)

Evaluated `(FEE_LO=66, A_NUM=0, B_DEN=2000)` on the 200 screening seeds via a scratch
degenerate-range copy of this file (frozen range collapsed to the single point, so `bench fit
--max-points 1 --no-report` evaluates exactly it — not committed, not part of the real search):

**screening avg edge: 385.973322**, against `001-cpmm-fee`'s own committed **384.82**
(`strategies/001-cpmm-fee/NOTES.md`) — a **+1.15 (0.3%) gap**, not "far off". Train/validation
re-evaluation of the same point: train 407.71 vs. `001`'s 406.14; validation 403.39 vs.
`001`'s 401.80 — consistently ~+1.5 across all three segments, the same direction and
magnitude everywhere. This confirms the state handling and sign mapping are correct (a broken
estimator would not reproduce `001`'s curve this closely across three independently-sampled
segments); the small, consistent gap is explained, not mysterious — it is exactly what
`COLD_FEE = 55` during the first `WARMUP_STEPS = 16` steps of every simulation (frozen,
independent of `FEE_LO`) would predict: a strategy that's *slightly* cheaper than flat-66 for
0.16% of each simulation's steps, in the direction of more competitive pricing, nets a small
positive edge delta rather than zero. The family provably contains a near-0-line point and
this port's plumbing is verified correct before spending any of the 300-point search budget.

## Search (docs/DESIGN.md §2.5)

Run via `cargo run -p prop-amm-bench --release -- fit --strategy
strategies/005-vol-adaptive-cpmm-fee` (`config/bench.toml`'s `[search]` budget, 300 points;
screening segment `1_000_000..=1_000_199`, common random numbers). Coarse grid (3-dimensional,
5 points/axis, 125 points) then coordinate descent. Full curve committed at
`results/2026-08-21-fit-005-vol-adaptive-cpmm-fee.md`.

**Converged after 152 of 300 points** (budget never exhausted); **zero invalid points** — every
evaluated parameter vector produced a valid edge, consistent with the pre-search fuzz gate
having already cleared this family across every regime corner.

**Winning point: `FEE_LO = 5, A_NUM = 13, B_DEN = 1265`.**

| Segment | n | Avg edge |
| --- | --- | --- |
| screening (search inner loop) | 200 | 405.376621 |
| train (final evaluation) | 1,000 | 430.785183 |
| validation (final evaluation) | 1,000 | 425.946116 |

Against `001-cpmm-fee`'s own committed numbers (screening 384.82, train 406.14, validation
401.80): **+20.5 screening, +24.6 train, +24.1 validation** — a consistent ~+6% improvement
across all three independently-sampled segments, beating this porting issue's own prediction
range (+10 to +40) near the top of it, and matching the source's own upstream README claim of
"~+6% over the best fixed fee" almost exactly (that document's numbers were measured with the
*source's original* constants `FEE_LO=20, A_NUM=7, B_DEN=160`, not this fitted point, so the
agreement is a family-level cross-check, not the same point).

**`FEE_LO` converged to its own frozen lower bound (5).** The coarse-grid curve shows every
`FEE_LO=5` row outperforming every `FEE_LO∈{20,36,51,66}` row at the matching `(A_NUM, B_DEN)`
— e.g. `[5,13,1265]=405.38` vs. `[66,13,1265]≈354.16` two rows down. This is a genuine
boundary hit, not a search artifact: per docs/DESIGN.md §2.4/§2.5, the frozen range is not
widened after seeing this result — a space widened post-hoc stops being an honest estimate.
Recorded as a known consequence for a future `005b` variant to explore (a lower `FEE_LO` floor
than 5, which this issue's own frozen-space rationale rejected in favor of avoiding the
near-zero-fee/normalizer-as-submission failure mode). Note the interior optimum is *not*
degenerate at that boundary: `A_NUM=13` and `B_DEN≈1265` are both interior points (coordinate
descent's step-halving converged smoothly around `B_DEN∈[1235,1387]`, a <0.1 edge-unit plateau
— see the curve's tail entries), so the fee mechanism's slope (`A_NUM`) is doing real work
compensating for a low floor, not just resting on a boundary in every dimension.

**Compile timing:** 154 warm compiles, min=0.317s, mean=0.616s, max=2.656s during this run —
exceeds `docs/DESIGN.md` §2.6's `<1s` target on the max sample. WHI-1205's own investigation
already ruled out every structural cause of a fast-path compile-time gap on this exact
machine/session pairing (see `strategies/001-cpmm-fee/NOTES.md` § Cheap verification) and
found the mechanism itself is not at fault; this run's higher variance (max 2.656s vs.
WHI-1205's 0.343s) is consistent with that section's standing conclusion — background system
load during a ~150-point, multi-minute run, not a regression in the fast path — and is not
re-investigated here (WHI-1205 is closed; a fresh recurrence would be its own issue if it
persisted independent of load, which a single run here cannot establish).

### Demonstration reproducibility cross-check

The nested-point demonstration above (`[66,0,2000] -> 385.973322`) reappears verbatim in this
search's own evaluated-curve data (`results/2026-08-21-fit-005-vol-adaptive-cpmm-fee.md`'s
`[66, 0, 2000] -> 385.973322` row, hit by the coarse grid itself) — the same fast path, same
screening seeds, same value, from two independent invocations. That agreement is itself a
small parity check on the fast path's own determinism.

## Grid mode: 27-cell fragility matrix (docs/DESIGN.md §2.3)

`cargo run -p prop-amm-bench --release -- grid --candidate strategies/005-vol-adaptive-cpmm-fee/lib.rs
--reference strategies/001-cpmm-fee/lib.rs` — reference is `001-cpmm-fee` (the 0-line every
M1 candidate is measured against, docs/DESIGN.md §2.8), not `000-normalizer`, since this is a
ranked candidate comparison, not a router-symmetry check. Full table committed at
`results/2026-08-21-grid.md`.

| cell | fee (bps) | liq mult | sigma | candidate | reference | mean diff |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | 30 | 0.4 | 0.0001 | 807.14 | 710.96 | **+96.18** |
| 1 | 30 | 0.4 | 0.0010 | 781.71 | 695.34 | **+86.36** |
| 2 | 30 | 0.4 | 0.0070 | 482.47 | 197.26 | **+285.20** |
| 3 | 30 | 1.0 | 0.0001 | 393.18 | 389.11 | +4.07 |
| 4 | 30 | 1.0 | 0.0010 | 385.94 | 382.63 | +3.30 |
| 5 | 30 | 1.0 | 0.0070 | 7.96 | -128.96 | **+136.92** |
| 6 | 30 | 2.0 | 0.0001 | 156.95 | 205.57 | **-48.61** |
| 7 | 30 | 2.0 | 0.0010 | 168.09 | 192.54 | **-24.45** |
| 8 | 30 | 2.0 | 0.0070 | -182.74 | -288.26 | **+105.52** |
| 9 | 55 | 0.4 | 0.0001 | 904.40 | 842.83 | **+61.57** |
| 10 | 55 | 0.4 | 0.0010 | 887.81 | 836.89 | **+50.92** |
| 11 | 55 | 0.4 | 0.0070 | 513.06 | 278.70 | **+234.37** |
| 12 | 55 | 1.0 | 0.0001 | 546.64 | 547.15 | -0.51 |
| 13 | 55 | 1.0 | 0.0010 | 546.20 | 546.96 | -0.76 |
| 14 | 55 | 1.0 | 0.0070 | 111.22 | 32.09 | **+79.13** |
| 15 | 55 | 2.0 | 0.0001 | 403.09 | 348.19 | **+54.90** |
| 16 | 55 | 2.0 | 0.0010 | 387.31 | 341.02 | **+46.29** |
| 17 | 55 | 2.0 | 0.0070 | -121.30 | -159.57 | **+38.28** |
| 18 | 80 | 0.4 | 0.0001 | 1058.70 | 1030.48 | **+28.22** |
| 19 | 80 | 0.4 | 0.0010 | 1045.36 | 1018.67 | **+26.69** |
| 20 | 80 | 0.4 | 0.0070 | 686.42 | 483.73 | **+202.69** |
| 21 | 80 | 1.0 | 0.0001 | 828.87 | 838.09 | **-9.22** |
| 22 | 80 | 1.0 | 0.0010 | 828.03 | 838.72 | **-10.69** |
| 23 | 80 | 1.0 | 0.0070 | 299.52 | 315.55 | **-16.04** |
| 24 | 80 | 2.0 | 0.0001 | 655.77 | 668.09 | **-12.31** |
| 25 | 80 | 2.0 | 0.0010 | 654.67 | 665.90 | **-11.24** |
| 26 | 80 | 2.0 | 0.0070 | 31.54 | 152.43 | **-120.89** |

17 of 27 cells favor `005`, several by a wide margin (up to +285 at cell 2); 10 favor `001`
(bold, negative). All 95% CIs exclude 0 except the two smallest-magnitude cells (12, 13) —
every other listed sign is a real effect, not noise (`results/2026-08-21-grid.md` for the
full CIs).

### Explaining the negative cells

**Every negative cell shares one property: `norm_liquidity_mult >= 1.0`** (the *opponent's*
reserves, not this candidate's — `docs/DESIGN.md` §2.8's own liquidity axis scales the
normalizer, never the submission). Conversely, **all 9 `liq=0.4` cells (thin opponent) are
strongly positive** — when the opponent's own reserves are shallow, its price impact is worse
than either fixed-fee at any level, so flow floods toward whichever venue is priced, and `005`'s
cheaper, adaptive floor (`FEE_LO=5` vs. `001`'s flat `66`) captures noticeably more of that
already-abundant flow.

Within `liq >= 1.0`, the negative cells cluster into two distinct groups, both real (every
cell named below has a 95% CI excluding 0 — only cells 12/13, not named in either group, are
the tiny, non-significant ones with CIs straddling 0):

- **All 6 `norm_fee_bps=80` cells (21-26) are negative** — the single largest and most
  consistent block, explained below.
- **Cells 6 and 7 (`norm_fee_bps=30, liq=2.0`, low/mid sigma: -48.61 and -24.45)** are a
  second, smaller effect with the same underlying cause as the `fee=80` cluster below (margin
  per trade, not flow-share) rather than noise. `fee=30, liq=2.0` is the single toughest
  opponent configuration in the whole grid — simultaneously the cheapest *and* the deepest the
  normalizer ever gets, so at low/mid sigma, where `sigma_hat` sits near the `FEE_LO=5` floor,
  `001`'s flat 66bps edges out our thinner floor on margin per trade the same way it does in
  the `fee=80` cluster below. This reasoning does *not* extend to cell 8 (same `fee=30,
  liq=2.0`, high sigma), which flips
  strongly **positive** (+105.52): at high sigma the adaptive fee's LVR defense (ramping well
  above 66, per the cell-26 calculation below) starts to dominate the margin-per-trade effect
  even against this toughest competitor — the two forces trade off differently by sigma level,
  and this grid does not resolve exactly where the crossover sits.

The `fee=80` pattern is the important one: at `norm_fee_bps=80` **both**
candidate and reference already do well (edges of 300-1050) regardless of which one is
running — an expensive, non-thin opponent still routes plenty of flow to *any* competitively
priced venue, so flow-share is not the deciding factor there. What differs is **edge captured
per unit of that already-won flow**: `001`'s flat 66bps charges a fixed, comfortable margin on
every trade it wins; `005`'s adaptive floor charges as little as 5bps when its own `sigma_hat`
estimate is low (true at `sigma=0.0001`/`0.0010`, cells 21/22/24/25) — winning essentially the
same flow at a much thinner margin nets strictly less edge. This is the flip side of the
mechanism's own designed tradeoff (cheap at low vol to *capture* spread the fixed-fee
normalizer over-charges): it only pays off when being cheap is what *wins* the flow in the
first place (the `liq=0.4` cells); when the flow was already coming regardless (`fee=80`,
`liq>=1.0`), being needlessly cheap only gives away margin.

The worst cell (26: `fee=80, liq=2.0, sigma=0.0070`, -120.89) combines this with the one
sigma level where it should matter least: at `sigma_hat~70` (grid's own `sigma=0.007` level,
per-step bps, matches the estimator's designed operating ceiling), the fitted fee formula
evaluates to `5 + 13*70/10 + 70^2/1265 ≈ 99bps` once warmed up — *above* `001`'s flat 66. That
this cell is still the single worst one indicates the estimator does not spend the whole
10,000-step simulation at its warmed-up, high-sigma value: `COLD_FEE=55`'s 16-step warmup, the
band-edge/retail-sampling bias documented in § Estimator bias below (inflating `sigma_hat` in
some regimes, but here more relevantly *not reaching* its steady-state fast enough relative to
a competitor that never needs to warm up at all), and simple realized-path variance around a
single `gbm_sigma` draw all pull the *time-averaged* realized fee below the formula's
steady-state value — exactly the mechanism's known fragility this port inherits faithfully
(§2.9: this is a property of the source's own design, not introduced by the port or the fit).

None of this is disqualifying: `005` beats `001` on the aggregate, decision-input segments
(screening/train/validation, all +20 to +25) by a comfortable margin, and grid mode is
explicitly *not* a ranking input (docs/DESIGN.md §2.3) — it identifies where the fitted point
is fragile, which is exactly what it did here. A `005b` variant that widens `FEE_LO`'s
practical floor when the opponent is expensive but not thin, or that fixes the `var_sum`
divisor bias noted in § Estimator bias, is the natural next step (§2.9) — out of scope for
this faithful port.

## Estimator bias (informational, not a defect in this port)

Two things the source's own header comment overstates or elides, both already anticipated by
the issue's review amendments and neither changed here (§2.9 — an improvement is a `005b`
variant, not part of this port):

- **The sampling-point claim is only conditional.** "The first executed trade of a NEW step is
  typically the arbitrageur's correction" holds only when the arbitrageur actually trades that
  step — at low `gbm_sigma`, many steps clear no arb profit at all, so the first executed trade
  is retail instead, and its post-trade reserves carry that trade's own price impact (~10-40bps
  at default reserves) on top of the true per-step move (as low as ~1-10bps). Expect this to
  inflate `sigma_hat` specifically in low-vol regimes.
- **`var_sum` is divided by `count`, not by elapsed steps**, so a sample spanning several quiet
  (no-first-trade) steps is not amortised — per-step variance is inflated further by roughly
  the average gap between samples. Combined with the band-edge effect (an arb-corrected price
  sits at the fee-band's edge, ± the fee itself, with mild positive feedback since a higher fee
  widens the band), `sigma_hat` is expected to equilibrate well above the true `gbm_sigma` in
  low-vol regimes specifically — the same 12 grid cells the fixed-fee 0-line already loses.
  Fixing the divisor would be a genuine improvement, filed as a `005b` variant candidate, not
  folded into this faithful port.

## Compute units (docs/DESIGN.md §2.9, cross-cutting finding #9)

No bench tooling exposes measured CU headroom today (`prop-amm validate` doesn't report it,
and `crates/executor`'s `SyscallContext::get_remaining()` — the value that would answer this —
is private and not surfaced by `BpfExecutor::execute`'s public API; `crates/executor` is
upstream-owned, so exposing it is an upstream-sync-lane change, not something to add inside
this single-strategy issue). This gap applies to every M1 strategy, not uniquely to this one.

Division-count estimate instead, since finding #9 itself names division count (not
instruction count) as the thing to bound on SBF: warmed-up `compute_swap` (past
`WARMUP_STEPS`) does 3 divisions in `fee_from_state` (`variance`, `lin`, `quad`) plus
`isqrt`'s Newton loop (2 divisions/iteration; the loop's input is bounded by `R2_CAP = 62,500`
by construction, converging in well under 16 iterations in practice) plus `cp_out`'s 2
divisions — on the order of 25 divisions worst case, versus `001-cpmm-fee`'s 2. At finding
#9's own cited worst-case cost (10^2-10^3 CU/division on SBF), that is a worst-case estimate
of roughly 2,500-25,000 CU, comfortably inside the 100,000 CU limit; the cold/warmup path
(most of each simulation's first 16 steps) does none of this and costs the same as `001`.
Corroborated empirically: `prop-amm validate`'s native/BPF parity check and `bench parity`
below both execute the real BPF program repeatedly with no compute-budget failure.

## Parity gate (docs/DESIGN.md §2.6)

Reproduced via `cargo run -p prop-amm-bench --release -- parity --strategy
strategies/005-vol-adaptive-cpmm-fee` against the committed `(FEE_LO=5, A_NUM=13,
B_DEN=1265)` point: `prop-amm validate` passes; the fast path and the reference path agree
on **all 1,000 `observation`-segment seeds to `0` relative difference** (well inside the
`1e-9` gate — the source's own comment claim of native/BPF parity by construction holds
after adaptation, unchanged); the fast-path aggregate (avg edge 422.93) matches `prop-amm
run`'s own 2-decimal output exactly (422.93, total 422930.50). See
`results/2026-08-21-parity-005-vol-adaptive-cpmm-fee.md` for the committed snapshot.

**Leaderboard-comparable number: avg edge 422.93** (`observation` segment, seeds `0..=999`,
native), against `001-cpmm-fee`'s own **399.97** on the same segment — a **+22.96 (+5.7%)**
improvement, and ahead of the source's own upstream README claim of "~+6% over the best
fixed fee" (that document's in-sample number, 423.6, was measured with the source's
*original* constants, not this fitted point — the near-exact agreement is a family-level
cross-check, not the same measurement).
