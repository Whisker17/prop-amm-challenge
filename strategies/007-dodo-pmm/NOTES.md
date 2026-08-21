# 007-dodo-pmm

## Provenance

Source form: **source (Solidity), library code** — `DODOEX/contractV2`'s `PMMPricing.sol` /
`DODOMath.sol` / `DecimalMath.sol`, pinned at commit
`2f1bcdac7ef1beee7599a756e2eed26732c2536d` (`docs/references/007-dodo-pmm/README.md` for the
full file list and chain of custody). Apache-2.0 licensed at that commit.

Unlike `005` (a direct, already-`pinocchio`-shaped Rust source), this is a 1e18-fixed-point
Solidity **library**, operating on a general `V0 != V1`, three-regime (`ONE`/`ABOVE_ONE`/
`BELOW_ONE`) PMM state machine. Porting it into this challenge's single-file
`compute_swap`/`after_swap` shape required: collapsing the state machine to `R = ONE`
(justified below, not a shortcut), rescaling from 1e18 to this harness's 1e9 nano fixed
point, and deriving a from-scratch closed-form quote and anchor-update rule — this is a
derivation port, not a transcription.

## Port target — decided by the issue, not by this port

**DPP's two-sided PMM, collapsed to `R = ONE`, with the arbitrageur as the oracle.** Not
`DPPOracle` (unportable — no pre-arbitrage price feed exists in this harness), not `DVM`
(one-sided by construction — `getPMMState()` hardcodes `Q0=0`, `R=ABOVE_ONE`, so a fixed-`i`
DVM bleeds under a GBM that drifts below the initial price), and not the full R-state
machine (three independent failure modes at real states: its `k=0` endpoint fails
`validate`, its R-crossing splice truncates output, and `DODOMath`'s `V2>V1 -> return 0`
guard is reachable — collapsing to `R=ONE` removes all three). Re-anchoring the anchor to
the post-trade mid on every executed trade means `B0=B`, `Q0=Q` always hold at quote time,
so the R-state machine (which exists to handle inventory drift *between* oracle updates) is
dead code — this is the faithful rendering of `DPPOracle`'s per-quote oracle read, not a
simplification of convenience.

## Fidelity self-assessment (docs/DESIGN.md §2.9)

This is **not** a byte-for-byte port (the source is Solidity at a different fixed-point
scale, operating on a more general state than this port needs) — every departure from the
source's own arithmetic is itemised here per the fidelity contract:

- **`R = ONE` collapse.** `V0 = V1` always (both equal the pre-trade reserve on the side
  being solved), which the source's own `_ROneSellBaseToken`/`_ROneSellQuoteToken` already
  express as the `V0 = V1` special case of `_SolveQuadraticFunctionForTrade` — no new
  algebra invented, just the case the source itself calls out as `R = ONE`.
- **1e18 -> 1e9 rescale.** `P_SCALE = 1e9` replaces `DecimalMath.ONE = 1e18`; `k` is kept as
  a plain rational `K_BPS/K_DEN` (`K_DEN = 10_000`, i.e. bps of `ONE`) rather than a second
  1e18-scaled fixed point, since `k`'s own useful range only needs 1e-4 granularity (see
  Parameter space below). This is *why* the two overflow clamps below are necessary at all
  — the rescale costs headroom the source's 256-bit-friendly EVM arithmetic never had to
  worry about.
