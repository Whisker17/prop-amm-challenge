# 004-ewma-shock-decay-fee

## Provenance

Source form: **source (Rust), direct submission shape** (the second-highest-fidelity source
in the frozen list after `005`, per `docs/DESIGN.md` §6.2).
`docs/references/004-ewma-shock-decay-fee/v2-solana-lib.rs` is `lilaclilac09/pamm-a`'s own
past competition submission (`src/lib.rs` in that repo), pinned at commit
`b2899305f8d91cf0df03858ca6515682493bece5`. No repo-level license is declared at that
commit (GitHub's API reports none); the repo's own `README.md` labels this exact file "the
competition submission" for a prior run of this same challenge.

That directory also holds `v2-ethereum-Strategy.sol` (a richer EVM port adding momentum +
inventory-skew terms) and `v3-flow-aware-strategy.rs` (adds an off-chain,
competitor-activity-derived signal with no in-simulator analogue). Per the issue's own
"Out of scope" section, **both are out of scope for this issue**: no clear reason surfaced
during porting to fold either in, so they stay candidate `004b`/`004c` variant material
(`docs/DESIGN.md` §2.9), not part of this faithful port.

## Fidelity self-assessment (docs/DESIGN.md §2.9)

**Minimum changes only**, per the fidelity contract:

- `compute_swap`, `fee_from_storage` (source: `fee_from_state`/inline fee calc),
  `after_swap`, and every byte-offset helper are mechanism-identical to the source: same
  storage layout, same EWMA update, same shock re-arm/decay rule, same constant-product
  output shape (`ceil_div`-based, saturating).
- The storage byte layout (`ewma_vol`@0, `last_rx`@8, `last_ry`@16, `shock_steps`@24, 32 of
  1024 bytes used) is unchanged and verified against `crates/shared/src/instruction.rs`'s
  current ABI (offsets 18/26/34 for post-trade `reserve_x`/`reserve_y`/`step` in the
  `after_swap` payload; offsets 0/1/9/17/25 for `side`/`input_amount`/`reserve_x`/
  `reserve_y`/`storage` in the `compute_swap` instruction) — they line up exactly, so this
  port required **no ABI adaptation**, only wiring (same finding `005`'s own NOTES.md made
  for its own source).
- What changed, and why each change is within the fidelity contract's bounds:
  - `NAME` — required by the submission interface (§2.9's explicit carve-out).
  - `MODEL_USED` — preserved as `"Claude Sonnet 4.6"` (the source's own constant) per the
    issue's explicit instruction: the mechanism is untouched, so the field describing which
    model produced it hasn't changed either (same precedent `001`/`005` set).
  - `BASE_FEE_1E9`/`VOL_MULT`/`MAX_FEE_1E9`/`SHOCK_FEE_PER_STEP_1E9` moved into (or, for
    `VOL_MULT`, kept in) a `// === PARAMS BEGIN/END ===` block, and the three fee-scale
    constants re-expressed in **bps** (`BASE_BPS`/`MAX_FEE_BPS`/`SHOCK_FEE_PER_STEP_BPS`)
    with the `_1E9` values derived from them (`bps * 100_000`, since `1 bps = 1e9/10_000`).
    This is mechanically required for `bench fit` to search them at all
    (`tools/bench/src/params.rs` only rewrites literal integer `const` lines) and matches
    the issue's own review-amendment table, which is already stated in bps. The *rescaling
    itself* changes no arithmetic result: `BASE_FEE_1E9 = BASE_BPS * 100_000` is evaluated
    at compile time (a `const` expression), so the runtime formula
    (`BASE_FEE_1E9 + vol_fee + shock_fee, capped`) is byte-for-byte the source's formula,
    just fed a differently-declared constant. Same pattern `005`'s port used for its own
    derived `R2_CAP = MOVE_BPS_CAP * MOVE_BPS_CAP`.
  - The source's hardcoded 100bps (`MAX_FEE_1E9 = 10_000_000`) fee cap became the searched
    `MAX_FEE_BPS`, per the issue's own review amendment (extending the cap is this
    strategy's central hypothesis, not an incidental change — see § Frozen parameter space).
  - Added a defensive `saturating_sub` in `compute_swap`'s `keep` calculation (the
    consumer of the fee value) — never binds within the frozen range, since
    `fee_from_storage` already clamps to `MAX_FEE_1E9 <= 40,000,000`, far below
    `1,000,000,000`; unchanged normal-path behavior, same class of change `005`'s
    `FEE_HARD_MAX` was. An earlier draft of this port also added a second clamp
    (`FEE_HARD_MAX_1E9`) *inside* `fee_from_storage` itself — removed after review: unlike
    `005`'s guard (which sits in `cp_out`, a function that takes an arbitrary caller-
    supplied `fee_bps`), `fee_from_storage` is the sole *producer* of the fee, so a second
    ceiling there had no caller-supplied value to guard against and was provably dead
    (900,000,000 vs. a maximum possible output of 40,000,000). `.min(MAX_FEE_1E9)` alone
    is the correct and complete guard for a producer.
  - Renamed the source's inline fee/output logic into `fee_from_storage`/`ceil_div` helper
    functions for readability; no arithmetic changed.
  - `after_swap`'s length guard is `data.len() < 42 || storage.len() < STATE_END` (42, not
    the source's `34`) — stricter than the source needs, since neither this function nor
    the source reads past offset 34 (`cur_ry`'s `read_u64(data, 26)` needs only 34 bytes).
    The extra 8 bytes account for the payload's unused `step` field (offset 34..42) for
    readability against `crates/shared/src/instruction.rs`'s documented `AFTER_SWAP_SIZE`
    boundary; harmless in practice since `encode_after_swap` always emits the full
    1,066-byte payload, but flagged here since it is a behavioral delta from the source
    the earlier draft of this list omitted.
  - `fee_from_storage`'s `if storage.len() < STATE_END { return BASE_FEE_1E9 }` branch has
    no source counterpart (the source's `fee_from_state`-equivalent logic assumed a
    full-length storage slice). Never triggered by this harness (storage is always exactly
    1024 bytes), but added as defense-in-depth consistent with finding #5's spirit and
    flagged here for the same reason as the guard above.

## Signal reinterpretation (cross-cutting finding #11, per-strategy review amendment)

The issue's own review amendment corrects its earlier "shock re-arms when the price move
since the last trade exceeds 0.5%" framing: `last_rx`/`last_ry` are the *pre-state of the
current trade*, so the "move since the last trade" `after_swap` computes is **the current
trade's own price impact**, not an independent read of the fair-price process. It correlates
with `gbm_sigma` only indirectly — arb-driven trades correct the fair-price drift (so their
impact scales with sigma), while retail trades contribute a sigma-independent impact floor
(~0.2-0.4% per trade at the default reserves) that the estimator cannot see below. This is
unchanged from the source (a faithful port inherits the source's own estimator, correct or
not — §2.9) and is exactly what the grid's negative-cell cluster below is consistent with:
the estimator adapts more reliably upward (high sigma) than downward (low sigma).

## Shape-safety rule (docs/DESIGN.md §2.9, cross-cutting finding #3)

Stated verbatim, per the finding's own instruction: inside `compute_swap`, the fee — **and
every other curve parameter** — may be a function of **storage bytes and compile-time
constants only. Never of `input_amount`.** `fee_from_storage` takes only `storage`;
`compute_swap` computes the fee once per call before touching `input`, and no other curve
parameter exists in this mechanism. This holds unchanged from the source. `step` is not
available in `compute_swap` — it is not read anywhere in that function, only in
`after_swap`'s payload (offset 34).

## Cold start and garbage-state handling (findings #4, #5)

- **Cold start.** Storage is zero-initialized at the start of every simulation. Zeroed
  `ewma_vol`/`shock_steps` read as `0`, so `fee_from_storage` returns exactly `BASE_FEE_1E9`
  before the router or arbitrageur ever fires a real trade — **no separate sentinel/magic
  branch is needed**, unlike `005` (whose estimator needed a `COLD_FEE` override during a
  warmup window). This family's cold-start value *is* its steady-state floor, so the two
  never diverge.
- **Garbage state.** `crates/cli/src/commands/validate.rs`'s randomized probe fills
  `storage[0..32]` (all four state fields) with pseudo-random bytes and calls
  `compute_swap` only (never `after_swap`). Every read in `fee_from_storage` flows through
  `saturating_mul`/`saturating_add` and a final `.min(MAX_FEE_1E9).min(FEE_HARD_MAX_1E9)`
  clamp — an arbitrary `u64` `ewma_vol` (up to `u64::MAX`) or `shock_steps` (clamped to
  `SHOCK_DECAY_STEPS` before use) can only saturate the fee at its ceiling, never panic or
  produce an out-of-range value. Confirmed by `prop-amm validate`'s "Randomized reserve/
  storage checks" passing with no arithmetic error (see § Parity gate below).

## Every error path saturates, never returns a spurious 0 (finding #6)

`compute_swap`'s output is `reserve.saturating_sub(ceil_div(k, new_reserve))` — the same
form `001`/`005` use — bounded above by the reserve by construction, so it never trips
`crates/sim/src/amm.rs`'s "quote > reserve -> 0.0" guard. For a huge `input`, `net` grows
without bound and `ceil_div(k, new_reserve) -> 0`, so the output saturates toward the full
reserve (exhaustion), never collapses to `0`. `net == 0` for `input == 1` at any fee this
family can produce is the same benign truncation-to-zero `005`'s NOTES.md already argued is
safe: `input == 1` is always the smallest nonzero input in any shape-check sample set, so
its `0` output can only sit below a larger input's output, never above one.

## Finding #7 (clamp-before-squaring / stable inversion) does not apply

`compute_swap` has no quadratic term and no square-root inversion — the output is a direct
constant-product ceiling-division, the same shape `001`/`005` already use safely. `isqrt`
does not appear anywhere in this file.

## Pre-search shape-fuzz gate (docs/DESIGN.md §2.9, WHI-1212)

`bench fuzz --strategy strategies/004-ewma-shock-decay-fee`: **PASS — zero shape
violations** across 324 states x 2 sides (dense sweeps + golden-section fair-price sample
sets, every `[grid]` regime corner in both a zeroed- and random-byte-storage variant, plus
states reached only after a full-length GBM drift). No report committed (a PASS writes
none, by design).

## Frozen parameter space (declared before any search runs — docs/DESIGN.md §2.4)

**Search 4 dimensions**, per the issue's per-strategy review amendment (in bps, matching
that table exactly — see § Fidelity self-assessment for the unit rescaling):

| Param | Range | Reason |
| --- | --- | --- |
| `BASE_BPS` | 4..=80 | source 8bps; upper bound = the opponent's own maximum fee (`norm_fee_bps <= 80`), above which the base alone out-prices every normalizer before the vol term ever loads |
| `VOL_MULT` | 0..=8 | source 2; dimensionless multiplier on `ewma_vol`, unit unchanged from the source. `0` is included deliberately — see § Containment below |
| `MAX_FEE_BPS` | 66..=400 | source's hardcoded cap was 100; extended per the review amendment because the vol term saturating at high sigma makes this cap *the entire high-sigma policy*, and 005's own grid showed a much higher fixed fee still strongly positive at `sigma=0.007`. Lower bound 66 is the 0-line (see § Containment); upper bound invented, unvalidated (issue's own words) |
| `SHOCK_FEE_PER_STEP_BPS` | 0..=10 | source 4bps; `0` is the shock ablation, which the signal-reinterpretation section above (a large retail print re-arms the shock and raises the fee right after uninformed flow — the wrong sign) makes a live possibility |

**Frozen, recorded as deliberately un-searched** (issue's own reasoning): `ALPHA_1E9`
(alpha = 0.20), `SHOCK_THRESHOLD_1E9` (0.5%), `SHOCK_DECAY_STEPS` (8) — all three have
direct source provenance and interact multiplicatively with the searched gains, so
searching them would buy little beyond what those gains already express.

### Containment (rule restated, per WHI-1209's correction to cross-cutting finding #10)

Unlike `005` (blocked by an unconditional `COLD_FEE`), **this family has no unconditional
term**: the cold-start fee *is* `BASE_BPS` itself, with no separate floor. At
`(BASE_BPS=66, VOL_MULT=0, MAX_FEE_BPS=66, SHOCK_FEE_PER_STEP_BPS=0)`, `vol_fee` and
`shock_fee` are identically zero regardless of `ewma_vol`/`shock_steps` (multiplied by
zero-valued gains), so `fee_1e9 = BASE_FEE_1E9 = 6,600,000` (66bps) **unconditionally, for
every step of every simulation** — this is **bit-exact** `001@66`, the stronger form the
restated rule prefers over `005`'s near-exact fallback.

### Demonstration

Evaluated via a scratch degenerate-range copy of this file (all four ranges collapsed to
the single point above, `bench fit --max-points 1 --no-report` — not committed, not part
of the real search, per the `bench-fit-degenerate-range-evaluates-one-exact-point`
technique):

**screening avg edge: 384.820761** — an **exact** match to `001-cpmm-fee`'s own committed
**384.82** (`strategies/001-cpmm-fee/NOTES.md`). Train/validation re-evaluation of the same
point: **train 406.144289** and **validation 401.800851**, exact matches to `001`'s own
406.14/401.80 across all three independently-sampled segments. This is the *stronger*, bit-
exact form the restated rule (WHI-1209's correction) prefers: no residual gap to predict or
explain, because no unconditional term exists in this mechanism to produce one. The state
handling and sign mapping are confirmed correct before any of the 300-point search budget
is spent.

### Search-tool note

`bench fit`'s coarse grid is generic over declared dimension count
(`tools/bench/src/search.rs::coarse_grid` — `budget^(1/n)` points per axis, half the total
budget, then coordinate descent over all `n` dimensions with the other half). For 4
declared params at the default 300-point budget this ran as roughly 3 points/axis (81 grid
points) then descent over all 4 — not the issue's own manually-sketched "grid the first 3
dims with shock fixed at its source value, then open the 4th," since the tool has no
partial-dimension grid mode. This stays inside the ≤300 budget and searches every declared
dimension, satisfying docs/DESIGN.md §2.5's actual requirement (coarse grid -> coordinate
descent, common random numbers, hard cap); it is recorded here as an implementer note, not
a deviation requiring escalation.

## Search (docs/DESIGN.md §2.5)

Run via `cargo run -p prop-amm-bench --release -- fit --strategy
strategies/004-ewma-shock-decay-fee` (`config/bench.toml`'s `[search]` budget, 300 points;
screening segment `1_000_000..=1_000_199`, common random numbers). Full curve committed at
`results/2026-08-21-fit-004-ewma-shock-decay-fee.md`.

**Converged after 137 of 300 points** (budget never exhausted); **zero invalid points** —
every evaluated parameter vector produced a valid edge, consistent with the pre-search fuzz
gate having already cleared this family across every regime corner.

**Winning point: `BASE_BPS = 34, VOL_MULT = 1, MAX_FEE_BPS = 391, SHOCK_FEE_PER_STEP_BPS =
0`.**

| Segment | n | Avg edge |
| --- | --- | --- |
| screening (search inner loop) | 200 | 426.831472 |
| train (final evaluation) | 1,000 | 451.178318 |
| validation (final evaluation) | 1,000 | 446.297129 |

Against `001-cpmm-fee`'s own committed numbers (screening 384.82, train 406.14, validation
401.80): **+42.01 screening, +45.03 train, +44.50 validation** — a consistent ~+11%
improvement across all three independently-sampled segments, landing just inside this
issue's own predicted validation range (415-455).

**`SHOCK_FEE_PER_STEP_BPS` converged to `0` — the shock ablation.** This is the live
possibility the issue's own review amendment flagged: a large retail print re-arms the
shock and raises the fee right after uninformed flow (the wrong sign), so the search
found no net benefit from the shock term at all within its frozen `0..=10` range. This is
a genuine finding about the mechanism, not a search artifact — `SHOCK_FEE_PER_STEP` is a
boundary hit at the *ablation* end, and the family's edge comes entirely from the
EWMA-vol term (`VOL_MULT=1`) and the widened `MAX_FEE_BPS` cap, not from the shock-decay
half of the mechanism its own name advertises.

**`MAX_FEE_BPS` converged to 391, near but not at its own upper bound (400).** An interior
point (not a boundary hit), so this is not a "the frozen space is too narrow" signal the
way `005`'s own `FEE_LO` boundary hit was. The evaluated curve shows the cap's value is
real but modest and diminishing in this region: at `[BASE_BPS=49, VOL_MULT=1,
SHOCK_FEE_PER_STEP_BPS=5]`, raising `MAX_FEE_BPS` from 233 to 390 gains **+4.03** screening
edge, while 390 to 400 gains essentially nothing (**-0.03**, noise-scale) — a plateau, not
a boundary the search is straining against. This is weaker evidence than a clean ablation
against the source's original 100bps cap would be: no evaluated point in the committed
curve pins `MAX_FEE_BPS <= 100` at the winning `(BASE_BPS, VOL_MULT)` region, so this
curve shows the widened cap helps somewhat over 233bps, not specifically over the source's
100bps ceiling. § Grid mode's high-sigma cells are a separate, indirect line of evidence
(the winning point vs. `001`, not an ablation of this cap specifically) and should not be
read as confirming the same claim more strongly than this direct sensitivity does.

**Compile timing:** 139 warm compiles, min=0.257s, mean=0.686s, max=2.554s during this run
— exceeds `docs/DESIGN.md` §2.6's `<1s` target on the mean and max samples. Consistent with
the standing conclusion `strategies/001-cpmm-fee/NOTES.md` and WHI-1205 already reached (a
session/system-load effect, not a fast-path regression) — not re-investigated here for the
same reason `005`'s NOTES.md gave: a single run cannot establish a fresh recurrence
independent of load.

## Grid mode: 27-cell fragility matrix (docs/DESIGN.md §2.3)

`cargo run -p prop-amm-bench --release -- grid --candidate
strategies/004-ewma-shock-decay-fee/lib.rs --reference strategies/001-cpmm-fee/lib.rs` —
reference is `001-cpmm-fee` (the 0-line every M1 candidate is measured against,
docs/DESIGN.md §2.8), not `000-normalizer`. Full table committed at
`results/2026-08-21-grid-004-ewma-shock-decay-fee.md`.

| cell | fee (bps) | liq mult | sigma | candidate | reference | mean diff |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | 30 | 0.4 | 0.0001 | 807.33 | 710.96 | **+96.37** |
| 1 | 30 | 0.4 | 0.0010 | 786.76 | 695.34 | **+91.42** |
| 2 | 30 | 0.4 | 0.0070 | 443.25 | 197.26 | **+245.98** |
| 3 | 30 | 1.0 | 0.0001 | 420.19 | 389.11 | **+31.08** |
| 4 | 30 | 1.0 | 0.0010 | 415.11 | 382.63 | **+32.48** |
| 5 | 30 | 1.0 | 0.0070 | -16.86 | -128.96 | **+112.10** |
| 6 | 30 | 2.0 | 0.0001 | 203.18 | 205.57 | -2.38 (CI includes 0) |
| 7 | 30 | 2.0 | 0.0010 | 206.78 | 192.54 | **+14.24** |
| 8 | 30 | 2.0 | 0.0070 | -211.90 | -288.26 | **+76.36** |
| 9 | 55 | 0.4 | 0.0001 | 934.93 | 842.83 | **+92.10** |
| 10 | 55 | 0.4 | 0.0010 | 921.37 | 836.89 | **+84.48** |
| 11 | 55 | 0.4 | 0.0070 | 477.31 | 278.70 | **+198.61** |
| 12 | 55 | 1.0 | 0.0001 | 582.74 | 547.15 | **+35.59** |
| 13 | 55 | 1.0 | 0.0010 | 586.75 | 546.96 | **+39.79** |
| 14 | 55 | 1.0 | 0.0070 | 113.27 | 32.09 | **+81.18** |
| 15 | 55 | 2.0 | 0.0001 | 375.64 | 348.19 | **+27.45** |
| 16 | 55 | 2.0 | 0.0010 | 380.87 | 341.02 | **+39.85** |
| 17 | 55 | 2.0 | 0.0070 | -124.60 | -159.57 | **+34.97** |
| 18 | 80 | 0.4 | 0.0001 | 1109.35 | 1030.48 | **+78.87** |
| 19 | 80 | 0.4 | 0.0010 | 1099.83 | 1018.67 | **+81.16** |
| 20 | 80 | 0.4 | 0.0070 | 665.84 | 483.73 | **+182.12** |
| 21 | 80 | 1.0 | 0.0001 | 821.12 | 838.09 | **-16.97** |
| 22 | 80 | 1.0 | 0.0010 | 824.36 | 838.72 | **-14.36** |
| 23 | 80 | 1.0 | 0.0070 | 346.91 | 315.55 | **+31.36** |
| 24 | 80 | 2.0 | 0.0001 | 637.06 | 668.09 | **-31.02** |
| 25 | 80 | 2.0 | 0.0010 | 636.87 | 665.90 | **-29.03** |
| 26 | 80 | 2.0 | 0.0070 | 110.82 | 152.43 | **-41.62** |

21 of 27 cells favor `004` significantly (up to +245.98 at cell 2); 5 favor `001`
significantly (bold, negative — cells 21/22/24/25/26); cell 6's 95% CI includes 0 (a
statistically insignificant tie, not a real effect). `004`'s significant-win count (21/27)
is well above `005`'s own 17/27 against the same reference.

### Explaining the negative cells

**Every significant negative cell is `norm_fee_bps=80` and `norm_liquidity_mult >= 1.0`**
(cells 21/22/24/25/26) — the same pattern `005`'s NOTES.md documented against the same
reference. At `norm_fee_bps=80` the opponent is already expensive and non-thin, so flow
routes to whichever venue is priced competitively *regardless* of which one is running —
flow-share is not the deciding factor there. What differs is **edge captured per unit of
that already-won flow**: `001`'s flat 66bps charges a fixed, comfortable margin on every
trade it wins; `004`'s fitted `BASE_BPS=34` is *below* 66bps, so whenever the vol term
hasn't ramped up much (calm-to-mid sigma, cells 21/22/24/25), winning the same flow at a
thinner margin nets strictly less edge than `001`'s flat charge.

**Cell 23 is the exception that proves this**: same `norm_fee_bps=80, liq=1.0` pairing,
but at `sigma=0.007` it flips to **+31.36**. At high sigma the EWMA-vol term ramps the fee
well above the flat 66bps floor (the fitted formula's `VOL_MULT=1` term scales directly
with the estimated relative price move, and `MAX_FEE_BPS=391` leaves ample headroom for
that ramp), so the LVR-defense benefit of pricing higher outweighs the margin-per-trade
loss the calmer cells show. **Cell 26 (`fee=80, liq=2.0, sigma=0.007`, -41.62) is the single
worst cell and the only high-sigma cell that stays negative** — the toughest opponent
configuration in the whole grid (cheapest *and* deepest simultaneously), where even the
ramped-up adaptive fee cannot fully offset the margin-per-trade disadvantage against a
non-thin competitor. This mirrors `005`'s own worst cell (also cell 26, same reference,
same reasoning) despite the two families using unrelated estimators — the effect is a
property of the grid's own toughest corner, not of either candidate's specific mechanism.

