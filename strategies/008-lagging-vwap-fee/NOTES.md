# 008-lagging-vwap-fee

## Provenance

Source form: **source (Rust, direct `pinocchio` submission shape)** —
`houseofjiao/prop-amm-challenge`'s own `strategy.rs`, pinned at
`152697153d93baf0ad82a5bac6c485bf17697b7c` (2026-02-20, verified via the GitHub API), 776
lines (`docs/references/008-lagging-vwap-fee/README.md` for the full provenance chain).

**This is another live competitor's current competition submission, with no LICENSE on
that repo or upstream** (`license: null` on both, verified via the GitHub API) — unlike
`004` (a competitor's *past* submission) or `007` (a *licensed* third-party library).
WHI-1236 Step 0 gates this explicitly:

- **(a) Copy for internal analysis: APPROVED** (owner decision recorded in WHI-1236,
  2026-08-22; reconfirmed explicitly in the implementing session before any file was
  copied or any measurement was run).
- **(b) Verbatim submission of the artifact to the challenge: left undecided in the issue,
  and treated as a standing "no" for this issue's lifetime in the implementing session.**
  This directory, this port, and every number below exist for internal M1 comparison only.

**No number from the source repository is cited as evidence anywhere in this file** —
their own headline (non-citable) was measured on a pre-2026-02-16 harness vintage, one day
before upstream's arb-direction fix (`97a1c67` et al., 2026-02-13). Every number in this
file is this project's own measurement, under the current harness, starting from the
pre-registered probe below.

Unlike `007` (a Solidity library requiring a from-scratch derivation port), the source here
already targets this exact ABI: `crates/shared/src/instruction.rs`'s
`compute_swap`/`after_swap` byte layouts and `crates/submission-sdk`'s
`set_return_data_u64`/`set_return_data_bytes`/`set_storage` are used identically by the
source and by every strategy already in this repository. This is therefore a **near-verbatim
port**, not a re-derivation — see § Fidelity self-assessment below for the machine-verified
diff proving exactly how verbatim.

## Mechanism

Confirmed directly from `strategy.rs` (not from memory); the full derivation is carried
forward into `lib.rs`'s own header comment. Summary:

- **Reference price.** A slow, Kalman-filtered VWAP `p_hat`, blended with a faster EMA
  `p_hat_fast` under high shock (`p_ref = (1-w)*p_hat + w*p_hat_fast`, `w = min(shock_hat,
  0.3)`), stands in for the fair price the interface does not supply. The lag is load-bearing
  by the source's own measurement (`LEARNINGS.md` § "Signed Flow Price Estimation": making
  the estimate more accurate *reduces* edge, because `dev^2 = (spot - p_ref)^2` **is** the
  fee signal — cited here as a third-party observation, not as evidence for this harness).
- **Directional classification + fee.** Each quote is classified by whether it would push
  `spot` further from `p_ref` (presumptively informed — `+arb_k*dev^2` surcharge) or toward
  it (presumptively uninformed — a risk-gated `-counter_k*(...)*dev^2` rebate).
- **Fee ratchet.** `after_swap` accumulates a `fee_floor` from realized
  volatility/toxicity/shock/size targets (`P0_BASE_FEE`/`P0_VOL_MULT`/etc.), decaying between
  steps; `compute_swap` reads it per-quote.
- **Profile ensemble.** Three fixed parameter tuples (`P0` balanced, `P1` max-defense, `P2`
  loss-minimizer) are scored on every executed trade via a counterfactual-edge EMA (with a
  route bonus for counter-direction flow), and the active profile switches under hysteresis
  (3-step cooldown, asymmetric gaps). Only `P0`'s constants are exposed to search (below);
  `after_swap`'s fee-ratchet target formula also only ever reads `P0`'s own constants, per
  the source's own comment — `P1`/`P2`'s `BASE_FEE`/`VOL_MULT`/etc. sets are dead code in the
  source itself.