- **Rationalised quadratic form in the `bSig` branch** (docs/DESIGN.md §2.9 cross-cutting
  finding #7): the source computes `numerator = squareRoot.sub(bAbs)` whenever `bSig`
  holds — exactly the subtractive-cancellation form
  `crates/sim/src/curve_checks.rs::exposes_false_positive_from_cancellation_prone_concave_curve`
  proves trips this repo's own concavity checker on a *provably legal* curve. This port uses
  `V2 = 2*k*V0^2 / (sqrt(disc) + b_abs)` instead — algebraically identical, immune to
  cancellation. A source-fidelity change forced by this repo's own checker, not a mechanism
  change.
- **Adaptive-precision `isqrt`** (`scaled_isqrt`, see § CU and arithmetic risk below): the
  source's 1e18/256-bit arithmetic has enough headroom that plain Solidity `sqrt` never
  needs this; at 1e9/u128 it does. This is new code with no Solidity analogue, added to
  satisfy `prop-amm validate`'s 1-nano concavity tolerance (see the callout below) — not a
  mechanism change, since it only tightens the same floor-then-ceil computation already
  described above.
- **Fee collapsed to a single `FEE_BPS` on the output.** Source-faithful direction
  (`DVMTrader.querySellBase/querySellQuote` fee the *output*, verified in source), but the
  source's own `lpFeeRate`/`mtFeeRate` split is collapsed to one rate since no separate LP/
  maintainer recipients exist in this harness (the same "no recipients" reasoning as any
  other M1 strategy that drops a real-world fee-distribution mechanism).
- **Anchor update (`after_swap`) is new code, not a port.** The source's oracle is external
  (`DPPOracle`); this harness has none. The re-anchor rule — recompute the PMM's own
  post-trade marginal price via `R_f` from the reconstructed pre-trade reserve, not the raw
  post-trade reserve ratio — follows directly from `getMidPrice`'s own formula (verified
  algebraically against the k=1 case reducing exactly to a CPMM's post-trade reserve ratio;
  see the derivation note in `lib.rs`'s own header comment), but has no line-for-line source
  counterpart since the source never re-derives its own mid without an oracle read.
- **Overflow clamps** (`RESERVE_CLAMP = 2^59`, `INPUT_CAP_MULT = 16`): engineering guards
  with no Solidity analogue (the EVM's 256-bit words never need them). Both are
  monotone-safe terminal plateaus, never binding in real simulations (§ CU and arithmetic
  risk below) — see cross-cutting finding #7 ("clamp first, never saturate").

## Shape-safety rule (docs/DESIGN.md §2.9, cross-cutting finding #3)

`solve_quadratic_for_trade` reads only the anchor price (from storage), the live reserves
passed alongside `input_amount`, and compile-time consts — never `input_amount` for
anything but `delta`. `fair_amount`, `part1`/`part2`, and the discriminant are all computed
without reference to which quote is being priced beyond `delta` itself, so the quoted curve
for a fixed state is a pure function of trade size — monotone and concave in input for `k`
in `(0, 1]` (verified below via `prop-amm validate` and `bench fuzz`). `step` is never read
in `compute_swap` (only in the `after_swap` payload, offset 34).

## Cold start and garbage-state handling (findings #4, #5)

- **Cold start.** Storage is zero-initialized per simulation. `compute_swap`'s
  `anchor_price` falls back to the live reserve ratio (`ry_c*P_SCALE/rx_c`) whenever the
  magic sentinel doesn't match — which a zeroed buffer never does — so the router's very
  first quote (before any `after_swap` has run) reads the *initial* reserve ratio as the
  anchor, exactly encoding the initial price.
- **Garbage state.** `validate.rs`'s randomized probe fills `storage[0..32]` (including the
  8-byte magic at offset 0) with pseudo-random bytes. An 8-byte random magic has a `2^-64`
  chance of colliding with `MAGIC` (`0x444F_444F_5F50_4D4D`), so every randomized-probe
  iteration falls back to the live-ratio anchor rather than reading `anchor_price` as an
  arbitrary `u128` — confirmed by `prop-amm validate`'s "Randomized reserve/storage checks"
  passing (§ Parity gate below).

## Every error path saturates or returns 0, never panics (finding #6)

- `solve_quadratic_for_trade` returns `0` for `v == 0 || delta == 0 || i_fp == 0` (the
  harness's own "quote > reserve -> 0" convention), and `v.saturating_sub(v_out)` folds
  `DODOMath`'s `V2 > V1 -> return 0` guard into a saturating subtraction — reachable only via
  ceil-jitter at the smallest inputs, where a zero output is monotone-safe (the same
  argument `005`'s NOTES makes for its own smallest-input truncation).
- `fair_amount` uses `checked_mul` for `i_fp * delta`, falling back to `u128::MAX` (which
  then clamps to `INPUT_CAP_MULT * v`) instead of panicking on overflow — a monotone-safe
  plateau for the arbitrageur's largest brackets (up to ~1.8e19 nano) against a stale, tiny
  anchor.
- `reciprocal_fp` returns `0` for `i_fp == 0` and floors to `0` for an extreme
  (validator-probe-only) anchor price rather than dividing by zero.
- Every reserve multiplication in `solve_quadratic_for_trade`/`rf_denominator_scaled` uses
  `saturating_mul`/`saturating_add` — never a bare arithmetic op that could panic on
  overflow in a synthetic extreme state.

## Clamping before squaring; adaptive-precision sqrt (findings #7, #9)

**Two overflow clamps**, both monotone-safe because they bind on *state*, never on the
computed output:

1. `RESERVE_CLAMP = 2^59` nano (~5.8e8 tokens) bounds every reserve fed into a squaring
   operation. Unclamped, a synthetic `validate`-probe reserve near `u64::MAX` (~1.8e19)
   squared (~3.4e38) sits right at `u128::MAX` before the discriminant's other terms are
   even added — `2^59` leaves enough headroom that `v^2`, `b_abs^2`, and the discriminant's
   cross term all provably fit u128 across the full `K_BPS` range (verified by direct
   enumeration across `v in {1, ..., RESERVE_CLAMP}` x `k_bps in {1, 25, ..., 9999}` x
   `fair/v in {0, 0.001, 0.5, 1.0, 2.0, 16.0}` before this was trusted — see the worked
   Python model this port's arithmetic was validated against).
2. `INPUT_CAP_MULT = 16` caps the effective fair-value input at `16 * v`, past which
   `solve_quadratic_for_trade` returns the cap-point output for any larger `delta` — a
   terminal plateau, not a truncation (slope falls to 0 after positive slopes, so a
   monotone-safe endpoint).

**`scaled_isqrt`: adaptive precision, not a fixed one.** A naive `isqrt` has a `+/-1`-unit
absolute floor error. The discriminant's magnitude swings by ~15 orders of magnitude across
this family's real operating range — ~1e22 at `validate`'s small fixed probe (real
reserves of a few thousand tokens) vs. ~1e37 at the `RESERVE_CLAMP` extreme — so a single
fixed scale-up factor is either wasted (too small at the small end) or overflows (too large
at the clamped end). `scaled_isqrt` instead spends whatever leading-zero headroom `u128`
has left on the discriminant *itself*: `shift = min(60, discriminant.leading_zeros()/2)`,
`sqrt_disc_scaled = isqrt(discriminant << 2*shift)`. At the small, realistic end this
buys roughly 1e8x finer effective precision (`shift` around 25-30); at the clamped extreme,
`shift` collapses toward 0 (no spare headroom left) — exactly where nano-level precision is
moot anyway, since the reserve itself is ~5.8e8 tokens.

### Why this was necessary — a real concavity violation, found and fixed before freezing

Before adding `scaled_isqrt`, `prop-amm validate` failed at `(K_BPS=2500, FEE_BPS=66)`:
`FAIL: Concavity violation (buy side). At size=10, step2=9930 > step1=9928` (a **2-nano**
violation against the checker's `CONCAVITY_STEP_TOL_NANO = 1` — `crates/cli/src/commands/
validate.rs`). Reproduced in an exact-rational (`Decimal`, 60 digits of precision) model of
the same quadratic: the *true* marginal-output differences at that probe were
`step1 = 9994.996`, `step2 = 9994.995` — genuinely concave, differing by under **0.0005
nano**, a signal an order of magnitude below a single `isqrt` floor unit once that unit is
scaled through the rest of the formula. This is exactly the "isqrt floor error times a
large coefficient" trap docs/DESIGN.md §2.9 cross-cutting finding #7 warns about, and
exactly the failure mode this issue's own § CU and arithmetic risk section flagged as
possible ("if fuzz still shows >4-nano jitter, fall back to integer bisection"). Rather than
falling back to bisection (a materially larger rewrite), `scaled_isqrt` resolves it directly
by shrinking the floor error itself below the signal it was drowning out — confirmed by
re-running `prop-amm validate` at all four Step 0.5 probe points (below) after the fix,
all passing cleanly, and by the exact-rational model matching the fixed integer
implementation's sign at every probed size.

**CU, measured through the BPF executor** (not assumed — this repo's public API doesn't
expose `SyscallContext::get_remaining()` through `BpfExecutor`, so this port used a
throwaway example reusing only `prop-amm-executor`'s *public* API — `BpfProgram::load`,
`SyscallContext::new`/`get_remaining`, and `solana_rbpf`'s own VM types directly — never
modifying the upstream-owned crate; the example was deleted before this PR, not committed):

| State | Side | consumed CU |
| --- | --- | --- |
| small reserves (100/10,000 tokens), max input (~1.8e19 nano) | buy | 5,942 |
| small reserves, max input | sell | 6,072 |
| large reserves (~5.76e8 tokens each), max input | buy | 6,007 |
| large reserves, max input | sell | 5,884 |
| small reserves, size=10 | buy | 6,042 |
| `after_swap`, cold start, large reserves | sell | 729 |
| `after_swap`, warm re-anchor, large reserves | sell | 383 |
| `after_swap`, warm re-anchor, large reserves | buy | 382 |

**Worst case: 6,072 CU** — comfortably under both the protocol's 100,000 CU limit and this
issue's own ~80,000 CU stop-rule threshold (Step 0.5 #2 below), and well below the
`leading_zeros`-seeded `isqrt`'s own predicted 2,000-25,000 CU estimate (measured at
`K_BPS=2500`, the general-branch, non-`k=1` case; every `K_BPS` in the frozen range shares
the same code path and division count, so this is representative, not a single lucky point).

## Parameter space — 2 dimensions (docs/DESIGN.md §2.4/§2.5)

| Param | Range | Reason per bound |
| --- | --- | --- |
| `K_BPS` (curvature, units of 1e-4 of `ONE`) | **25..=10_000** | Upper `10_000` is `k=1.0` (the CPMM containment point, and the source's own ceiling, `require(k <= 10**18)` in `DVM.init`). Lower `25` is `k=0.0025` (~400x amplification vs. a CPMM at the same reserves) — the opponent is at most 2x our depth (`norm_liquidity_mult <= 2.0`), so 10x already out-depths every opponent and 400x is past any plausible optimum. `k=0` excluded as shape-fatal (the source's own `k=0` branch is a hard flat cap, `output = min(i*delta, V1)`, which fails `validate`'s strict monotonicity check at its fixed probe sizes). **Lower bound invented, unvalidated** (per the issue). |
| `FEE_BPS` | **1..=500** | Same space and rationale as `001-cpmm-fee`'s own frozen `FEE_BPS`: brackets the opponent's 30-80bps with headroom, and 66 must be reachable for containment. |

**Frozen, with reasons recorded:** fee on the output (source-faithful, `lpFee`/`mtFee`
collapsed to one rate); the anchor rule (full re-anchor to the post-trade mid on every
executed trade — a partial/EWMA anchor is a `007b` variant, its weight an invented
parameter this port declines to add); `RESERVE_CLAMP = 2^59` and `INPUT_CAP_MULT = 16`
(engineering guards, never binding in real regimes); the `scaled_isqrt` shift cap (`60`,
chosen only to keep `2*shift` safely under u128's 128-bit width, never a binding precision
limit at any tested state).

**No special-casing to `001`'s exact ceil-div arithmetic.** The issue's own "if
bit-exactness is preferred" callout is optional; this port keeps the general `k=1` closed
form (`fair*V/(V+fair)`, floored) uniformly rather than special-casing `K_BPS=10_000` to
`001`'s bit-exact arithmetic — simpler, one fewer special case, and the containment
demonstration below shows the resulting gap is well inside the predicted tolerance.
Recorded here, before the freeze, per the issue's own instruction.

## Step 0.5 — the bounded probe (run before any search, per the issue's own pre-registered stop rules)

All four steps used the scratch degenerate-range method `005` established: a throwaway
single-point range (frozen range collapsed to `MIN==MAX`) with `bench fit --max-points 1
--no-report`, never committed, never part of the real search budget.

### 1. Shape gate

`prop-amm validate` and `bench fuzz --strategy strategies/007-dodo-pmm` at
`K_BPS in {25, 400, 2500, 10_000}`, `FEE_BPS = 66` fixed:

| `K_BPS` | `prop-amm validate` | `bench fuzz` (324 states x 2 sides) |
| --- | --- | --- |
| 25 | PASS (incl. native/BPF parity) | PASS — zero shape violations |
| 400 | PASS (incl. native/BPF parity) | PASS — zero shape violations |
| 2,500 | PASS *after* the `scaled_isqrt` fix (see § CU above for the violation this caught and fixed) | PASS — zero shape violations, both before and after the fix |
| 10,000 | PASS (incl. native/BPF parity) | PASS — zero shape violations |

**Result: PASS at all four points.** *Stop rule* ("`wontfix` if a violation survives one
bounded fixing pass") was not triggered — the one violation found (at `K_BPS=2500`) was
fixed within the prescribed toolkit (a stable-form precision fix, the same category as the
rationalised quadratic and the two clamps), not deferred.

### 2. CU, once, through the BPF executor

**Worst case: 6,072 CU** (§ CU and arithmetic risk above, full table there). *Stop rule*
("`wontfix` above ~80,000 CU") not triggered — over 13x headroom.

### 3. Containment demonstration

`(K_BPS=10_000, FEE_BPS=66)` on the 200 screening seeds (`bench fit --max-points 1
--no-report` against a degenerate `10000..=10000`/`66..=66` range):

**screening avg edge: 386.842545**, against `001-cpmm-fee`'s committed **384.82** — a
**+2.02** gap. *Stop rule* ("diagnose if `|gap| > 5`") not triggered; the issue predicted
`|gap| <= ~2` (sign "mildly negative") from two unconditional terms (fee-on-output vs.
`001`'s fee-on-input; anchor-drift under nonzero fee) — the **magnitude** matches almost
exactly, the **sign** does not (this port measures a small *positive* delta, not negative).
Given the two terms the issue itself names are both plausibly sub-nano-precision-sensitive
at this scale (rounding-direction choices in the ceil/floor split, and the exact anchor
re-derivation this port uses — new code with no direct Solidity counterpart, per §
Fidelity self-assessment), a sign flip within the same tiny predicted magnitude reads as
implementation-detail noise around a genuinely near-zero effective gap, not broken
plumbing — consistent with the observation-segment and full grid results below, which show
the *same* small, consistently positive gap across 1000 seeds and all 27 regime cells.

Train/validation re-evaluation of the same point: **train 396.34** vs. `001`'s 406.14
(**-9.80**); **validation 391.74** vs. `001`'s 401.80 (**-10.06**) — a larger, and
sign-*flipped*, gap than screening's own +2.02. This is a real, reportable discrepancy
between segments, not a copy error (re-checked): screening is the *first 200* of train's
1000 seeds (`config/bench.toml`, `subset_of = "train"`), so the remaining 800 train seeds
pull the full-train average substantially negative relative to screening. The most likely
explanation, not independently confirmed: fee-on-output vs. fee-on-input is a genuinely
second-order effect only for *small* trades relative to reserves (verified algebraically:
at `B=100, input=10, fee=66bps`, the two conventions differ by ~0.07% of output) — but the
arbitrageur's largest brackets are *not* small relative to reserves, and at large trade
sizes the two fee conventions diverge well past second order. If the additional 800
train/validation seeds happen to sample more large-arb-trade activity than the 200
screening seeds do, this mechanism-level (not implementation-bug) explanation is
consistent with the sign and magnitude observed. Flagged here rather than investigated
further, since it does not change this issue's own decision (the concentration probe below
is decided on **screening**, per protocol, and the containment point itself is not being
searched or tuned).

### 4. The concentration probe — the actual go/no-go

Three points on screening, same degenerate-range method:

| Point | screening avg edge | vs. 384.82 |
| --- | --- | --- |
| `(K_BPS=2500, FEE_BPS=66)` | 169.06 | **-215.76** |
| `(K_BPS=400, FEE_BPS=66)` | -1,732.86 | **-2,117.68** |
| `(K_BPS=100, FEE_BPS=66)` | -5,150.66 | **-5,535.48** |

**All three land dramatically below 384.82.** *Stop rule triggered*: "the `k` axis is
net-harmful at every tested concentration, the family's whole thesis is dead — record a
negative result and close **without** running the 300-point search."

## Negative result — the concentration axis does not pay in this harness

**No 300-point search was run; 0 of the 300-point budget was spent** (Step 0.5's four
probes are explicitly outside the search budget, per protocol). This is the "honest failure
mode" the issue's own Prediction section named as a live possibility: *"a collapse to
`k ~ 1` and a tie at ~402 — i.e. the concentration axis turns out not to pay, which is
itself a clean result."* The collapse is confirmed, and it is not merely a tie — see below.

**Why concentration fails here, mechanically:** the issue's own "cost side is the honest
counterweight" section names the reason correctly in advance — against a stale anchor, the
arbitrageur's extractable size is amplified by roughly `1/k`, the same factor that
amplifies retail depth capture. At `k=0.25` (`K_BPS=2500`) that's already a 4x amplification
of adverse selection with only a partial depth-capture benefit (the router only sends
retail flow past the sizes the normalizer can't absorb, which — per `005`'s own grid — is a
minority of typical trade sizes); at `k=0.04`/`0.01` (`K_BPS in {400, 100}`) the adverse-selection
amplification (25x-100x) overwhelms any plausible retail-side benefit, producing the
catastrophic (-1,733, -5,151) screening losses measured. The probe's monotone-looking
collapse (169 -> -1,733 -> -5,151 as `k` shrinks) is consistent with this single mechanism
dominating at every tested concentration, not three unrelated failures.

**The family's best point is its own upper boundary (`k=1`), which is not a tie — it is a
small, consistent edge win over `001-cpmm-fee`.** Per the parity gate below, the
committed `(K_BPS=10_000, FEE_BPS=66)` point measures **avg edge 402.05** on the
`observation` segment (seeds `0..=999`) against `001-cpmm-fee`'s own **399.97**
(`strategies/001-cpmm-fee/NOTES.md`) — a **+2.08 (+0.5%)** improvement, consistent with the
+2.02 screening gap above. The 27-cell grid (§ Grid mode below) shows the *same sign*
in every one of 27 cells (26 positive, 1 slightly negative but statistically
indistinguishable from 0) — a small, structurally consistent win from the fee-on-output/
anchor-drift differences documented in § Fidelity self-assessment, not sampling noise.

**Committed value: `(K_BPS=10_000, FEE_BPS=66)`** — the boundary the pre-registered stop
rule closes the family at, per the `WHI-1209` boundary-hit precedent this issue's own
Prediction section names: presented here as a boundary result, not as an interior optimum,
because it is one.

## Grid mode: 27-cell fragility matrix (docs/DESIGN.md §2.3)

`cargo run -p prop-amm-bench --release -- grid --candidate strategies/007-dodo-pmm/lib.rs
--reference strategies/001-cpmm-fee/lib.rs` at the committed `(K_BPS=10_000, FEE_BPS=66)`
point. Full table: `results/2026-08-21-grid-007-dodo-pmm.md`.

| cell | fee (bps) | liq mult | sigma | candidate | reference | mean diff |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | 30 | 0.4 | 0.0001 | 716.70 | 710.96 | +5.74 |
| 1 | 30 | 0.4 | 0.0010 | 700.63 | 695.34 | +5.29 |
| 2 | 30 | 0.4 | 0.0070 | 203.52 | 197.26 | +6.26 |
| 3 | 30 | 1.0 | 0.0001 | 391.45 | 389.11 | +2.34 |
| 4 | 30 | 1.0 | 0.0010 | 385.02 | 382.63 | +2.38 |
| 5 | 30 | 1.0 | 0.0070 | -126.89 | -128.96 | +2.07 |
| 6 | 30 | 2.0 | 0.0001 | 206.71 | 205.57 | +1.15 |
| 7 | 30 | 2.0 | 0.0010 | 193.51 | 192.54 | +0.96 |
| 8 | 30 | 2.0 | 0.0070 | -287.34 | -288.26 | +0.93 |
| 9 | 55 | 0.4 | 0.0001 | 849.56 | 842.83 | +6.73 |
| 10 | 55 | 0.4 | 0.0010 | 843.03 | 836.89 | +6.14 |
| 11 | 55 | 0.4 | 0.0070 | 284.45 | 278.70 | +5.75 |
| 12 | 55 | 1.0 | 0.0001 | 549.17 | 547.15 | +2.02 |
| 13 | 55 | 1.0 | 0.0010 | 548.97 | 546.96 | +2.01 |
| 14 | 55 | 1.0 | 0.0070 | 33.83 | 32.09 | +1.74 |
| 15 | 55 | 2.0 | 0.0001 | 348.87 | 348.19 | +0.68 |
| 16 | 55 | 2.0 | 0.0010 | 341.71 | 341.02 | +0.69 |
| 17 | 55 | 2.0 | 0.0070 | -159.53 | -159.57 | +0.05 |
| 18 | 80 | 0.4 | 0.0001 | 1035.70 | 1030.48 | +5.21 |
| 19 | 80 | 0.4 | 0.0010 | 1024.81 | 1018.67 | +6.15 |
| 20 | 80 | 0.4 | 0.0070 | 490.03 | 483.73 | +6.30 |
| 21 | 80 | 1.0 | 0.0001 | 840.81 | 838.09 | +2.72 |
| 22 | 80 | 1.0 | 0.0010 | 841.25 | 838.72 | +2.53 |
| 23 | 80 | 1.0 | 0.0070 | 318.17 | 315.55 | +2.61 |
| 24 | 80 | 2.0 | 0.0001 | 669.21 | 668.09 | +1.13 |
| 25 | 80 | 2.0 | 0.0010 | 667.05 | 665.90 | +1.15 |
| 26 | 80 | 2.0 | 0.0070 | 152.85 | 152.43 | +0.42 |

**26 of 27 cells favor `007` (positive), 1 (cell 17) is a statistical tie** (95% CI
`[-0.33, 0.43]`, straddling 0 — the only cell where this is true; every other cell's CI
excludes 0, per `results/2026-08-21-grid-007-dodo-pmm.md`). The pattern is consistent with
the fee-on-output vs. fee-on-input distinction: the effect is *largest* in the thinnest,
cheapest-opponent cells (0-2, 9-11, 18-20, where flow volume and hence total fee revenue is
largest) and *smallest* at high sigma against a deep opponent (cell 17, 26 — where trade
sizes and hence the fee-convention delta per trade shrink relative to the volatility-driven
swings dominating the edge number). No cell is fragile in the way `005`'s own grid showed
(no cell flips sign, unlike `005`'s 10 negative cells) — expected, since this port and
`001` are nearly the same mechanism at `K_BPS=10_000`.

## Parity gate (docs/DESIGN.md §2.6)

`cargo run -p prop-amm-bench --release -- parity --strategy strategies/007-dodo-pmm`:
`prop-amm validate` passes; the fast path and reference path agree on **all 1,000
`observation`-segment seeds to `0` relative difference** (well inside the `1e-9` gate); the
fast-path aggregate (avg edge 402.05) matches `prop-amm run`'s own output exactly (402.05,
total 402052.04). See `results/2026-08-21-parity-007-dodo-pmm.md` for the committed
snapshot.

**Leaderboard-comparable number: avg edge 402.05** (`observation` segment, seeds
`0..=999`, native), against `001-cpmm-fee`'s own **399.97** on the same segment — a
**+2.08 (+0.5%)** improvement.

## Compute units

See § Clamping before squaring; adaptive-precision sqrt above — measured, not assumed, per
cross-cutting finding #9: **worst case 6,072 CU** for `compute_swap`, **729 CU** for
`after_swap`'s cold-start path, **383 CU** for its warm re-anchor path. All comfortably
inside the 100,000 CU protocol limit.