**Cell 6** (`fee=30, liq=2.0, sigma=0.0001`, -2.38, CI includes 0) is noise, not a real
effect — the smallest-magnitude cell in the table and statistically indistinguishable from
a tie.

None of this is disqualifying: `004` beats `001` on every aggregate decision-input segment
(screening/train/validation, all +42 to +45) by a wider margin than `005` achieved, and
grid mode is explicitly not a ranking input (docs/DESIGN.md §2.3) — it identifies where the
fitted point is fragile, which is exactly the same fragility `005`'s own port already
documented against the same reference and cells.

## Compute units (docs/DESIGN.md §2.9, cross-cutting finding #9)

No bench tooling exposes measured CU headroom today (same gap `005`'s NOTES.md recorded —
`crates/executor` is upstream-owned, so exposing `SyscallContext::get_remaining()` is an
upstream-sync-lane change, not something to add inside this single-strategy issue).

Division-count estimate instead, since finding #9 itself names division count (not
instruction count) as the thing to bound on SBF: this mechanism is markedly cheaper than
`005`'s (no `isqrt` loop). `fee_from_storage` does **zero** runtime divisions — every
`_1E9` constant it reads is a compile-time-evaluated `const` expression, and the fee
formula itself is pure saturating multiply/add/min. `compute_swap`'s output path
(`net = input * keep / 1_000_000_000`, then `ceil_div`) is **2 divisions**. `after_swap`'s
update (`price_change_1e9`'s `checked_div`, then `new_vol`'s `/ 1_000_000_000`) is **2
divisions**. Worst case per instruction call is therefore **~2 divisions**, on par with
`001-cpmm-fee`'s own 2 and roughly an order of magnitude cheaper than `005`'s ~25. At
finding #9's own cited worst-case cost (10^2-10^3 CU/division on SBF), that is a worst-case
estimate of roughly 200-2,000 CU, comfortably inside the 100,000 CU limit. Corroborated
empirically: `prop-amm validate`'s native/BPF parity check and `bench parity` below both
execute the real BPF program repeatedly with no compute-budget failure.