## Fidelity self-assessment (docs/DESIGN.md §2.9)

This **is**, to the extent the fidelity contract allows, a byte-for-byte port — verified
mechanically, not just asserted. A normalized diff (comments stripped, whitespace
collapsed, tokenized) between `docs/references/008-lagging-vwap-fee/strategy.rs` and
`lib.rs` shows **every** difference falls into one of four permitted categories:

1. **`NAME`/`MODEL_USED` interface** (§2.9's explicitly-permitted minimum change): `NAME`
   is this project's own registry label ("008 Lagging-VWAP Directional Fee"), not the
   source's own submission name (WHI-1236 Step 0(b) leaves verbatim submission of the
   artifact undecided — this project's registry entry is never presented as the source
   author's own work). `MODEL_USED` ("Opus 4.6") is preserved unchanged, since the
   mechanism itself is unchanged (same pattern `004`'s own NOTES.md documents for its own
   `MODEL_USED` preservation).
2. **`#[cfg(not(feature = "no-entrypoint"))]`** added before `entrypoint!` — this
   repository's own build convention (every strategy in `strategies/` carries it; the
   source, targeting a standalone binary, did not need it).
3. **The four `PARAMS` constants hoisted into a `// === PARAMS BEGIN/END ===` block**
   (`ARB_K_BPS`, `COUNTER_K_BPS`, `TARGET_BASE_BPS`, `SIZE_K_BPS`), each declared with a
   frozen range and referenced via a derived `<NAME> as u128 * BPS` expression at the same
   call sites the source used a literal `<N> * BPS` — numerically identical at the
   committed point (`5_200 == 5200`, `1_000 == 1000`, `16 == 16`, `2_900 == 2900`), required
   by this project's own search protocol (docs/DESIGN.md §2.4/§2.5), per WHI-1236's own
   instruction on which four constants to expose.
4. **Formatting only** (trailing-comma / line-break placement from `rustfmt` on the touched
   lines) — no token-level content change.

**One more, fifth category, also behavior-neutral: declaration order.** `ALPHA`'s
declaration moved from the source's "Tunable constants (after_swap)" block to just after
`KALMAN_INNO_THRESH` (both blocks it could plausibly sit in, since it is read only by the
Kalman gain gate) — value unchanged (`20_000_000`), name unchanged and kept as generic as
the source wrote it rather than renamed for clarity, only its position in the file moved.
Rust `const` declaration order has no effect on behavior.

**Nothing else differs.** Every dead constant the source itself never reads
(`P1_BASE_FEE`...`P2_SHOCK_CUBE`, `S_STEP_DIR_VOL`/`S_STEP_SHOCK_CUM`/`S_STEP_TOX_MAX`) is
kept verbatim rather than dropped — §2.9's "faithfully first" cuts against even a trivial,
correctness-neutral cleanup during a port, and this file inherits the source's own
`#![allow(dead_code)]`.

**This mechanical diff supersedes the usual "zero the ratchet, pin the fee, compare against
`001`'s 0-line" containment demonstration** the porting-issue template calls for
(§ Containment below) for this specific risk: that demonstration exists to catch a
transcription bug in a *re-derived* port (`007`'s own use of it, after rescaling and
rewriting a quadratic form); a token-level diff against the pristine, unmodified source is
strictly stronger evidence there is no transcription bug here, because there is no
re-derivation to have introduced one.

## Shape risk (docs/DESIGN.md §2.9's pre-search shape-fuzz gate)

`prop-amm validate`: **PASS** — ELF loads and verifies, both sides' monotonicity and
concavity checks pass, randomized reserve/storage checks pass, native/BPF parity is exact
(12 sims, 2,000 steps, `delta=0.000000000` against `tol=0.000001000`).

`bench fuzz --strategy strategies/008-lagging-vwap-fee`: **PASS — zero shape violations**
across 324 states x 2 sides (240 dense-sweep points/grid, 7 golden-section fair-price
multipliers, in both the zeroed- and random-byte-storage variants). Per docs/DESIGN.md
§2.9, a PASS writes no `results/*.md` report by design; this is this port's own record of
having run it.

