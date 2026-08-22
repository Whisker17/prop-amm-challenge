# 004b-floor-subtracted-ewma-fee

## Provenance

A `docs/DESIGN.md` §2.9 variant of `strategies/004-ewma-shock-decay-fee` (WHI-1223), not a
fresh external port — no new `docs/references/` entry is needed; the parent's own
`docs/references/004-ewma-shock-decay-fee/` remains the mechanism's ultimate source. The
issue's own thesis: replace the parent's plain `ewma_vol * VOL_MULT` vol-fee term with a
floor-subtracted, finer-gain residual (`max(ewma_vol - FLOOR, 0) * VOL_MULT_NUM /
VOL_MULT_DEN`), decoupling the calm-regime fee level from the high-sigma slope. Per §2.9
this is a variant, not a new strategy: same estimator, same storage layout, same state
update (`after_swap` untouched) — only the state-to-fee map's shape changes.

## Objective (WHI-1223's own framing)

The parent's fitted point (`BASE_BPS=34, VOL_MULT=1, MAX_FEE_BPS=391`) is coupled: a slope
steep enough to reach the high-sigma optimum (>=500bps, per the cross-report comparison the
issue's own Objective cites) would also raise the calm-regime fee well above the 0-line's
66bps, collapsing the aggregate. Subtracting a floor before applying the slope was intended
to let the calm fee refit toward 66 independently of a much steeper high-sigma slope. The
issue named this the only one of three candidate 004-boundary follow-ups with a credible
path past the parent's own 446.30 validation, and explicitly ruled out the other two
(extending `MAX_FEE_BPS` alone; repairing the shock layer) as measured dead or out of scope
for a single variant slot — see the issue body for that analysis; not re-derived here.

## Fidelity self-assessment (docs/DESIGN.md §2.9)

**Minimum changes only**, relative to the parent:

- `after_swap` is **byte-identical** to the parent's — same storage layout, same EWMA
  update, same shock re-arm/decay rule. The shock counter keeps ticking harmlessly even
  though its fee contribution is frozen at 0, which is what keeps the state trajectory (and
  therefore containment, below) bit-exact.
- `compute_swap`, the `ComputeSwapInstruction` schema, the byte-offset helpers, and the
  constant-product output shape are unchanged from the parent.
- The one mechanism change is inside `fee_from_storage`: the parent's
  `ewma_vol.saturating_mul(VOL_MULT)` becomes a floor-subtracted residual computed in
  `u128`:
  ```rust
  let residual = (ewma_vol as u128).saturating_sub(FLOOR_1E9 as u128);
  let vol_fee = residual * VOL_MULT_NUM as u128 / VOL_MULT_DEN as u128;
  ```
  `VOL_MULT` (a single multiplier, range `0..=8`) is replaced by `VOL_MULT_NUM/VOL_MULT_DEN`
  (`DEN` frozen at `8`, `NUM` range `0..=64`) — the same real slope interval the parent
  froze, just quantized in `1/8` steps instead of whole numbers, so `VOL_MULT_NUM=8` is
  exactly the parent's `VOL_MULT=1`.
- `MODEL_USED` is **not** preserved from the parent's constant. Unlike the parent (a
  faithful port of `lilaclilac09/pamm-a`'s own submission, where the mechanism is untouched
  and the field names the model that wrote the *original* source), this variant's mechanism
  is genuinely different — a new state-to-fee map authored for this issue — so `MODEL_USED`
  names the model that wrote *this* variant (`"Claude Sonnet 5"`), the same convention
  `003-piecewise-linear`/`006-hedged-pnl`/`007-dodo-pmm` already established for
  from-scratch or substantially-adapted mechanisms.