## Parity gate (docs/DESIGN.md §2.6)

Reproduced via `cargo run -p prop-amm-bench --release -- parity --strategy
strategies/004-ewma-shock-decay-fee` against the committed `(BASE_BPS=34, VOL_MULT=1,
MAX_FEE_BPS=391, SHOCK_FEE_PER_STEP_BPS=0)` point: `prop-amm validate` passes; the fast
path and the reference path agree on **all 1,000 `observation`-segment seeds to `0`
relative difference** (well inside the `1e-9` gate); the fast-path aggregate (avg edge
443.75) matches `prop-amm run`'s own 2-decimal output exactly (443.75, total 443746.27).
See `results/2026-08-21-parity-004-ewma-shock-decay-fee.md` for the committed snapshot.

**Leaderboard-comparable number: avg edge 443.75** (`observation` segment, seeds `0..=999`,
native), against `001-cpmm-fee`'s own **399.97** on the same segment — a **+43.78 (+10.9%)**
improvement, and against `005-vol-adaptive-cpmm-fee`'s own **422.93** on the same segment —
a further **+20.82 (+4.9%)** improvement, making `004` the current leader among ranked M1
candidates on this measure (docs/DESIGN.md §2.10 ranks on validation, not this observation
row — see § Search above for the validation number the actual ranking will use: 446.30).