The source's own shape argument (carried into `lib.rs`'s header comment) holds: within one
`compute_swap` call, `fee` is a pure function of storage and constants (never written); only
the size-penalty term depends on `input`, and `d/dinput[input^2/(input+R)] in [0,1)` with
`S - f >= 0.9*S > size_k` keeps `den` increasing and concave, so `out = R_out -
floor(k*S/den)` is monotone and concave. The one named residual — `quote_profile`'s
overflow fallback (`ski.checked_mul(input)` falling back to `(ski/sum)*input` above
`input ~8.8e14` nano) switching formulas — is exactly what `bench fuzz`'s dense sweeps
(up to `MAX_INPUT_AMOUNT`) are the gate for, and it passed clean.

## CU and arithmetic risk

**Measured through the BPF executor** (not assumed — this repo's public API doesn't expose
consumed CU through `BpfExecutor` directly, so this port used a throwaway example reusing
only `prop-amm-executor`'s *public* API — `BpfProgram::load`, `SyscallContext::new`/
`get_remaining` (`solana_rbpf::vm::ContextObject`), and `solana_rbpf`'s own VM types
directly — never modifying the upstream-owned crate; the example was deleted before this PR,
not committed, same workaround `007`'s own NOTES.md documents):

| Call | State | Side | storage | consumed CU |
| --- | --- | --- | --- | --- |
| `compute_swap` | small reserves (100/10,000 tokens), max input (~1.8e19 nano) | buy | zeroed | 2,186 |
| `compute_swap` | small reserves, max input | buy | random | 4,193 |
| `compute_swap` | small reserves, max input | sell | zeroed | 2,176 |
| `compute_swap` | small reserves, max input | sell | random | 3,545 |
| `compute_swap` | large reserves (~5.76e17 nano each), max input | buy | zeroed | 2,475 |
| `compute_swap` | large reserves, max input | buy | random | 4,294 |
| `compute_swap` | large reserves, max input | sell | zeroed | 2,464 |
| `compute_swap` | large reserves, max input | sell | random | 3,647 |
| `after_swap` | cold start, large reserves | buy | zeroed | 4,921 |
| `after_swap` | warm (momentum scoring across all 3 profiles), large reserves | buy | random | 18,440 |
| `after_swap` | cold start, large reserves | sell | zeroed | 4,936 |
| `after_swap` | warm, large reserves | sell | random | 15,913 |

**Worst case: 18,440 CU** (`after_swap`'s warm path, which runs the 3-profile momentum
scoring on every executed trade) — comfortably under the protocol's 100,000 CU limit (5.4x
headroom) and under `007`'s own ~80,000 CU stop-rule convention. Consistent with the
source's own division-count estimate (~10-14 in `compute_swap`, ~60-90 in `after_swap`'s
worst path) and the absence of any square root in this file (the `isqrt` seeding trap named
in `007`'s own NOTES.md does not arise here).

Overflow: `rx*ry` fits u128 exactly (checked at the shape-fuzz corner states); `R_in*S`
fits; the one genuine overflow site is the `checked_mul` fallback named above, which
`bench fuzz` exercises directly. Rounding is floor throughout the output path (source's own
choice, preserved verbatim).

## Parameter space

Per WHI-1236's own instruction: the source carries ~40 live constants; the 300-point search
budget admits 1-4. The four freed are the source's own `P0` (active ~85% of the time per
the source's own — non-citable — measurement; the choice of *which* four to free is this
project's own reasoning, not the cited fraction):

```rust
// === PARAMS BEGIN ===
const ARB_K_BPS: u64 = 5200; // range: 0..=27333
const COUNTER_K_BPS: u64 = 1000; // range: 0..=4600
const TARGET_BASE_BPS: u64 = 16; // range: 4..=80
const SIZE_K_BPS: u64 = 2900; // range: 0..=4350
// === PARAMS END ===
```

Bounds: `27,333`/`4,600` are the source's own `P2` (loss-minimizer) values for the same two
constants — the widest the source author ever ran them; `4,350` is the source's own `P1`
value; `4..=80` mirrors `004`'s own anchored base-fee range (the opponent's maximum fee).
Every bound here is a **source anchor**, not invented — but every anchor is a *stale-harness*
anchor (measured on the source author's pre-2026-02-16 vintage), the same caveat this
family's whole evidential position carries.

Every other constant (profiles 1/2 in full, tox/shock read weights, decay rates, Kalman
gain bounds, switcher hysteresis thresholds) is frozen at the source's own value.

### Containment

**Run, not superseded.** Per the issue's own instruction, via the degenerate-range scratch
method (`004b`/`005b`/`007`'s own precedent: a throwaway copy, never committed, with every
`PARAMS` range collapsed to `MIN==MAX` at the target value): `ARB_K_BPS`/`COUNTER_K_BPS`/
`SIZE_K_BPS` collapsed to `0..=0`, `TARGET_BASE_BPS` to `66..=66`, and — since this
mechanism carries live non-`PARAMS` terms no `MIN==MAX` collapse can reach — the scratch
copy additionally zeroed, by hand, every remaining source of fee variation: `P0_VOL_MULT`/
`P0_TOX_QUAD`/`P0_TOX_CUBE`/`P0_SHOCK_QUAD`/`P0_SHOCK_CUBE` (the `after_swap` ratchet's
non-base terms), `SIZE_FEE_K` and `PRICE_DEV_K` (the size-fee and cold-start deviation
terms), every profile's own `*_SHOCK_K` and the `profile_params` `default_fee`/`tox_read_k`
pair (all three set to `66bps`/`0` so the ensemble switcher's choice of active profile
cannot matter), and the two literal, non-`PARAMS` discrete bumps in `after_swap`'s
`make_target` closure (`tox_shock_spike`'s `+15bps`, `sv_bump`'s `+5`/`+10bps`) — none of
which are searchable but all of which are live at every trade otherwise. Run via
`bench fit --strategy strategies/008-0line --max-points 1 --no-report`:

| Segment | n | `008-0line` avg edge | `001-cpmm-fee`'s own committed number | gap |
| --- | --- | --- | --- | --- |
| screening | 200 | 384.821251 | 384.820761 | **+0.00049** |
| train | 1,000 | 406.149734 | 406.144289 | +0.005445 |
| validation | 1,000 | 401.809011 | 401.800851 | +0.008160 |

The issue's own stop rule: "any `|gap| > 0.05` on screening means broken plumbing." Measured
screening gap **0.00049** — two orders of magnitude under that threshold, and under the
issue's own predicted magnitude ("under 0.01") too. The issue predicted the sign would be
*negative* (this port's own floor-rounded output path giving the pool less than `001`'s
ceil-rounded path); measured here it is *positive*, by less than half a thousandth of an
edge unit — the same sign-flip-within-a-tiny-predicted-magnitude `007`'s own NOTES.md
records for its own containment check, read there (and here) as implementation-detail
rounding noise around a genuinely near-zero effective gap, not a plumbing defect.
`008-0line` is never committed; no `results/*.md` report is written for it (a `--no-report`
quick check, per docs/DESIGN.md §2.5), and this table is this port's own record of having
run it.

**A second, independent line of evidence, for the risk the literal demonstration does not
cover.** The table above rules out broken plumbing at the 0-line boundary; it does not by
itself rule out a transcription bug elsewhere in the ~40 constants and control flow the
0-line collapse never exercises differentially (since everything is flattened to one
formula there). For that, § Fidelity self-assessment's machine-verified normalized diff
against the pristine source is strictly stronger evidence than a re-derivation's own 0-line
check could be: it shows there is no re-derivation step to have introduced a transcription
bug in. `prop-amm validate`'s parity checks, `bench fuzz`'s 324-state PASS, the P0
plumbing/reproducibility check below, `bench parity`'s exact agreement against `prop-amm
run`, and `bench grid`'s 27-cell run are five further, independent executions of the actual
compiled artifact through the actual harness — none of which would have passed if storage
offsets were wired wrong, the ABI mismatched, or the fast path diverged from the BPF path.

## The pre-registered probe (WHI-1236)

### 1. `bench fuzz` — PASS (§ Shape risk above, 0 of the 300-point budget)

### 2. P0 — plumbing and reproducibility (0 of the 300-point budget)

Verbatim source constants (`ARB_K_BPS=5200, COUNTER_K_BPS=1000, TARGET_BASE_BPS=16,
SIZE_K_BPS=2900`) on the 200 `screening` seeds, via the degenerate-range scratch method
(`004b`/`005b`/`007`'s own precedent: a throwaway copy with the `PARAMS` range collapsed to
`MIN==MAX` at the committed value, never committed), **run three times**: twice as a quick,
uncommitted check (`bench fit --max-points 1 --no-report`), then once more as a genuine,
unbounded `bench fit` invocation (no `--max-points`, no `--no-report`) — which produces an
official `results/*.md` report in its own right, per docs/DESIGN.md §2.5's own rule that a
bounded/`--no-report` run can never itself stand as a committed point's evidence:

| Run | Kind | Screening avg edge (n=200) |
| --- | --- | --- |
| 1 | quick check, `--max-points 1 --no-report` | 478.537310 |
| 2 | quick check, `--max-points 1 --no-report` | 478.537310 |
| 3 | genuine `bench fit`, no bypass flags — `results/2026-08-22-fit-008-lagging-vwap-fee-anchor.md` | 478.537310 |

**Bit-identical to the last digit, all three runs.** Train/validation re-evaluation of the
same point (also bit-identical across all three, including the officially reported run):
**train 509.422465, validation 503.907498.** The officially reported run's own budget
accounting shows `Spent: 1` against the degenerate copy's own single-point declared
space — this is **not** charged against `008`'s own 300-point family budget below (§ 5),
which searches the family's real, full-width frozen space and is accounted separately; the
degenerate copy is never committed, exists only to route this specific point through the
same non-bypassed reporting machinery every other committed number in this project goes
through, and the same convention `004b`/`005b`/`007` already establish for a pre-search
anchor/probe check.

### 3. Kill rule, applied to P0's screening number

Per WHI-1236's own act table (co-leaders' screening: `003b` 426.184965, `004` 426.831472;
0-line `001` 384.820761):

- `< 384.82` → measured negative, close. **Not this case.**
- `384.82` to `< 426.2` → mid-table, close without search. **Not this case.**
- `426.2` to `< 428.8` → tie territory, owner decision. **Not this case.**
- **`>= 428.8` (co-leader + 2.0) → proceed to full train/validation at the verbatim point,
  then the 4-dim search.** **This is the case: 478.537310 clears the threshold by +49.7,
  clears the higher co-leader (`004`, 426.83) by +51.7, and clears the 0-line by +93.7.**

### 4. `bench parity` — native/BPF agreement at the committed point

`bench parity --strategy strategies/008-lagging-vwap-fee` (`observation` segment, seeds
`0..=999`, 1,000 sims x 10,000 steps): **1,000/1,000 seeds agree within 1e-9 relative
(max observed diff 0e0).** Fast path avg edge 501.5657 (`total edge 501565.63 / 1000`)
matches `prop-amm run`'s own avg edge exactly.
(`results/2026-08-22-parity-008-lagging-vwap-fee.md`.)

### 5. The 4-dim search — ran per protocol, did not improve on the anchor

`bench fit --strategy strategies/008-lagging-vwap-fee` (default budget 300, screening
segment, common random numbers): **converged after 172 of 300 points** (coarse grid over
each dimension's endpoints/midpoint, then coordinate descent; 0 invalid points). The
report's own compile-timing line flags fast-path compiles exceeding the `<1s` target during
this run — not investigated further (no independent per-strategy compile-timing baseline
was measured for this port to compare against; P0's own runs used `--no-report` and record
no timings). This is a wall-clock observation about the machine this search happened to run
on, not a code-shape claim, and has no bearing on the correctness of the measured edge
numbers, which reproduced bit-identically across two independent runs of this same search
(§ Verification).

**Winning point:** `ARB_K_BPS=26480, COUNTER_K_BPS=7, TARGET_BASE_BPS=14, SIZE_K_BPS=3378`
— screening avg edge **472.288277**, train **502.748645**, validation **497.705855**
(`results/2026-08-22-fit-008-lagging-vwap-fee.md`).

**This is worse than the verbatim/anchor point on every segment measured** (screening
478.54 vs. 472.29; train 509.42 vs. 502.75; validation 503.91 vs. 497.71) — the coarse grid
never evaluates the anchor point itself (its own factorial corners are `{0, 13667, 27333}
x {0, 2300, 4600} x {4, 42, 80} x {0, 2175, 4350}`, none of which is `[5200, 1000, 16,
2900]`; verified by grep against the full evaluated-curve list — the exact anchor vector
never appears), and coordinate descent from those corners converged to a different local
optimum rather than finding its way back to the (unsearched) anchor.

**Why the anchor plausibly beats a coordinate descent that treats it as unknown:** this
family's ensemble switcher scores all three profiles against each other on every trade and
routes flow to whichever wins that scoring — so changing `P0`'s constants doesn't just
change `P0`'s own quote, it changes *how often* `P0` gets selected at all, while `P1`/`P2`
stay fixed at the source author's own values (jointly hand-tuned together with `P0`, per the
source's own ~90-commit tuning campaign). A 4-dimensional coordinate descent that varies
only `P0` against two *fixed* co-tuned profiles is searching a differently-shaped surface
than the one the source author tuned against — non-monotonic and apparently hard for
coordinate descent to navigate back to the jointly-tuned point from a corner start, within
this family's equal 300-point budget (docs/DESIGN.md §2.5's "known and accepted
consequence": higher-dimensional families are covered less densely by an equal budget).

**Decision: the anchor point is committed, not the search's winner.** Committing a
measurably worse point solely because it came from the budgeted search, while a cheaper,
already-known point in the same declared space is strictly better on every segment, would
misrepresent the family's own best measured result. This is the same "commit the best
validated point, not the process's own artifact" judgment `004b`/`005b`/`007` each made in
their own way (closing on a pre-search probe rather than spending search budget chasing a
result the probe already argued against) — here the direction is inverted (the pre-search
number *wins*, not loses), but the principle is the same. **0 of the 300-point budget's
*outcome* is adopted; the 172 points spent are recorded here as a negative finding about
this family's own loss surface, not discarded.** `strategies/008-lagging-vwap-fee/lib.rs`'s
committed `PARAMS` block is therefore the source's own unmodified `P0` values.

**Boundary hits, flagged:** the committed point (`5200, 1000, 16, 2900`) sits strictly
interior to all four declared ranges — not a boundary artifact. The *non-adopted* search
winner's `COUNTER_K_BPS=7` and `ARB_K_BPS=26480` are numerically close to their `0` and
`27333` range extremes, but checking the evaluated curve rules out a genuine boundary
artifact for either: coordinate descent tried both neighbors on each dimension
(`COUNTER_K_BPS in {6,7,8}` at 472.229/472.288/472.275; `ARB_K_BPS in {26479,26480,26481}`
at 472.177/472.288/472.213 — both single-dimension peaks at the reported value, not
monotone toward the bound), so these are genuine interior local optima that happen to sit
near an edge, not values the search was pushed against a wall to reach. Neither is
committed either way.

### 6. `bench grid` — the 27-cell fragility matrix, against `001-cpmm-fee`

`bench grid --candidate strategies/008-lagging-vwap-fee/lib.rs --reference
strategies/001-cpmm-fee/lib.rs` (1,080 sims, 27 cells x 40 seeds;
`results/2026-08-22-grid-008-lagging-vwap-fee.md`):

**Wins 26 of 27 cells.** The single loss is cell 26 (`fee=80bps, liq=2.0x, sigma=0.0070`):
candidate 116.44 vs. reference 152.43, mean diff **−35.99** `[−43.10, −28.88]`.

This is a materially different outcome than WHI-1236's own pre-registered prediction, which
expected cells 21/22 (`fee=80, liq=1.0x`, low/mid sigma) to "stay lost" per finding 4
(competitor-blindness) and cells 8/17/26 (the high-sigma deep-liquidity corners) to "stay
negative." **Measured: cell 21 is +36.07 `[18.58, 53.56]`, cell 22 is +40.88 `[30.39,
51.38]`, cell 8 is +157.13 `[145.65, 168.61]`, and cell 17 is +97.29 `[87.05, 107.53]` — all
four clear wins, not losses.** Only cell 26 (the single most extreme corner: highest fee,
deepest liquidity, highest volatility simultaneously) stays negative, and by a modest
margin relative to the family's other wins. This is recorded as-is rather than smoothed
over — the prediction was wrong in a specific, falsifiable way, and the actual mechanism
(directional classification against a lagging VWAP, plus the counter-direction rebate)
evidently reaches further into the grid than the ticket's own worst-case framing expected.

### 7. `bench compare` — paired comparison against the current M1 leader (`004`)

`bench compare --candidate strategies/008-lagging-vwap-fee/lib.rs --reference
strategies/004-ewma-shock-decay-fee/lib.rs --segment validation` (n=1,000;
`results/2026-08-22-compare-008-lagging-vwap-fee-vs-004-ewma-shock-decay-fee.md`):

**Paired mean diff: +57.610368. 95% CI: [53.209045, 62.011692], n=1,000.** The CI excludes
zero by a wide margin — this is not a marginal or noisy result.

## Summary table

| Segment | n | `008` avg edge | Co-leader (`004`) | 0-line (`001`) | diff vs. `004` |
| --- | --- | --- | --- | --- | --- |
| screening | 200 | 478.537310 | 426.831472 | 384.820761 | +51.71 |
| train | 1,000 | 509.422465 | 451.178318 | 406.144289 | +58.24 |
| validation | 1,000 | 503.907498 | 446.297129 | 401.800851 | +57.61 (paired CI `[53.21, 62.01]`) |
| observation (leaderboard-comparable) | 1,000 | 501.5657 | 443.75 | 399.97 | — |

**This is the largest margin over the current M1 leader of any entry measured in this
project.** Per docs/DESIGN.md §2.10, M1 ranks on validation — on that measure, `008` (as
committed above) is the new M1 leader by a wide, statistically clear margin, contingent on
the same evidential caveats every entry here carries (§3.3's own-measurement protocol; no
number from the source repository counted as evidence at any point in this measurement).

## Acceptance criteria (per WHI-1236)

- [x] Step 0 (a) and (b) each answered explicitly in the issue before any file was copied
      or any measurement was run (§ Provenance above; WHI-1236 itself carries the full
      record).
- [x] No number from the source repository appears as evidence anywhere in this project's
      docs, NOTES, or reports — every number in this file is this project's own
      measurement.
- [x] `bench fuzz` passes on the verbatim file, with the size-penalty overflow fallback
      specifically exercised (§ Shape risk).
- [x] P0 is run twice and is bit-identical; `bench parity` passes (§ The pre-registered
      probe, steps 2/4).
- [x] The kill rule is applied honestly (§ 3): it did not trigger a close, and the port
      proceeded per its own `>= 428.8` branch.
- [x] Worst-case CU is measured through the BPF executor and recorded, not inherited
      (§ CU and arithmetic risk: 18,440 CU worst case).
- [x] The BPF scaffolding (`#[inline(never)]` on all five of `quote_profile`/
      `profile_params`/`raw_edge_sample`/`score_one_profile`/`handle_after_swap`, the
      isolated 1024-byte storage frame) is preserved verbatim, same functions and order as
      the source.
- [x] Proceeded past P0: near-exact containment demonstrated with the predicted-gap
      threshold applied — screening gap **+0.00049** against `001`'s 384.820761, two orders
      of magnitude under the issue's own `|gap| > 0.05` stop rule (§ Containment), plus the
      stronger mechanical-diff argument for the transcription-bug risk that check doesn't
      cover (§ Fidelity self-assessment); fitted point (the anchor, not the search's own
      inferior point) with train/validation/observation (§ Summary table); `bench grid`
      produced (§ 6); boundary hits flagged (§ 5 — the committed anchor is interior to all
      four ranges; the non-adopted search winner is not).
- [x] A companion docs task records the §6.2/§2.10 post-freeze exception — already landed
      (`WHI-1237`, merged prior to this issue's own porting work).
- [x] `cargo test --workspace` green; fmt/clippy per `AGENTS.md` (see PR description for
      the exact run).

## Verification

- `prop-amm validate strategies/008-lagging-vwap-fee/lib.rs`: PASS (ELF load/verify,
  monotonicity, concavity, randomized-storage checks, native/BPF parity).
- `bench fuzz --strategy strategies/008-lagging-vwap-fee`: PASS, zero shape violations,
  324 states x 2 sides.
- `bench fit --strategy <degenerate anchor copy> --max-points 1 --no-report` (scratch,
  never committed), run twice: bit-identical screening/train/validation.
- `bench fit --strategy <degenerate anchor copy>` (no bypass flags — a genuine, officially
  reported run against the collapsed single-point space):
  `results/2026-08-22-fit-008-lagging-vwap-fee-anchor.md`, bit-identical to both quick
  checks above.
- `bench fit --strategy <degenerate 0-line copy> --max-points 1 --no-report` (scratch,
  never committed, every non-`PARAMS` fee-variation source hand-zeroed too): screening
  384.821251 against `001`'s own committed 384.820761, gap +0.00049 (§ Containment).
- `bench parity --strategy strategies/008-lagging-vwap-fee`: PASS, exact native/BPF/`prop-amm
  run` agreement.
- `bench fit --strategy strategies/008-lagging-vwap-fee`: converged 172/300, non-adopted
  winner recorded above.
- `bench grid --candidate strategies/008-lagging-vwap-fee/lib.rs --reference
  strategies/001-cpmm-fee/lib.rs`: 26/27 cells won.
- `bench compare --candidate strategies/008-lagging-vwap-fee/lib.rs --reference
  strategies/004-ewma-shock-decay-fee/lib.rs --segment validation`: paired +57.61,
  CI excludes zero.
- All `bench`/`prop-amm` invocations above were run from a detached scratch worktree
  outside the primary clone's directory tree (`nested-worktree-breaks-cargo-isolated-builds`
  in this project's own agent memory; same workaround `004b`/`005b`/`007` document) — every
  `results/*.md` report was copied back verbatim; no scratch strategy source is committed
  anywhere.
- Each of the four committed `results/*.md` reports for this strategy stamps
  `Commit: 7e539b6+dirty` — the scratch worktree's own `git status` reports dirty because
  `strategies/008-lagging-vwap-fee/lib.rs` (this PR's own file, byte-identical to what's
  committed here) was staged there before each run, so the reports correctly disclose that
  the measured code differs from `origin/dev`'s tip rather than silently understating it
  (`results-file-dirty-stamp-circularity` in this project's own agent memory: an untracked
  scratch file does *not* trigger the dirty flag by this tool's own design, so a
  regenerated report from an unstaged copy would misleadingly show a clean commit).