- `SHOCK_FEE_PER_STEP_BPS` is frozen at `0` **outside** the `PARAMS` block (a plain const,
  not searched) — the parent's own fit already ablated this term to `0` (screening
  `-1.81` at the winner; `+19.4` from `5 -> 0` at a nearby point), and a repair would
  require classifying a print as arb- vs retail-driven, which needs the fair price — the
  unobservable this estimator exists to infer. That's a new signal, i.e. a new strategy
  family, not a variant (the issue's own reasoning; not re-derived here).

## Cold start and garbage-state handling (inherited from the parent, findings #4, #5)

Unchanged in kind from the parent: zero-initialized storage reads `ewma_vol = 0`, so at the
committed point (`FLOOR_1E9 = 0`) the residual is also `0` and `fee = BASE_FEE_1E9` with no
separate sentinel branch. For garbage-state random bytes
(`crates/cli/src/commands/validate.rs`'s randomized probe): `residual`'s `saturating_sub`
never underflows; `vol_fee`'s multiply/divide and the final three-term sum are plain `u128`
arithmetic (not saturating) that relies on headroom rather than clamping — the largest
possible residual (`u64::MAX`) times the largest possible `VOL_MULT_NUM` (`64`) is
~1.18e21, comfortably inside `u128`'s ~3.4e38 ceiling, so nothing overflows before the final
`.min(MAX_FEE_1E9)` clamp bounds the result. Of the whole formula, only two operations are
actually saturating: `residual`'s `saturating_sub` and `shock_fee`'s
`shock_steps.saturating_mul` (the latter inherited unchanged from the parent) — `vol_fee`'s
multiply/divide and the final three-term sum are the plain, headroom-bounded arithmetic
described above. `bench fuzz` (below) confirms this empirically, the same way it did for
the parent.

## Shape-safety rule (docs/DESIGN.md §2.9, cross-cutting finding #3) — unchanged

`fee_from_storage` still reads only `storage`, never `input_amount`. The new residual
computation is itself a function of `ewma_vol` (state) and compile-time constants only — a
ReLU on *state*, never on the input — so no kink enters any quoted curve and every quote is
still one fixed-fee CPMM, which `curve_checks.rs` provably accepts.

## Pre-search shape-fuzz gate (docs/DESIGN.md §2.9, WHI-1212)

`bench fuzz --strategy strategies/004b-floor-subtracted-ewma-fee`: **PASS — zero shape
violations** across 324 states x 2 sides (dense sweeps + golden-section fair-price sample
sets, every `[grid]` regime corner in both a zeroed- and random-byte-storage variant, plus
states reached only after a full-length GBM drift). No report committed (a PASS writes
none, by design).

## Frozen parameter space (declared before any search runs — docs/DESIGN.md §2.4)

Per the issue's own review-amendment table, verbatim:

| Param | Range | Bound reasoning |
| --- | --- | --- |
| `BASE_BPS` | 4..=80 | inherited verbatim from the parent; upper = the opponent's max `norm_fee_bps`. Both bounds anchored. |
| `VOL_MULT_NUM` (DEN=8) | 0..=64 | slope in `[0, 8]` in 1/8 steps — the same real interval the parent froze, finer quantization only. `0` makes the family contain the 0-line. |
| `FLOOR_BPS` | 0..=60 | `0` is required for bit-exact parent containment. The physical floor is 12-28bps/trade (`retail_mean_size / INITIAL_Y`), inflated toward 20-40 by size dispersion; 60 is ~2x the top estimate. Upper bound invented, unvalidated (issue's own words). |
| `MAX_FEE_BPS` | 66..=500 | lower = the 0-line. Upper = the starter anchor: 500 is the largest fee for which any committed measurement of strong high-sigma performance exists. |

**Frozen outside the block, not independent parameters:** `VOL_MULT_DEN = 8` (a
quantization choice, a power of two so `NUM/DEN` lowers to a shift) and
`SHOCK_FEE_PER_STEP_BPS = 0` (the parent's own ablation, see § Fidelity self-assessment).
Also frozen at the parent's own source anchors: `ALPHA_1E9` (0.20), `SHOCK_THRESHOLD_1E9`
(0.5%), `SHOCK_DECAY_STEPS` (8).

**`MAX_FEE_BPS`'s upper bound widens relative to the parent's own frozen range** (the
parent froze `66..=400`; this variant freezes `66..=500`), which is results-informed in the
sense that the issue's own reasoning for the new bound cites the parent's committed fit
curve (the cap converged to 391, an interior point, not a boundary hit — `strategies/004-
ewma-shock-decay-fee/NOTES.md` § Search) to argue 500 is a safe, still-anchored ceiling
(the largest fee with any committed strong-high-sigma measurement, per the issue's Objective
table above). This is legitimate as this variant's own newly-declared frozen space (§2.4
requires freezing *before this issue's own search*, which this range does), not a
retroactive widening of the parent's already-closed search. The new headroom was not left
unexercised, either: P1 below reaches all the way to `MAX_FEE_BPS=500` (the new upper
bound) and P2 to `450` — both above the parent's old `400` ceiling — so the two
pre-registered probes that trigger the kill rule are themselves direct evidence the widened
range doesn't help; the 300-point coordinate-descent *search* is what never ran, not the
widened space itself.

Budget: 4 dims, 300 points -> coarse grid 3 levels/axis = 81 points, ~219 for descent (never
spent — see § Negative result below).

### Containment (bit-exact, twice — per WHI-1209's correction to cross-cutting finding #10)

Evaluated via the scratch degenerate-range method (`bench-fit-degenerate-range-evaluates-
one-exact-point` technique: a throwaway copy with every PARAMS range collapsed to
`MIN==MAX`, `bench fit --max-points 1 --no-report`, never committed). Every number in this
section and in § Pre-registered stop rules below uses the `screening` segment (n=200) or
the `train`/`validation` segments (n=1,000 each, `config/bench.toml`), 10,000 steps
(`BASELINE_STEPS`), the native fast-compile path, run against the repo at commit `2d32177`
(`origin/dev` tip) — the technique is inherently pre-commit (a throwaway scratch copy is
evaluated before this issue's own `strategies/004b-floor-subtracted-ewma-fee/lib.rs` exists
as a committed file, so no `results/*.md` report is written or possible for these points;
`bench fit --no-report` and a `bench fuzz` PASS are both designed to write none, per
docs/DESIGN.md §2.9/§2.5).

**P0 — the parent's fitted point, `(BASE_BPS=34, VOL_MULT_NUM=8, FLOOR_BPS=0,
MAX_FEE_BPS=391)`:** at `FLOOR_1E9=0` the `saturating_sub` is an identity and
`VOL_MULT_NUM/VOL_MULT_DEN = 8/8 = 1` is exact in `u128` for every `u64` input, so this
formula computes exactly the parent's `ewma_vol * 1`; `after_swap` is unchanged, so the
state trajectory is identical.

| Segment | n | 004b (P0) | 004 (committed) | match |
| --- | --- | --- | --- | --- |
| screening | 200 | 426.831472 | 426.831472 | exact |
| train | 1,000 | 451.178318 | 451.178318 | exact |
| validation | 1,000 | 446.297129 | 446.297129 | exact |

**Exact match on all three independently-sampled segments** — the plumbing is correct
before any of the stop-rule probes below are trusted.

**The 0-line, `(66, 0, 0, 66)`:** `VOL_MULT_NUM=0` zeroes `vol_fee` unconditionally
(regardless of `ewma_vol`), and `MAX_FEE_BPS=66=BASE_BPS` clamps the fee at exactly 66bps
for every step — bit-exact `001-cpmm-fee@66`.

| Segment | n | 004b (0-line) | 001-cpmm-fee (committed) | match |
| --- | --- | --- | --- | --- |
| screening | 200 | 384.820761 | 384.82 | exact |
| train | 1,000 | 406.144289 | 406.14 | exact |
| validation | 1,000 | 401.800851 | 401.80 | exact |

Downside is therefore bounded at a tie with the parent's own **446.30** validation, not
merely with `001`'s 401.80.

## Pre-registered stop rules — the bounded probe (run before any search, per the issue's own protocol)

Following `007-dodo-pmm`'s own "Step 0.5" precedent: three points evaluated on the 200
screening seeds via the same degenerate-range method, **all explicitly outside the
300-point search budget**:

| Point | `(BASE, NUM, FLOOR, MAX)` | screening avg edge | vs. parent's 426.83 |
| --- | --- | --- | --- |
| P0 (plumbing) | (34, 8, 0, 391) | 426.831472 | 0.00 (exact containment, see above) |
| P1 (thesis) | (66, 40, 30, 500) | 325.846366 | **-100.99** |
| P2 (hedged) | (55, 24, 25, 450) | 412.704830 | **-14.13** |

*Stop rule* — pre-registered in the issue: "if `max(P1, P2) < 428.8` (parent's screening
426.83 + 2.0, i.e. 2-4x within-family screening jitter), the decoupling thesis is dead —
close the lane as a pre-registered negative without spending the 300-point budget or a
review cycle."

`max(P1, P2) = max(325.846366, 412.704830) = 412.704830 < 428.8` — **the kill rule
triggers.** No 300-point search was run; 0 of the 300-point budget was spent.

Train/validation re-evaluation of the same two points, for completeness (not decision
inputs — screening is what the stop rule is defined on):

| Point | train | validation |
| --- | --- | --- |
| P1 | 346.303968 | 343.558219 |
| P2 | 439.177053 | 435.892684 |

Both confirm the screening-scale verdict: P1 is a large, unambiguous loss; P2 is a smaller
but still clear loss relative to the parent's 451.18/446.30.

## Negative result — the decoupling thesis does not pay in this harness

**The floor-subtraction mechanism, as specified, does not decouple the regimes
productively at either tested combination.** This is the "clean negative result" the
issue's own Prediction section named as a live (~30%) possibility, though the specific
outcome — a *loss* at both P1 and P2, not a tie — is stronger than the ~30%-probability
"collapses to `FLOOR~0, NUM~8`" scenario it described.

**Plausible mechanism (not independently measured here — the actual evidence is the P0/P1/P2
numbers above, which do carry provenance; this is offered only as a candidate explanation,
not a verified one):** P1 pushes `BASE_BPS` to 66 (the 0-line's own value) and relies
entirely on the residual term to earn back the calm-regime cost via the high-sigma cap.
`FLOOR_BPS=30` sits inside the issue's own estimated physical-floor range (12-28bps/trade,
inflated toward 20-40 by size dispersion — § Frozen parameter space above), i.e.
deliberately close to where the sigma-independent, retail-driven impact floor is expected to
sit. If `ewma_vol` spends a meaningful share of steps at or below that floor even outside
the rare high-sigma tail, the residual is zeroed there and the fee is pinned at
`BASE_BPS=66` — strictly worse than the parent's `34 + small vol_fee` in the calm cells
where the parent already loses to `001@66` (cells 21/22/24/25 in the parent's own grid), and
no better in the cells where the parent wins comfortably at a much lower base. P2's milder
floor (25bps) and lower base (55bps) is a smaller loss but still not a win, consistent with
the same mechanism operating less aggressively. Neither this session nor the cited NOTES.md
files contain a direct measurement of `ewma_vol`'s time-distribution across sigma regimes,
so this account should be read as a hypothesis consistent with the measured P1/P2 losses,
not as an independently confirmed causal chain.

**Addendum (2026-08-22, WHI-1225): this is now a direct measurement, substantiating (not yet
independently confirming) the hypothesis above.** `WHI-1225`'s own Probe A (`bench
estimator-probe`, `strategies/005b-elapsed-steps-divisor-fix/NOTES.md` § Added scope)
replicates `ewma_vol` from a real run and records its trade- and volume-weighted distribution
against a floor sweep, bucketed by true-sigma tercile (`ewma_vol` updates on every executed
submission trade, with no per-simulation-step dedup, so this is a fraction of trades, not of
simulation steps). Result: even in the **Low** sigma tercile (the calmest third of the sampled
range, not an extreme tail), `ewma_vol` sits at or below 25 bps on **56.2%** of executed trades
(30.7% of volume) and at or below 30 bps on **70.3%** of trades (44.1% of volume) — exactly the
two floors P2 and P1 used above. This is now a directly measured distribution, not just a
plausible-sounding account — it shows `ewma_vol` genuinely does spend a majority of calm-regime
trades at or below the floors P1/P2 used, the necessary premise for the proposed causal chain.
It is not, on its own, sufficient proof that the residual-zeroing this measurement makes
possible is *the* reason both probe points lost (that would need the residual's own
contribution to the fee, and the specific loss cells, traced through) — but it substantially
strengthens the account beyond "a hypothesis consistent with the measured losses." Full table:
`results/2026-08-22-estimator-probe-005b.md`.

**The committed value is the containment point, `(BASE_BPS=34, VOL_MULT_NUM=8,
FLOOR_BPS=0, MAX_FEE_BPS=391)` — a bit-exact tie with the parent, not an interior optimum.**
Per the `WHI-1209` boundary-hit precedent this issue's own Prediction section names
(reused by `007-dodo-pmm` for the same situation): presented here as the point the
pre-registered stop rule closes the family at, not as a fitted result of the (unrun)
300-point search. Confirmed independently via `bench compare` (below): the paired mean
difference against the parent is **exactly 0.000000** (95% CI `[0.000000, 0.000000]`,
n=1,000, and identically `0.000000` in every one of the 27 regime-slice bins) — the
strongest form containment can take, stronger than a merely-tied point that happens to
average out.

`bench compare --candidate strategies/004b-floor-subtracted-ewma-fee/lib.rs --reference
strategies/004-ewma-shock-decay-fee/lib.rs`: full report at
`results/2026-08-22-compare-004b-floor-subtracted-ewma-fee-vs-004-ewma-shock-decay-fee.md`.

## Grid mode: 27-cell fragility matrix (docs/DESIGN.md §2.3)

`cargo run -p prop-amm-bench --release -- grid --candidate
strategies/004b-floor-subtracted-ewma-fee/lib.rs --reference strategies/001-cpmm-fee/lib.rs`
— reference is `001-cpmm-fee`, matching the parent's own grid setup. Full table:
`results/2026-08-22-grid-004b-floor-subtracted-ewma-fee.md`.

Because the committed point is a bit-exact tie with the parent, **every one of the 27
cells reproduces the parent's own committed grid numbers exactly** (`results/2026-08-21-
grid-004-ewma-shock-decay-fee.md`) — cross-checked directly against several cells,
including the two the issue's own Prediction section named as noteworthy:

- **Cell 23** (`fee=80, liq=1.0, sigma=0.0070`): candidate 346.91 vs reference 315.55,
  **+31.36** — the parent's own regression-risk cell, unchanged: still a win here (not the
  regression the issue flagged as a risk for a *successfully decoupled* variant, since this
  variant ties the parent rather than shifting its fee curve).
- **Cell 26** (`fee=80, liq=2.0, sigma=0.0070`): candidate 110.82 vs reference 152.43,
  **-41.62** — the worst cell in the table, unchanged from the parent's own worst cell, for
  the same reason the parent's NOTES.md gives: the toughest opponent configuration in the
  whole grid (cheapest *and* deepest simultaneously), a property of the grid's own
  toughest corner rather than of either candidate's specific mechanism (the parent's
  sibling `005-vol-adaptive-cpmm-fee` hits the same worst cell against the same
  reference).

No new fragility is introduced or removed by this variant, since it is mechanically
identical to the parent at the committed point.

## Compute units (docs/DESIGN.md §2.9, cross-cutting finding #9)

Division-count estimate, same method the parent's NOTES.md used (no bench tooling exposes
measured CU headroom; `crates/executor` is upstream-owned). `fee_from_storage` now contains
one `/` operator (`residual * VOL_MULT_NUM / VOL_MULT_DEN`) that the parent's pure
saturating-multiply formula didn't have — but `VOL_MULT_DEN` is a compile-time constant
power of two (`8`), which a release-mode compiler lowers to a right-shift rather than an
integer-division instruction (the same claim the issue's own Shape and CU risk section
makes), so this adds **no new runtime division call** in practice. `compute_swap`'s output
path (`net = input * keep / 1_000_000_000`, then `ceil_div`) is unchanged at **2 divisions**;
`after_swap` (byte-identical to the parent) is unchanged at **2 divisions**. Worst case per
instruction call stays at the parent's **~2 divisions**, on par with `001-cpmm-fee` and
roughly an order of magnitude cheaper than `005-vol-adaptive-cpmm-fee`'s ~25. Corroborated
empirically: `bench parity` (below) executes the real BPF program repeatedly with no
compute-budget failure.

## Parity gate (docs/DESIGN.md §2.6)

`cargo run -p prop-amm-bench --release -- parity --strategy
strategies/004b-floor-subtracted-ewma-fee`: `prop-amm validate` passes; the fast path and
the reference path agree on **all 1,000 `observation`-segment seeds to `0` relative
difference** (well inside the `1e-9` gate); the fast-path aggregate (avg edge 443.75)
matches `prop-amm run`'s own 2-decimal output exactly (443.75, total 443746.27) — bit-exact
with the parent's own committed observation number. See
`results/2026-08-22-parity-004b-floor-subtracted-ewma-fee.md` for the committed snapshot.

**Leaderboard-comparable number: avg edge 443.75** (`observation` segment, seeds `0..=999`,
native) — identical to `004-ewma-shock-decay-fee`'s own **443.75** on the same segment (a
tie, not an improvement), and a **+43.78 (+10.9%)** improvement over `001-cpmm-fee`'s own
**399.97** on the same segment (docs/DESIGN.md §2.10 ranks on validation, not this
observation row — see § Containment above for the validation number: 446.297129,
identical to the parent's).

## Verification (docs/DESIGN.md §4.4 acceptance criterion, `AGENTS.md`)

- `cargo test --workspace`: **214 passed, 0 failed, 1 ignored** — no regression against the
  green baseline `AGENTS.md` records (identical count to the parent's own verification run;
  `strategies/**` adds no workspace tests, same precedent `001`/`004`/`005`/`007` set).
- `rustfmt` applied to `strategies/004b-floor-subtracted-ewma-fee/lib.rs` (the only touched
  source file); `cargo fmt --all` deliberately not run, per the repo's own caveat that it
  reformats inherited upstream files outside this issue's scope.
- No new `clippy`-worthy issues in the touched file; `strategies/**` is outside the cargo
  workspace (`Cargo.toml`'s `members`/`exclude` lists), so `cargo clippy --workspace` does
  not lint it directly, matching `001`/`004`/`005`/`007`'s own precedent.

## Note on the environmental build workaround used while producing this record

Same limitation the parent's NOTES.md documented: `prop-amm validate`/`run` and `bench
grid`/`parity`/`compare` (which shell out to the reference compile path,
`crates/cli/src/commands/compile.rs::ensure_build_dir`) cannot run from inside this issue's
own `.claude/worktrees/` checkout — cargo's workspace-ancestor walk reaches the primary
clone's root `Cargo.toml` instead of stopping at the nested worktree. Those specific
commands were run from a detached scratch worktree outside the primary clone's directory
tree (created fresh from the commit that actually contains this strategy's source, so the
committed reports' `Commit:` stamp is genuinely traceable rather than pointing at the
pre-port parent commit) and their `results/*.md` output copied back here; `bench fit`/
`fuzz` (whose own fast-path build directory already carries an empty `[workspace]` table,
WHI-1205) ran directly in this worktree with no workaround needed.