## Verification (docs/DESIGN.md §4.4 acceptance criterion, `AGENTS.md`)

- `cargo test --workspace`: **214 passed, 0 failed, 1 ignored** (no regression against the
  green baseline `AGENTS.md` records).
- `rustfmt` applied to `strategies/004-ewma-shock-decay-fee/lib.rs` (the only touched
  source file); `cargo fmt --all` deliberately not run, per the repo's own caveat that it
  reformats inherited upstream files outside this issue's scope.
- No new `clippy`-worthy issues in the touched files; `strategies/**` is outside the cargo
  workspace (`Cargo.toml`'s `members`/`exclude` lists), so `cargo clippy --workspace` does
  not lint it directly, matching `001`/`005`'s own precedent.

## Note on the environmental build workaround used while producing this record

`prop-amm validate`/`run` and `bench grid`/`parity` (which shell out to the reference
compile path, `crates/cli/src/commands/compile.rs::ensure_build_dir`) cannot run from
inside this issue's own `.claude/worktrees/` checkout — cargo's workspace-ancestor walk
reaches the primary clone's root `Cargo.toml` instead of stopping at the nested worktree
(a pre-existing, environmental limitation, not something this port introduced or worked
around by changing any committed source). Those specific commands were run from a
detached scratch worktree outside the primary clone's directory tree and their
`results/*.md` output copied back here; `bench fit`/`fuzz` (whose own fast-path build
directory already carries an empty `[workspace]` table, WHI-1205) ran directly in this
worktree with no workaround needed.
