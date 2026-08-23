# prop-amm-challenge — Design Document / PRD

> This file is the **spec of record** for this project. Every issue, architectural
> decision, and parameter traces back to a section here. It is produced by grilling the
> idea into shape (`/grill-me`) and formalizing the result (`/to-spec`) — do not skip
> straight to code with this document empty.
>
> **State of this document:** §1–§8 are written. §4.1/§4.2 describe both inherited
> upstream code and our own layer. **§6.2 was owner-input-blocked** (WHI-1197) until the
> frozen strategy list was supplied and filed under `docs/references/`; it is now frozen
> and M1 is ticketable.

## 1. Background & Goals

### 1.1 Vision

Find the **best AMM price curve** for the Prop AMM Challenge simulation — not merely the
highest number we can produce, but a curve whose advantage we can *explain* and *defend*.
The challenge (<https://ammchallenge.com/prop-amm>) is a public one, so a body of candidate
strategies already exists in the wild: some with source, some as Solidity to be ported,
some as prose descriptions of a mechanism. This repo turns that scattered material into a
ranked, reproducible comparison under one measurement protocol, and names a winner whose
margin survives a held-out sample.

The load-bearing word is *reproducible*. A single `avg edge` printed by a harness we wrote
ourselves is not evidence; it is a number. Everything in §2 exists to make the final
ranking mean something.

### 1.2 v1 Scope

v1 delivers three things:

1. **A measurement layer** (`tools/bench`) that evaluates any candidate `lib.rs` under a
   fixed, documented protocol: paired-by-seed comparison, three disjoint seed segments,
   two evaluation modes, and a parity gate against the upstream CLI.
2. **A strategy layer** (`strategies/`) holding one self-contained submission file per
   candidate, each accompanied by a `NOTES.md` recording provenance, the frozen parameter
   space, and its measured numbers.
3. **A ranked comparison and a named winner**, decided on a test segment used exactly once,
   with a `results/` snapshot traceable to a commit.

Parameters inherited rather than invented — per-step volatility range, normalizer fee and
liquidity sampling, step count, the edge formula — come from the challenge and live in
`crates/shared/src/config.rs`. They are **not** re-derived here (§3.1).

### 1.3 Non-goals (explicitly out of scope for v1)

- **Leaderboard rank is not a v1 deliverable.** Submitting to the web UI stays *possible*
  (`AGENTS.md` § Promotion lanes keeps the tag → submission path intact) but no v1 success
  criterion depends on it, and no decision is made on a leaderboard number (§1.4, §2.2).
- **No modification of the simulator.** `crates/{shared,executor,sim,cli,submission-sdk}`
  and `programs/` are upstream-owned. A local edit invalidates every measurement in
  `results/` (§3.2).
- **No exhaustive search per family.** Search budget is capped and equal across families by
  design (§2.5); v1 does not attempt to find each family's global optimum.
- **No full retail/arb edge decomposition.** Observability stops at L1 (§2.7); L2 is a
  documented upgrade path (§8), not v1 work.
- **No automated CI gate on bench numbers.** `results/` snapshots are produced and reviewed
  by hand; wiring them into CI is not v1.
- **No new strategy ideas of our own** beyond the one-variant-per-finalist rule (§2.9).
  Original designs are v0.2.0 work; this bars our own designs, not ports of another's
  mechanism, which §2.9's fidelity contract governs instead (§6.2's `007` and `008` are
  such ports, each added post-freeze by exception per §2.10 — `008`'s licence and
  copying-for-internal-analysis-only limits are recorded there too).

### 1.4 Success criteria

v1 is done when all of the following hold:

1. `tools/bench` reproduces `prop-amm run` **per seed**, not just in aggregate, for the
   starter program (§2.6 parity gate).
2. The **0-line** is established: the CPMM fee family (`strategies/001-*`) is fitted under
   the full protocol, and its fee↔edge response is single-peaked (§2.8).
3. Every strategy on the frozen list (§6.2) has reached a terminal state: a fitted point
   with train/validation numbers, an explicit `wontfix` with a recorded structural reason
   (§2.9), or a `Canceled` by owner decision with its reason recorded in §6.2 (§2.10).
4. A winner is named from a **single** use of the test segment, reported with a paired
   confidence interval and its regime slice table, and the answer to "does it beat the best
   fixed-fee CPMM?" is stated explicitly — including if the answer is no.
5. `results/` holds the snapshot backing that claim, carrying the commit sha it was
   produced at.

**All five hold as of WHI-1226 — v1 is done.** See §9 for the checked-off record, the
named winner (`008`), and its paired test-segment interval over runner-up `003b`.

## 2. Requirements / Specification

This section is the measurement protocol. It is the part of the repo most likely to be
quietly violated under time pressure, so each rule states *what it protects against*.

### 2.1 Primary metric

The metric is upstream's: `SimResult.submission_edge` per simulation, summed from the
per-trade formula in `crates/sim/src/engine.rs:55`. We do **not** substitute our own
objective — optimising something the challenge does not reward would answer a different
question than the one asked.

**Primary ranking statistic:** the *paired* mean difference in edge, per seed, between a
candidate and a reference, with a confidence interval. Two candidates evaluated on the same
seed see the same price path, the same retail stream and the same normalizer parameters, so
the per-seed difference removes almost all the variance that an unpaired comparison of two
means would leave in. Comparing two aggregate `avg_edge` values is not sufficient and is
not permitted as the basis for a ranking claim.

**Measured anchor:** the starter program (500 bps CPMM, `programs/starter/src/lib.rs`)
scores **avg edge 210.50** over seeds `0..=999` at 10,000 steps, native path. Reproduced at
commit `0ada20b` via `cargo run -p prop-amm --release -- run programs/starter/src/lib.rs`
(total edge `210496.44`). This number is the parity anchor in §2.6.

### 2.2 Seed segmentation

Seeds are the only source of sampling. `HyperparameterVariance::apply(&base, seed)`
(`crates/shared/src/config.rs:93`) derives a simulation's entire regime from its seed, and
`SimulationConfig.seed` then drives the price path (`config.seed`), the retail stream
(`seed+1`) and the arbitrageur (`seed+2`) as three independent `Pcg64` streams
(`crates/sim/src/engine.rs:16-34`). Seeds are `u64`, so independent samples are unlimited
and cost only compute.

| Segment | Seeds | Used for | Reuse |
| --- | --- | --- | --- |
| **train** | `1_000_000..=1_000_999` | fitting a family's parameters | unlimited |
| ↳ *screening subset* | `1_000_000..=1_000_199` | the search inner loop (§2.5) | unlimited |
| **validation** | `2_000_000..=2_000_999` | picking a family's final point; cross-family ranking | many times, logged |
| **test** | `3_000_000..=3_000_999` | the final winner decision | **exactly once** |
| **grid** | `4_000_000 + cell*1_000 + i` | grid mode (§2.3) | unlimited |
| **observation** | `0..=999` | reporting only | never a decision input |

Three segments rather than two, because the test segment is what makes the winner's number
an honest estimate. Every family is fitted on train and ranked on validation; with a dozen
families ranked on the same validation segment, the top score carries a winner's-curse
component — the highest number contains luck as well as skill. A segment touched only once,
after the list is frozen (§2.10), is the only thing that removes it.

`0..=999` is the grader's own default (`BASELINE_SIMS`/`BASELINE_STEPS` with
`seed_start=0, seed_stride=1`). It is reported in every table so a leaderboard-comparable
number is always visible, and it is **excluded from every decision** — which is exactly
what keeps it comparable.

**Segment starts are far apart deliberately.** Disjointness must be checkable by eye.

### 2.3 Two evaluation modes

`runner::run_batch_native` accepts an arbitrary `Vec<SimulationConfig>` and every field of
`SimulationConfig` is `pub`, so bench is not limited to upstream's sampling distribution.

- **distribution mode** — configs from `HyperparameterVariance::apply` over a seed segment.
  This is the graded distribution; it produces the headline paired statistic and all
  rankings.
- **grid mode** — an explicit factorial of regime corners, each cell run over its own seed
  block. This produces the fragility matrix.

  | Axis | Levels | Source |
  | --- | --- | --- |
  | `norm_fee_bps` | 30, 55, 80 | endpoints + midpoint of upstream's `U{30,80}` |
  | `norm_liquidity_mult` | 0.4, 1.0, 2.0 | endpoints + neutral of upstream's `U[0.4,2.0]` |
  | `gbm_sigma` | 1e-4, 1e-3, 7e-3 | endpoints + ~midpoint of upstream's `U[0.0001,0.007]` |

  27 cells × 40 seeds. `retail_arrival_rate` and `retail_mean_size` are held at
  `SimulationConfig::default()` (0.8, 20.0) to keep the factorial readable; that is a
  scope choice, and widening it is a §8 item.

Grid mode exists because post-hoc binning of the sampled distribution cannot support a
fragility claim: the corners that break a curve are precisely the rarest draws. With three
independent uniform axes, the "high volatility × cheap competitor × thin competitor" corner
is roughly a 1-in-60 event — about 17 simulations in a 1,000-sim batch, which is far too few
for a paired interval to say anything. Grid mode buys balanced coverage; it is **not** the
graded distribution and must never be reported as a headline edge.

### 2.4 A strategy is a family, not a point

A candidate is delivered as a **parameterised curve** plus a **frozen search space**, not as
a fixed set of constants. Collected material does not support point comparison: prose
descriptions give a mechanism and no numbers, and Solidity constants were tuned for a
different market than this simulator. Ranking the original constants would measure whose
defaults happened to suit this harness, and would discard good curve forms whose constants
were simply never fitted here.

Each strategy therefore delivers:

1. the parameterised `compute_swap` (and `after_swap`, if the mechanism needs state),
2. a **parameter space declared in `NOTES.md` and frozen before any search runs**,
3. the fitted point, committed as the strategy's `lib.rs` — the best point measured within
   that frozen space by the family's own declared protocol, which is usually the 300-point
   search's own convergence result but is not required to be: a pre-registered probe (this
   issue's own required P0-style step) run before the search can measure a point the
   search's coarse grid never touches and that beats everything the search finds within its
   budget. §2.5's own equal-budget invariant still applies to whichever point is committed:
   a `--max-points`/`--no-report` quick check is never sufficient on its own (§2.5 says so
   explicitly) — the committed point's own numbers must also come from a genuine,
   non-bypassed `bench fit`/`bench compare`/`bench parity` invocation with its own
   `results/*.md` report, even when that invocation is run against a purpose-built
   degenerate-range copy that pins the declared space to the single point being confirmed.
   §8's "protocol lesson" 7 records the first instance and the reasoning,
4. its train and validation numbers, and its CLI parity reproduction (§2.6).

Freezing the space *before* searching is what makes the seed segmentation of §2.2 worth
anything: a space widened after seeing results is a search over spaces, and its validation
number is no longer an honest estimate.

### 2.5 Search protocol

- **Algorithm:** coarse grid to locate, then coordinate descent to refine. Families carry
  1–4 free parameters and the objective is noisy and non-smooth (integer bps, integer
  division truncation throughout), which is territory where population methods buy nothing
  and cost a dependency and a reproducibility burden.
- **Common random numbers:** every point in a search is evaluated on the *same* screening
  seeds (`1_000_000..=1_000_199`). This makes point-to-point comparison paired as well.
  Resampling seeds per point would leave enough noise to chase a phantom optimum.
- **Equal budget, hard cap: 300 evaluation points per family.** Recorded in `NOTES.md`.
  Unequal budgets would reintroduce through the back door exactly what §2.9 exists to keep
  out: a ranking that reflects effort spent rather than curve quality. `bench fit
  --max-points <N>` (WHI-1205) can *lower* this for a quick, uncommitted check of the fast
  path itself (e.g. its compile timing) — it is not a way to fit a strategy on a smaller
  budget, and `bench fit` refuses it without `--no-report`: a bounded run can never
  produce `results/` evidence, so this equal-budget invariant still holds for every
  committed point.
- **Known and accepted consequence:** an equal point budget favours low-dimensional
  families. A 4-parameter family is covered far less densely by 300 points than a
  1-parameter one. This is not a defect to correct — being hard to tune is a real drawback
  for a curve meant to be deployed — but it must be stated whenever a high-dimensional
  family loses (§8).
- **Final point evaluation:** the search winner is re-evaluated on the full train segment
  (1,000 sims) and on validation (1,000 sims) before entering the ranking.
- **Search-time panics (WHI-1213).** `crates/sim/src/curve_checks.rs`'s shape check panics
  mid-simulation on a monotonicity/concavity violation, from inside a rayon worker spawned
  by `runner::run_batch_native` — `crates/sim` is upstream-owned (§3.2), so the fix lives
  entirely in `tools/bench`, the caller. `bench fit` catches it
  (`std::panic::catch_unwind` around the whole batch-run call, with the panic hook
  suppressed for that call so an expected, handled outcome doesn't spam a backtrace) rather
  than letting it abort the process. A caught point becomes a distinct `Invalid` outcome —
  not scored `-inf` (which would contaminate the single-peak scale/tolerance computation,
  §2.8, since `abs(-inf)` poisons the `f64::max` fold) and not silently skipped (which could
  let coordinate descent wander back into the same panicking region without penalty): it
  still consumes one of the 300 evaluation points, is excluded from the peak search, and is
  listed by parameter vector and panic message in the `results/` snapshot. The search winner
  is re-evaluated the same way on the full train/validation segments (this section's "final
  point evaluation") — a point valid on `screening`'s seeds is not guaranteed valid on a
  different, larger seed set (the Orbic family's quantization jitter, WHI-1206 — now
  Canceled, §6.2; the phenomenon is general, the example is retained — is exactly this:
  probabilistic across seeds, not just across parameter values) — and an invalid
  re-evaluation blocks the run the same way a failed single-peak check does (§2.8): the
  evidence gathered so far is still written, but the point is not entered into the ranking
  un-flagged. If `bench fuzz` (§2.9's pre-search shape-fuzz gate, WHI-1212) already passed
  for a family but a search point still panics, that gap is itself a signal the gate's
  corner set is incomplete — the failing parameter vector recorded here is exactly the
  input needed to extend it. If *every* point the coarse grid evaluates panics there is no
  fitted point and the run fails, but the snapshot is still written first (same rule as
  above): those vectors are the whole basis for the two calls that case forces — extending
  that corner set, or routing the family to §2.9's `wontfix` (§8 risk 7).

### 2.6 Compile paths and the parity gate

Two compile paths exist, with different jobs:

- **Fast path (search).** `tools/bench` maintains a single reused build directory with a
  shared `target/`, rewriting only `src/lib.rs` per point. WHI-1194 measured 160 warm
  compiles at min=0.472s, mean=1.000s, max=1.311s and, lacking a control, attributed the
  ~9x gap over the original **0.11–0.57 s** estimate to the measuring session's
  environment. WHI-1205 checked the four candidate structural causes (Cargo.toml
  rewritten per point, missing `--features no-entrypoint`, the build silently inheriting
  the root `[profile.release]`'s `lto=true`/`codegen-units=1`, and the timed window
  covering more than the build) — all four ruled out as the explanation (see
  `strategies/001-cpmm-fee/NOTES.md` for the full accounting) — and a fresh, bounded
  re-measurement (`bench fit --max-points 8 --no-report`, WHI-1205's own flags for cheap
  verification — `--no-report` by design, so unlike WHI-1194's figures above there is no
  committed `results/` snapshot for this one; cited here from the run's own console
  output, reproduced in `strategies/001-cpmm-fee/NOTES.md`) came in at **7 warm compiles:
  min=0.320s, mean=0.327s, max=0.343s**,
  meeting the original estimate's order of magnitude once the build's `dlopen`/tempfile
  load step is counted alongside the `cargo build` itself. The 9x gap did not reproduce
  on the same machine, and none of the four candidates implicated the fast path's own
  design — it never rebuilds `pinocchio`/`wincode`/`prop-amm-submission-sdk` after the
  directory's first use, only `user_program` itself relinks per point. That is as far as
  this evidence goes: non-reproduction rules out the four named causes, it does not prove
  WHI-1194's specific session-environment explanation was the correct one (see
  `strategies/001-cpmm-fee/NOTES.md`'s "What this does and does not establish"). Every one
  of these figures — the original estimate, WHI-1194's measurement, and WHI-1205's
  re-measurement — is a large improvement over the reference path below.
- **Reference path (reporting).** The upstream CLI, `crates/cli/src/commands/compile.rs`.
  Measured: **7–10 s and ~51 MB per point**, because `ensure_build_dir`
  (`compile.rs:36`) keys an isolated build directory by source hash, so `pinocchio`,
  `wincode`, `darling` and `syn` are rebuilt for every parameter point.

The fast path is a **partial re-implementation** of upstream's compile step, which also
injects the native shim (`native_shim_source`) and rejects `unsafe`
(`source_contains_unsafe_keyword`). If upstream ever changes either, our measurements drift
from the graded environment *silently*. That risk is bought off by a mandatory gate:

> **Parity gate.** A strategy's committed parameter point must be reproduced through
> `prop-amm validate` and `prop-amm run`, and the numbers recorded in `NOTES.md`. A
> mismatch against the fast path is a **blocker**, not a note.

The starter anchor of §2.1 (avg edge 210.50, seeds `0..=999`) must match **per seed**, not
merely in aggregate — an aggregate match can hide compensating per-seed errors.

### 2.7 Observability level L1

`SimResult` carries only `(seed, submission_edge)` (`crates/shared/src/result.rs:2`), and
richer per-trade data would require editing `crates/sim` — forbidden by §3.2. What *is*
reachable without any upstream change: `run_simulation_native` takes the submission's and
the normalizer's `after_swap` slots as separate arguments, and upstream calls `after_swap`
only on **real trades**, never during router or arbitrageur quoting (README § afterSwap).

**L1 (v1 scope):** bench wraps both AMMs' `after_swap` in pass-through recorders and derives

- **flow share** — our executed volume ÷ total executed volume, and
- **edge per unit volume** — captured spread per unit of flow.

Together these separate the two failure modes that a single edge number conflates: *not
attracting flow* versus *attracting it and pricing it badly*. That distinction is the
central tension of the challenge (README: price too aggressively and retail routes away),
and without it a porter facing a bad number has nothing to act on.

The wrappers must delegate unchanged; they may not alter behaviour.

**L2 (not v1):** the fair price at every step is exactly reconstructible outside the
simulation, since `GBMPriceProcess` is public and its stream depends only on
`(initial_price, mu, sigma, dt, config.seed)`, and `after_swap` carries `step`. Replaying it
alongside the retail stream would allow a full per-trade retail/arb edge decomposition. It
is deferred: it is a *copy* of upstream's call ordering, and if that ordering ever differs
the decomposition is wrong without failing. See §8.

### 2.8 Baselines — where the 0-line sits

- **`strategies/001-cpmm-fee`** — constant product with a free fee parameter, fitted under
  the full protocol. This is the 0-line. A strategy that does not beat the best fixed-fee
  CPMM is recorded as a **negative result**, not as a mid-table entry.
- **starter, as shipped** — 500 bps, avg edge 210.50 on `0..=999`. A fixed reference point
  and the parity anchor. Note that 500 bps is almost certainly *not* the family optimum;
  the competitor charges 30–80 bps.
- **normalizer-as-submission** — `crates/shared/src/normalizer.rs` run as the candidate,
  i.e. the same curve as the opponent. A meaningful zero: it shows how the router splits
  flow under perfect symmetry. "Perfect symmetry" is pointwise — matched fee **and** matched
  initial reserves — not "vs. the opponent's sampled per-simulation regime": a submission has
  no way to read the opponent's actual `norm_fee_bps`/`norm_liquidity_mult` for that
  simulation, so a fixed-parameter submission (`strategies/000-normalizer/lib.rs`, WHI-1195)
  is symmetric only against the opponent's *default* point, not its distribution. Verified
  both ways: `tools/bench/src/telemetry.rs`'s
  `matched_curve_and_reserves_produce_flow_share_near_half` pins both axes and confirms flow
  share lands within 0.01 of 0.5; `strategies/000-normalizer/NOTES.md`'s Finding records the
  ungated `observation`-segment measurement (0.606) and why the difference is expected, not a
  bug.

`001` doubles as the **self-check of the protocol itself**: a fee family's edge response
should be single-peaked. If bench reports a multi-modal response or an absurd optimum, that
is a bench defect, not a discovery. This check must pass before any other strategy's numbers
are trusted.

The check runs over the sub-curve of points that produced a valid edge only (§2.5,
WHI-1213) — an `Invalid` point is a hole in the curve, not a data point with an extreme
edge, so it is simply absent rather than injected as a sentinel value. With fewer than two
valid points the check has nothing to compare and is reported `INCONCLUSIVE`, not `PASS` —
a vacuous "no decrease seen" over 0 or 1 points must not be read as confirmation of
single-peakedness.

**A negative result has three distinguishable forms** this project has actually produced,
and `strategies/README.md`'s registry records which one applies: *loses to the 0-line*
(`006`, avg edge 369.79 < `001`'s 399.97), *ties its parent bit-exact* (`004b`, `bench
compare` vs. `004` measures a paired mean diff of exactly 0.000000, CI `[0.000000,
0.000000]`), and *closes on a probe-stage measurement without running a search*
(`007`'s Step 0.5 boundary hit; `005b`'s Probe A survives its own literal kill conditions,
but a paired `bench compare` CI computed at the probe stage shows a real net loss — either
way, none of §2.5's 300-point budget is spent). The distinction
matters for how much weight the negative carries: a bit-exact tie is not evidence the
modification was wrong, only that it was untested past the probe; a probe-closed negative
spent none of the search budget, unlike `006`'s fully-searched negative. See §8 for the
measured findings this produced.

### 2.9 Fidelity contract

Ported strategies are implemented **faithfully first**. Only the minimum changes needed to
make a strategy *run at all* are permitted:

- monotonicity and concavity (`crates/sim/src/curve_checks.rs:23` **panics** mid-simulation
  on violation — this is a crash, not a low score),
- the 100k CU limit,
- safe Rust only (`compile.rs:141` rejects any `unsafe` token, detected at `compile.rs:229`),
- the `NAME` / `MODEL_USED` / `get_model_used` interface.

Improvements beyond that are **not** folded in. They are opened as an explicit variant
(`002b-<slug>`) that ranks as its own entry. This keeps the comparison a comparison of
algorithms, and as a side benefit quantifies exactly what our own modifications were worth.

**Structural incompatibility is a terminal state.** Some designs — piecewise curves,
volatility-scaled fees, concentrated liquidity — violate the discrete concavity check by
construction. An issue that establishes this records the reason and closes as `wontfix`
(`docs/agents/triage-labels.md`); it does not stay open indefinitely.

**Provenance is mandatory.** Prose-only strategies may be second-hand or simply wrong.
`NOTES.md` records the source, its form, and a fidelity self-assessment.

**The pre-search shape-fuzz gate.** `prop-amm validate` alone probes far too little of a
candidate's input/state space to make "passed validation" mean "won't panic mid-search" —
10 sizes at one fixed reserve state, versus the ~10^7 instances a real 1000-sim run
exercises. `bench fuzz --strategy <dir>` (WHI-1212) closes that gap: dense sweeps and
golden-section-shaped sample sets, run against every `[grid]` regime corner — both on the
CPMM invariant and randomly jittered off it — including states only reachable after a
full-length GBM drift, in both a zeroed- and a random-byte-storage variant, mirroring
`curve_checks.rs`'s own check. Every M1 strategy issue runs this gate before a `bench fit`
search is allowed to spend paired-seed budget on that candidate. A PASS writes no report
(the gate is meant to run repeatedly, before every search); a violation commits a
`results/*.md` report naming the state and input pair — unless today's report slot for
that strategy is already taken by an earlier run, in which case it's noted rather than
silently dropped (`docs/DEFERRED_ISSUES.md`) — the same evidentiary role a runtime panic's
stack trace plays for this section's `wontfix` path.

### 2.10 Convergence

1. Freeze the strategy list (§6.2). Nothing is added to v1 after this point, except
   through a recorded exception below.
2. Run every listed strategy through M1 to a terminal state.
3. Rank on validation. Open **one** variant for each of the top three (§2.9).
4. Use the test segment **once**: winner and runner-up, paired interval, regime slices.
5. v1 ends. Later ideas belong to v0.2.0, with a fresh test segment.

The freeze point is the mechanism, not the good intentions. Without it the list keeps
growing — every strategy can spawn a variant — validation gets consumed indefinitely, and
sooner or later someone peeks at test. Only a declared "stop adding now" prevents that.

**Removal is not an addition, but it is not silent either.** An entry may leave the frozen
list after the freeze only by owner decision, and only by being marked Canceled in its
§6.2 row with a reason and a pointer to the issue that decided it — never by deleting the
row. §1.4's "every strategy on the frozen list has reached a terminal state" is otherwise
unfalsifiable: a row that just isn't there can't be checked against it. This rule governs
removals only; it does not reopen the freeze for new entries.

**Addition is the mirror case, and it is likewise not a reopening.** An entry may be added
to the frozen list after the freeze only by owner decision, recorded in a new §6.2 row
marked "added post-freeze by exception" with the reason — including a **stated reason why
the entry is not redundant with what is already measured** (below) — and only **while the
test segment is still unspent** — the same protection the freeze exists to preserve. Once
the test segment is spent, no addition is possible without a fresh test segment, which by
definition makes the entry v0.2.0 work (§1.3) rather than a v1 addition — the window is
**self-closing**, not open-ended, which is what stops "one deliberate addition" from
drifting into an unbounded list. This rule governs additions only; it does not reopen the
freeze generally.

**The clause has been exercised twice**, each use with its reason recorded in §6.2: `007`
(`WHI-1219`/`WHI-1220`), on provenance grounds; and `008` (`WHI-1236`/`WHI-1237`), because
it attacks an axis M1 has measured as *unattacked* rather than dead (`WHI-1235`'s
competitor-blindness finding, §8). One use is an exception; two is a pattern.

That pattern implies the non-redundancy condition folded into the rule above, one both
prior uses happened to satisfy without it being stated as a requirement at the time. Well
into M1, with `WHI-1235`'s six recorded findings, that bar is materially higher than it
was at freeze time — a candidate that only re-attacks an axis already measured as dead is
not worth an exception.

## 3. Cross-cutting Policies

### 3.1 Parameter provenance

Every tunable falls into exactly one bucket:

| Bucket | Where it lives | Who may change it |
| --- | --- | --- |
| Challenge-owned (volatility range, normalizer sampling, step count, edge formula) | `crates/shared/src/config.rs` | upstream sync only. **Never** tuned to improve a local number — the grader runs upstream's values. |
| Ours, protocol-level (seed segments, search budget, grid levels, sim counts, fuzz-gate sample counts) | `config/bench.toml`, typed + fail-fast validated | a PR that also updates §2 |
| Ours, strategy-level (a family's free parameters) | the strategy's `lib.rs`, space declared in its `NOTES.md` | frozen before search (§2.4) |

No parameter appears in code without a §2 or `NOTES.md` citation.

### 3.2 The upstream boundary

`crates/**` and `programs/**` are upstream-owned. A local edit there is reverted by the
next sync and, worse, silently invalidates every number in `results/`. Our layer reaches
the simulator only through public APIs — `runner::run_batch_native`,
`engine::run_simulation_native`, `HyperparameterVariance::apply`, `GBMPriceProcess`.

`upstream/main` is currently **frozen**: `main..upstream/main` is empty and its last merge
is dated 2026-02-16. Measurement stability is therefore good in practice, but the boundary
rule stands — see §8 for what an upstream thaw would cost.

### 3.3 Reproducibility of a reported number

Any number quoted outside a throwaway console session must state: the seed segment, the
simulation and step counts, the execution path (native / BPF), and the commit sha. Reports
land in `results/<date>-<stage>.md`. A number without this provenance may not be cited in
an issue, a `NOTES.md`, or a ranking.

### 3.4 Budgets

Search: ≤300 points per family (§2.5). Disk: the fast path holds a single ~51 MB build
directory; the reference path's isolated directories are cleaned after use, since a
500-point search through the CLI would consume ~25 GB. `.build/` is gitignored.

## 4. System Architecture

### 4.1 Tech stack

Inherited from the challenge, not chosen by us — the grader runs upstream's harness, so
every entry here is a constraint rather than a decision.

| Piece | What | Why it is not ours to change |
| --- | --- | --- |
| Language | Rust 2021, Cargo workspace, `resolver = "2"` | The submission is a Rust source file the grader compiles. |
| Submission artifact | one `lib.rs` implementing `compute_swap`, built against `crates/submission-sdk` (`pinocchio`) | Fixed by the challenge's submission interface. |
| Execution | `solana_rbpf` (BPF) in `crates/executor`, or a natively compiled dylib via `libloading` for local speed | Both paths must agree; the native path exists only so local iteration is fast. |
| Simulation | `crates/sim`, parallelised with `rayon`; RNG is `rand` + `rand_pcg` (`Pcg64`) + `rand_distr` | Seeded and reproducible — the draw *order* in `crates/shared/src/config.rs` is load-bearing for seed reproducibility. |
| CLI | `crates/cli` (`prop-amm`), `clap` derive | `validate` / `run` / `build` are the local contract with the web submission. |
| Release profile | `lto = true`, `codegen-units = 1`, `opt-level = 3` | Benchmark numbers are only comparable under the same profile. |

Not present and deliberately so: no storage, no service, no deploy target. The "deploy" of
this project is pasting a `lib.rs` into the web UI.

**Added for our own layer:** nothing new at runtime. `tools/bench` uses `rayon`, `clap`,
`anyhow` and `serde`/`toml` (already workspace dependencies or std-adjacent). The paired
statistics of §2.1 — mean difference, standard error, t-interval — are ~30 lines and are
implemented inline rather than pulling a statistics crate whose version would become part
of every number's provenance.

### 4.2 Module layout

`AGENTS.md` §Architecture mirrors this section — keep them in sync. **(u)** marks
upstream-owned code: it changes only through `docs/GIT_WORKFLOW.md` § Upstream sync, and
editing it in a feature PR gets silently reverted by the next sync.

```text
crates/
├── shared/            (u) sim config + per-sim parameter sampling, normalizer CPMM
│                          reference, swap instruction ABI
├── executor/          (u) run a submission behind one interface: BPF (solana_rbpf) or
│                          native dylib
├── sim/               (u) the simulation itself — price process, arbitrageur, retail
│                          flow, order router, curve shape checks, engine loop.
│                          This is what produces the edge number.
├── cli/               (u) the `prop-amm` binary: validate / run / build / compile
└── submission-sdk/    (u) the pinocchio surface a submission's lib.rs links against
programs/
├── starter/           (u) BPF, outside the workspace — what a submission is copied from
└── normalizer/        (u) BPF, outside the workspace — the benchmark CPMM

strategies/                ours — one directory per candidate
├── README.md              the registry: id, name, source form, status, current numbers
├── 001-cpmm-fee/
│   ├── lib.rs             self-contained submission source; the committed fitted point
│   └── NOTES.md           provenance, mechanism, frozen parameter space, measured numbers
└── NNN-<slug>/            same shape; `NNNb-<slug>` for a §2.9 variant
tools/
└── bench/                 ours — the measurement layer (workspace member)
config/
└── bench.toml             ours — seed segments, search budget, grid levels, fuzz-gate
                              sample counts (§3.1)
results/
└── <date>-<stage>.md      ours — comparison snapshots, each carrying a commit sha
```

**Why strategies are files, not trait implementations.** The obvious "pluggable" design — a
trait with one implementation per strategy crate — is impossible here.
`crates/cli/src/commands/compile.rs:22` hardcodes the submission's `Cargo.toml` (only
`pinocchio`, `wincode`, `prop-amm-submission-sdk`) and `ensure_build_dir` copies exactly
**one** source file to `src/lib.rs`. A submission cannot link a shared strategy crate, and
`include!` would fail on the grader, which receives a single file. Every strategy is
therefore a self-contained `lib.rs`; modules *within* the file are fine.

The consequence is ~40–60 lines of boilerplate duplicated per strategy (entrypoint,
`NAME`/`MODEL_USED`, the `wincode` decode struct, fixed-point helpers). This is accepted for
v1: the duplicated block is small, copying is honest, and the committed file is exactly the
submission artifact, which keeps tag → submission traceable. Template-and-concatenate is
revisited only once ≥5 strategies share the same non-trivial numeric helpers (§7).

`strategies/` sits outside `crates/` so no candidate is ever mistaken for upstream code.

### 4.3 Key interfaces

| Seam | Contract | Load-bearing? |
| --- | --- | --- |
| `prop_amm_executor::SwapFn` = `fn(&[u8]) -> u64` | how bench hands a compiled candidate to the simulation | **Yes** — a plain fn pointer, not a closure, so bench telemetry (§2.7) must route through thread-local/atomic state rather than captured context |
| `runner::run_batch_native(sub_fn, sub_after, norm_fn, norm_after, configs, workers)` | bench's entry point into the simulation; accepts arbitrary configs | **Yes** — this is what makes grid mode (§2.3) possible without an upstream edit |
| `AfterSwapFn` = `fn(&[u8], &mut [u8])`, one slot per AMM | the only real-trade hook; upstream never calls it during quoting | **Yes** — the whole of L1 observability rests on it |
| `HyperparameterVariance::apply(&base, seed)` | seed → full regime, deterministic | **Yes** — lets bench label any result's regime with no upstream change |
| `GBMPriceProcess::new(...)` | seed → exact price path | Not used in v1; the basis of L2 (§2.7) |
| bench's fast compile path | source text → loadable dylib | **Deliberately not abstracted.** It duplicates upstream logic; the parity gate (§2.6) is the control, not an interface |
| `tools/bench/src/curve_checks.rs`'s mirror of `crates/sim/src/curve_checks.rs::submission_shape_violation` | `bench fuzz`'s (§2.9) shape check, off the frozen-search critical path | Duplicates *private* upstream logic (`mod curve_checks;`, not `pub`, so no import exists) with **no equivalent control** — unlike the fast compile path, nothing else in the system fails if this copy drifts from upstream's. Mitigated by one ported upstream regression test; residual risk logged in `docs/DEFERRED_ISSUES.md` |

### 4.4 Core flows

**Fitting one strategy family (M1, per issue).**

1. Author `strategies/NNN-<slug>/lib.rs` faithfully (§2.9); declare and freeze the
   parameter space in `NOTES.md` (§2.4).
2. `prop-amm validate` — interface, monotonicity, concavity, CU, native/BPF parity. A
   concavity failure here is cheaper than a mid-simulation panic later.
3. Search: ≤300 points, coarse grid → coordinate descent, screening seeds
   `1_000_000..=1_000_199`, common random numbers (§2.5).
4. Re-evaluate the winning point on full train, then validation.
5. Grid mode: 27-cell fragility matrix. Any cell significantly negative gets an explanation
   in `NOTES.md` (§2.3's fragility veto).
6. Parity gate: reproduce the committed point through `prop-amm validate` + `prop-amm run`;
   record in `NOTES.md` (§2.6).
7. Update `strategies/README.md`.

**Naming the winner (M2).** Freeze → rank on validation → one variant per top-3 → single
test-segment run for winner and runner-up → `results/` snapshot → §1.4 checked off.

### 4.5 State & recovery

The measurement layer is stateless. Storage inside a simulation is upstream's 1024-byte
buffer, zero-initialised per simulation and read-only during `compute_swap`. Nothing needs
to survive a crash: a bench run is fully determined by (source, config table, seed segment),
so any result is re-derivable by re-running it. The only durable artifacts are the committed
`strategies/**` sources, `NOTES.md` files, and `results/` snapshots.

## 5. Data & Observability

- **Per-simulation record:** `(seed, submission_edge)` from upstream, plus the regime
  reconstructed via `HyperparameterVariance::apply`, plus L1 counters (executed trade count
  and volume, per AMM) from the `after_swap` recorders (§2.7).
- **Derived per candidate:** paired mean difference vs reference with a t-interval; flow
  share; edge per unit volume; the 27-cell grid matrix; the `0..=999` observation row.
- **Reports:** `results/<date>-<stage>.md`, each carrying the commit sha, seed segment, sim
  and step counts, and execution path (§3.3). Snapshots are committed — they are the
  evidence behind §1.4, and a claim whose evidence was never committed is not a claim.
- **`NOTES.md` per strategy** is the durable record of *why*: provenance, fidelity
  self-assessment, frozen parameter space, search budget spent, fitted point, train /
  validation / parity numbers.
- **No logging or alerting infrastructure.** Runs are foreground, minutes long, and
  operator-observed.

## 6. Milestones

### 6.1 Structure

| Milestone | Deliverable | Success criterion |
| --- | --- | --- |
| **M0** | The measurement layer | bench reproduces `prop-amm run` per seed for starter (210.50 on `0..=999`); the 0-line is fitted and single-peaked; grid mode and L1 report |
| **M1** | Every listed strategy at a terminal state | each has a fitted point with train/validation/parity numbers, a recorded `wontfix`, or a recorded `Canceled` (§2.10) |
| **M2** | A named winner | one test-segment use; paired interval; regime slices; `results/` snapshot; §1.4 satisfied |

M0 is split into three issues so the measurement layer is validated *before* anything is
built on top of it (`WHI-1193` → `WHI-1194` → `WHI-1195`). The ordering is the point:
`WHI-1193` ends by cross-checking bench against the upstream CLI, and starter's 210.50 is
the only anchor produced by an upstream code path — the one chance to catch a bench that is
wrong in the same direction as its own tests.

### 6.2 The frozen strategy list

Frozen 2026-08-21 (WHI-1197). This is the complete list M1 iterates over — per §2.10,
nothing is added to v1 after this point, apart from the post-freeze exceptions recorded
below (`007`, `008`). Each entry's original material is filed under
`docs/references/<id>-<slug>/`, one directory per strategy (a directory rather than a
single file, since several entries are multi-file source trees); each directory carries
its own `README.md` with the four fields below plus a mechanism summary, known parameters,
and a fidelity note. That `README.md` is a **snapshot fixed at freeze time** — for the
2026-08-21 freeze that means `002`–`006`; for the post-freeze exceptions `007` and `008` it
means the point each one's own porting issue starts, since that is each entry's own freeze
moment — and it does not change once the porting issue starts. `NOTES.md` (§2.4, §2.9) is
the living record after that: it re-declares the parameter space in the porting issue's own
words and is the one that governs if the two ever drift, since it is what's actually frozen
before search runs.

| Id | Name | Source form | Original material | Known parameters (starting point, not frozen — §2.4) |
| --- | --- | --- | --- | --- |
| `002` | Orbic — **Canceled (WHI-1206)** | Solidity (to port) | `docs/references/002-orbic-flashbots/` — Flashbots' `ExamplePropAmm.sol` | `concentration ∈ [1, 2000)`; oracle-published `multX`/`multY`; 5% lock threshold |
| `003` | Piecewise Linear | source (Rust) + prose (blog) | `docs/references/003-piecewise-linear/` — `benedictbrady/prop-amm`'s on-chain program | `NUM_PRICE_POINTS = 7` / `NUM_SEGMENTS = 6` per side; per-segment liquidity is derived, not free |
| `004` | EWMA Dynamic Fee + Shock-Decay | source (Rust) + Solidity (richer port) + source (v3 extension) | `docs/references/004-ewma-shock-decay-fee/` — `lilaclilac09/pamm-a`'s own past competition submission | `SHOCK_THRESHOLD_1E9 = 5_000_000` (0.5%); vol EWMA α = 0.20; fee cap 100bps; `VOL_MULT`/`SHOCK_FEE_PER_STEP`/`SHOCK_DECAY_STEPS`/`BASE` per source |
| `005` | Vol-Adaptive CPMM Fee | source (Rust, direct submission shape) | `docs/references/005-vol-adaptive-cpmm-fee/` — `dcccrypto/percolator-perp-liquidity`'s `EdgeMax_CumVar.rs`, pinned before its later removal from that repo | `fee_bps = clamp(20 + 0.7·σ̂ + σ̂²/160, 20, 130)`; `COLD_FEE = 55`; `WARMUP_STEPS = 16` |
| `006` | Hedged PnL | prose (HackMD) | `docs/references/006-hedged-pnl/` — flagged: the doc's own scoring-metric framing does not match this repo's simulator (its volatility range does match); the portable content is its "Linear Price Impact Model" section | none — four cross-impact coefficients (`k++`,`k+-`,`k-+`,`k--`), no numeric anchor given |
| `007` | DODO PMM (`R = ONE`, arbitrageur-as-oracle) — **added post-freeze by exception (WHI-1219); supersedes `002`'s earmark, see below** | Solidity (to port) | `docs/references/007-dodo-pmm/` (created by WHI-1219, per its own snapshot-at-porting-start) — `DODOEX/contractV2` @ `2f1bcdac7ef1beee7599a756e2eed26732c2536d` (Apache-2.0) | `K_BPS ∈ [25, 10_000]` (curvature, 1e-4 units of `ONE`); `FEE_BPS ∈ [1, 500]` |
| `008` | Lagging-VWAP Directional Fee + Profile Ensemble — **added post-freeze by exception (WHI-1236); no citation of source numbers, see below; measured new M1 leader on validation, see §8; named the v1 winner over runner-up `003b`, see §9 (WHI-1226)** | source (Rust, direct submission shape) | `docs/references/008-lagging-vwap-fee/` (created by WHI-1236, per its own snapshot-at-porting-start) — `houseofjiao/prop-amm-challenge`'s own **current, live** competition submission, pinned `152697153d` | `ARB_K_BPS ∈ [0, 27_333]`; `COUNTER_K_BPS ∈ [0, 4_600]`; `TARGET_BASE_BPS ∈ [4, 80]`; `SIZE_K_BPS ∈ [0, 4_350]` (committed at the source's own unmodified `P0` anchor — `strategies/008-lagging-vwap-fee/NOTES.md` § 5 for why the 300-point search's own winner was not adopted) |

`000-normalizer` and `001-cpmm-fee` (§2.8) are the M0 baselines already landed
(`strategies/`) and are not part of this M1 list — they are the 0-line every entry above
is measured against, not additional candidates.

**`002` is Canceled**, per §2.10's removal clause: struck by owner decision rather than
carried to a measured negative result (`WHI-1206`). The mechanism depends on top-of-block
parameter republication via Flashbots' `PrioUpdateRegistry`; this harness exposes no
pre-arbitrage surface (four instruction tags, `after_swap` fires only on executed trades,
and a `sol_set_storage` call inside `compute_swap` is silently discarded), so re-anchoring
can only ever happen post-hoc. For a symmetric zero-fee curve that lateness is fatal, not
merely inconvenient: the arbitrageur has already moved the reserves to the fair price by
the time any re-anchor could run, so post-hoc re-anchoring is a **no-op**, not just late —
and the family has no fee axis at all, so the revenue model stays broken even granting the
primitive. `WHI-1206` carries the full argument, the per-sigma bleed table, and a recorded
dissent. The surviving idea — virtual-reserve amplification at a real spread, i.e. `001`
plus a concentration knob — was filed as a **v0.2.0 candidate**; that earmark is now
**superseded by `007`** (immediately below), which is that same idea ported as an M1 entry
rather than reopened as v0.2.0 work. `002`'s own mechanism was separately revived, not as a
submission but as an out-of-competition ceiling measurement (§10, `ceilings/C-orbic-oracle/`,
WHI-1247/WHI-1248).

**`007` was added post-freeze by exception** (`WHI-1219`, recorded in `WHI-1220`). The
freeze's purpose was still served at the time: the test segment (§2.2) was unspent, and
going from four surviving candidates to five is a small increase in selection bias, not a
list that keeps growing. It is admissible under the freeze as a **port** rather than an
original design (§1.3, §2.9) — this is the deliberate, recorded exception §2.10 describes,
not a general reopening. It **supersedes** the v0.2.0 earmark above:
"virtual-reserve amplification at a real spread, i.e. `001` plus a concentration knob" is
exactly DODO PMM collapsed to `R = ONE`, so that idea is absorbed into `007` rather than
left open as a separate future v0.2.0 issue.

**Measured and closed, not merely relocated (WHI-1235).** `007`'s own M1 result answers the
absorbed earmark rather than just carrying it forward: concentration is net-harmful at
every tested `K_BPS` short of its own `k = 1` CPMM boundary
(`results/2026-08-21-grid-007-dodo-pmm.md`, `strategies/007-dodo-pmm/NOTES.md` § Negative
result — see §8). "Virtual-reserve amplification at a real spread" is therefore a
**measured negative**, not an open v0.2.0 candidate waiting on `007` to run. §2.10's
addition clause does not reopen it without a fresh measurement.

**`008` was added post-freeze by exception** (`WHI-1236`), approved by the owner
2026-08-22, with the test segment (§2.2) still unspent at approval time — the same window
`007`'s exception used. Its reasoning is not `007`'s: `007`'s justification was
**provenance** (a port of published Apache-2.0 math is an M1-shaped entry, whereas the
same idea as our own invention would be v0.2.0 work). `008`'s justification is that it is
the only candidate this project has evaluated that **attacks an axis M1 measured as
*unattacked* rather than dead**: it holds the arb-vs-retail classifier that `WHI-1235`'s
own shock-surcharge finding (§8) says a repaired shock mechanism requires — event-driven
shock surcharges are mis-signed in this harness because they re-arm on large *retail*
prints, and repairing that needs a classifier fed by the fair price the interface does not
supply. `008` builds that classifier from the only fair-price substitute the interface does
permit — a deliberately lagging, Kalman-filtered trade VWAP.

**`008`'s provenance and licence differ from every prior entry.** The source is another
**live** competitor's **current** submission (`houseofjiao/prop-amm-challenge`, pinned
`152697153d`) — unlike `004` (a competitor's **past** submission) or `007` (a **licensed**
third-party library). **Neither that repository nor upstream carries a licence**
(`license: null` on both, verified via the GitHub API) — default all-rights-reserved.
Copying the material into `docs/references/008-lagging-vwap-fee/` for internal analysis
was explicitly approved (`WHI-1236` Step 0(a)); **verbatim submission of the artifact to
the challenge was deliberately left undecided** and returns as its own decision at the
release step (`WHI-1236` Step 0(b)).

**No number from that repository may be cited as evidence anywhere in this project.** Its
headline result was measured on their fork's own harness vintage, which predates
upstream's arb-direction fix: their shared history anchors at upstream's PR #17 merge
(`afa98be0b1`, 2026-02-12), one day before that fix landed (`97a1c67` et al.,
2026-02-13). The fix's own source comment says evaluating from the reserve ratio alone
"can be a misleading directional signal for non-CP strategies"
(`crates/sim/src/arbitrageur.rs`) — and `008` is exactly that class (directional fee
asymmetry via the arb/counter classifier above), so the fix targets precisely this
strategy's shape. Only our own re-measurement, starting at `008`'s pre-registered probe,
counts.

Two entries carry an explicit provenance caveat, read before porting:

- **`003`** — the piecewise-linear liquidity curve itself (7 points / 6 segments,
  self-replenishing) is confirmed in source. A second detail from the collected
  material — an oracle-staleness spread-widening backoff — traces only to the author's
  own blog post describing it as an unimplemented, exploratory mock-up with no formula
  given. Treat the curve as the faithful port; treat staleness backoff as, at most, a
  `003b` variant (§2.9).
- **`006`** — the source document titled itself as describing *this* challenge, and its
  stated volatility range (`U[0.01%, 0.70%]`) matches `crates/shared/src/config.rs`'s
  `gbm_sigma_min`/`gbm_sigma_max` exactly — but its stated **scoring metric** ("Hedged
  PnL", a terminal-inventory formula) does not match this repo's actual per-trade
  average-edge metric (§2.1). Per §2.9's provenance contract, this is filed as-is rather
  than silently corrected; the porting issue treats the doc's linear-price-impact
  mechanism as the thing to port, not its claimed scoring rule.

One M1 issue per strategy above is opened per `docs/agents/issue-template.md`, each
`blockedBy` the last M0 issue (`WHI-1195`): `WHI-1206` (`002`, **Canceled** — see above),
`WHI-1207` (`003`), `WHI-1208` (`004`), `WHI-1209` (`005`), `WHI-1210` (`006`). `007`'s
issue is `WHI-1219` — opened post-freeze, per the exception recorded above and in
`WHI-1220`. `008`'s issue is `WHI-1236` — opened post-freeze, per the exception recorded
above and in `WHI-1237`.

## 7. Rejected Alternatives

Each of these was considered and rejected during the design interview. An agent whose
output contradicts an entry here must flag it explicitly rather than silently override.

| # | Rejected | Why |
| --- | --- | --- |
| 1 | **Strategies as a trait with one impl per crate** | Impossible: the grader compiles one file, and `compile.rs` hardcodes the dependency set (§4.2). |
| 2 | **Template-and-concatenate strategy sources** | Removes ~50 duplicated lines but adds a generation step, and the submitted artifact stops being the authored file. Revisit at ≥5 strategies sharing non-trivial helpers. |
| 3 | **Generating the submission without committing the generated file** | Breaks tag → submission traceability (`AGENTS.md` § Promotion lanes): a leaderboard score could not be traced to a tree. |
| 4 | **Leaderboard rank as the success criterion** | A black-box single sample. A ranking that cannot be reproduced cannot support "best curve". Kept as optional post-hoc validation (§1.3). |
| 5 | **A robustness metric (worst regime / 5th percentile) as the primary** | Would optimise something the challenge does not reward. Robustness is retained as a veto instead (§2.3). |
| 6 | **Plain `avg edge`, no regime slicing** | Averages away the corners; would select a curve that fails in a quarter of the parameter space, invisibly. |
| 7 | **Grid mode only** | Balanced coverage, but its numbers are not comparable to the graded distribution. |
| 8 | **Comparing strategies at their original constants** | Measures whose defaults suited this simulator, and discards good forms that were never fitted here (§2.4). |
| 9 | **Two-stage: point screening first, fit only the finalists** | The screening ranking is untrustworthy for exactly the reason in #8, so it can eliminate the eventual winner before it is ever fitted. |
| 10 | **Search through the CLI compile path only** | 7–10 s and 51 MB per point caps search depth at the disk limit (~25 GB for 500 points). Rejected in favour of a fast path plus the parity gate (§2.6). |
| 11 | **Exposing `crates/cli`'s compile module as a library** | Would remove the duplication in #10 but requires editing two upstream files, renegotiated at every sync (§3.2). |
| 12 | **Two seed segments with usage accounting** | Cheaper, but leaves the winner's number carrying selection bias with only prose as mitigation (§2.2). |
| 13 | **Training on `0..=999`** | The most direct leaderboard optimisation, but destroys the only distribution-matched honest estimate we have. |
| 14 | **Search budget scaled by parameter dimension** | Hides a real drawback (hard-to-tune curves) and makes the scaling function itself an unsourced parameter (§2.5). |
| 15 | **Unbounded search until convergence** | Convergence detection is unreliable under noise; in practice it becomes "whoever searched longest wins". |
| 16 | **Improving strategies in place during porting** | Turns the ranking into a measure of porting effort. Improvements become explicit variants (§2.9). |
| 17 | **Delivering a faithful *and* an improved version of every strategy** | Doubles M1 for little gain — most strategies have no interesting improvement, so the second track becomes ceremony. |
| 18 | **L2 observability (full retail/arb split) in v1** | It is a copy of upstream's call ordering that fails silently if that ordering differs. Deferred until a strategy is actually stuck (§8). |
| 19 | **Editing `crates/sim` to emit richer results** | Reverted by the next sync, and invalidates every committed number (§3.2). |
| 20 | **Threshold or time-boxed convergence** | Neither yields a defensible freeze point for a single-use test segment (§2.10). |
| 21 | **No baseline / relative ranking only** | Cannot answer "is any of this better than the simplest thing that works", plausibly the single most valuable finding (§2.8). |
| 22 | **A single M0 issue** | Lands the measurement layer and its cross-check together, so a bench wrong in the same direction as its tests passes unnoticed (§6.1). |

## 8. Known Risks & Open Questions

**Risks**

1. **The fast compile path drifts from upstream's.** It re-implements the shim injection and
   `unsafe` rejection of `compile.rs`. An upstream change would move our numbers silently.
   *Mitigation:* the §2.6 parity gate on every committed point. *Residual:* search-phase
   numbers between gates are unverified.
2. **bench and its own tests can be wrong together.** Both are ours. *Mitigation:* the
   starter 210.50 per-seed cross-check (`WHI-1193`) and the single-peak self-check on the
   fee family (`WHI-1194`) — two independent anchors produced by upstream code paths.
3. **The winner's test number is still a single sample.** Using the test segment once means
   no repeat measurement. *Mitigation:* report the paired interval, not just the point
   estimate; the runner-up is measured in the same pass for scale.
4. **Equal search budget penalises high-dimensional families** (§2.5). Accepted, but it must
   be stated explicitly whenever such a family loses, or we will have concluded something
   about curve quality that is really about budget.
5. **An upstream thaw invalidates everything measured.** `upstream/main` has not moved since
   2026-02-16 and `main..upstream/main` is empty, so the exposure is currently low. But a
   sync that touches `curve_checks`, `router` or `arbitrageur` tolerances retires every
   number in `results/` at once, and — because the test segment is single-use — leaves the
   winner undecidable. *Mitigation:* after any upstream sync, re-run all baselines and all
   completed strategies, snapshot to `results/`, and treat the test segment as spent
   (v0.2.0 draws a fresh one).
6. **Provenance of prose-only strategies is unverifiable.** A second-hand description may
   misstate the mechanism; we would rank a strategy nobody proposed. *Mitigation:* the
   fidelity self-assessment in `NOTES.md`.
7. **Concavity is a runtime panic, not a score.** Structurally incompatible designs can
   absorb unbounded effort. *Mitigation:* the `wontfix` terminal state (§2.9) for a family
   that panics **everywhere** in its frozen range. A family that panics only in part of its
   range (two of M1's five live families do — WHI-1207/1210; `002`/WHI-1206 is Canceled,
   §6.2) is not this case: `bench fit` handles it per-point via the `Invalid` outcome
   instead (§2.5, WHI-1213).

**Open questions**

1. **Whether the leaderboard is still accepting submissions.** Does not affect v1 (§1.3),
   but decides whether §1.4's winner ever gets submitted from a tag.
2. **Does the grader use seeds `0..=999`?** Assumed from `BASELINE_SIMS`/`BASELINE_STEPS`
   defaults, not confirmed. Only affects how the observation row is interpreted.
3. **Are `retail_arrival_rate` and `retail_mean_size` worth adding to the grid?** Held at
   defaults for readability (§2.3); a flow-sensitive strategy might need them.
4. **When does the duplication in §4.2 justify generation?** Trigger set at ≥5 strategies
   sharing non-trivial numeric helpers; the count is a guess.
5. **Is L2 needed?** Deferred (§2.7). The trigger is a strategy whose loss cannot be
   diagnosed from flow share and edge per unit volume alone.

**M1 measured findings (WHI-1235)**

M1 produced several conclusions that were *measured*, not merely speculated, and that
change what someone would try next. Recorded here so a reader gets them without digging
through `strategies/*/NOTES.md` and closed issues.

*Open opportunities*

1. **The largest measured, unexploited opportunity in the project.** Grid cell seeds are
   fixed-addressed (`base + cell*1000 + i`, WHI-1195), so raw candidate averages are
   comparable across grid reports. Comparing `results/2026-08-20-grid.md` (starter@500)
   against `results/2026-08-21-grid-004-ewma-shock-decay-fee.md` (the fitted M1 winner) at
   `gbm_sigma = 0.0070`: a flat 500 bps out-earns the winner in **seven of the nine**
   high-sigma cells (all but cells 23 and 26) — by margin, descending: cell 2 by +322.5,
   cell 11 by +252.1, cell 8 by +200.9, cell 5 by +142.9, cell 20 by +138.4, cell 17 by
   +125.5, and cell 14 by a smaller +50.6. The six largest margins alone dwarf the winner's
   entire
   +44.50 aggregate advantage over the 0-line (`results/2026-08-20-fit-001-cpmm-fee.md` vs.
   `results/2026-08-21-fit-004-ewma-shock-decay-fee.md`, validation avg edge). **Nobody has
   captured it.** `004b` (WHI-1223) was built specifically to, by decoupling the calm fee
   level from the high-sigma response slope, and it failed measurably (screening P1
   −100.99, P2 −14.13; kill rule fired before any search, 0 of 300 budget spent —
   `strategies/004b-floor-subtracted-ewma-fee/NOTES.md` § Negative result). The reason that
   attempt failed is finding 3 below — the headroom is real, one designed attempt to reach
   it failed, and the failure is itself now measured.
2. **Competitor-blindness is what the class is actually missing — triple-corroborated.**
   Three independent variant assessments converged on the same binding constraint from
   different directions: `005`'s own 8-cell loss cluster, all with `norm_liquidity_mult >=
   1.0` (`strategies/005-vol-adaptive-cpmm-fee/NOTES.md`), is left untouched by `005b`'s
   estimator correction — the cluster sits on the parent's grid, not on any change `005b`
   made; `003b`'s cells 21/22 are inner-margin cells no candidate change reaches, confirmed
   after the fact at −30.30 / −30.30 (`results/2026-08-22-grid-003b-wider-band-deeper-book.md`);
   `004b`'s floor-decoupling only partially reaches the `fee=80` calm cells. No submission
   can observe `norm_fee_bps` or `norm_liquidity_mult`, but flow share is inferable from
   `after_swap` call frequency and executed volume — exactly what L1 already measures
   (§2.7). Leading v0.2.0 candidate, now measurement-backed rather than speculative; **not
   scheduled** — the v0.2.0 list is unfrozen and an issue outside a frozen list is an
   orphan (§2.10), so no v0.2.0 issue is opened against this finding.

   **External corroboration, vintage-bound (WHI-1236) — third-party reported, not measured
   under §3.3's protocol.** `008` — added post-freeze by exception (§6.2, WHI-1237) once its
   provenance gate was approved — cites `houseofjiao/prop-amm-challenge`'s own
   `LEARNINGS.md` (source pinned at `152697153d`), a measured dead-end corpus from that
   author's tuning campaign on their **pre-2026-02-16 harness**. It reports this same
   flow-share-inference premise measured directly there: `cnt_ema -> norm_fee` r = 0.662 and
   `arb_prefers_us -> norm_fee` r = 0.828 — the inference is real and strong there too. That
   author then tried to exploit it with a fee boost and it **regressed at their own
   optimum**, via a named mechanism: a slow-decaying fee floor overshoots the opponent's fee
   and routes retail away. These figures carry none of §3.3's own-measurement provenance (no
   seed segment, sim/step count, or execution path is available for them) precisely because
   they are *not* a number produced under this project's protocol — they are cited as a
   third party's report for context only, not as evidence for any claim about this harness.
   It is nonetheless a recorded failed attempt with a named failure mode, so a v0.2.0
   attempt at this axis does not have to restart from zero.

   **Not answered by `008`'s own port, despite the shared source.** `008` accumulates a
   flow-share-shaped signal of its own (`cnt_ema`, an EMA of trades-per-step, storage offset
   144) but never reads it into any fee or profile decision — the source's own
   `LEARNINGS.md` records that exploiting it via a fee boost regressed at their optimum
   (immediately above), and the committed mechanism reflects that: `cnt_ema` is persisted
   but dead for decision-making. `008`'s own strong measured result (finding 5 below) is
   real, but it is evidence for a *different* gap — the arb-vs-retail directional classifier
   finding 5 names as missing, not this finding's flow-share/competitor-inference axis. This
   finding's own v0.2.0 candidate remains unattacked by any measured M1 entry.

*Resolved negatives*

3. **Floor subtraction on the own-trade-impact signal is dead in this harness.**
   `results/2026-08-22-estimator-probe-005b.md` § Added scope measured `004`'s `ewma_vol`
   against a floor sweep, bucketed by true-sigma tercile and volume-weighted (the weighting
   that matters, since the fee only bites where flow arrives): at the two floors `004b`
   actually probed (30 for P1, 25 for P2), 31–44% of executed volume in the low-sigma
   tercile alone has its residual zeroed (44.1% ≤30bps, 30.7% ≤25bps) — and a material,
   if smaller, share in the mid tercile too (30.9% ≤30bps, 17.3% ≤25bps; high tercile:
   12.8% / 5.7%), pinning the fee at `BASE` exactly where the parent already loses to
   `001@66`. This substantiates `004b`'s own closing hypothesis (WHI-1223), which its
   `NOTES.md` correctly flagged as unverified at the time. Generalised form: any
   floor-subtraction scheme on this signal is dead here, because the own-trade-impact floor
   sits inside the fee range that matters rather than below it.
4. **Estimator quality is not the binding constraint for volatility-adaptive fees.** `005b`
   (WHI-1225) corrected a genuine defect in `005`'s estimator — dividing accumulated
   variance by sample `count` rather than elapsed steps, which fails to produce the
   per-step quantity the source's own comment claims (median `count/n_steps` on the
   low-sigma tercile is 0.42, i.e. `E[gap] ~ 2.4`). The correction worked as an estimator
   (Spearman vs. true `gbm_sigma`: 0.508 → 0.665) and lost as a strategy: paired −19.29
   [−26.69, −11.90], n=200, CI excluding zero, at the parent's own fitted map
   (`results/2026-08-22-compare-005b-elapsed-steps-divisor-fix-vs-005-vol-adaptive-cpmm-fee.md`).
   Cause: a 35.6% average `sigma_hat` reduction across the whole screening population
   (`strategies/005b-elapsed-steps-divisor-fix/NOTES.md`) — far larger and more broadly
   distributed than a calm-only effect — against a fee map whose slope was calibrated to
   the inflated signal; dynamic-range decompression was only 8.72%, below the bar for
   expecting a refit to recover it. General form: for this class, a more accurate
   volatility estimate is not automatically a better strategy — the fitted slope absorbs
   the bias, so correcting the bias without refitting is strictly harmful, and the
   improvement available from better estimation (8.72% decompression) is small next to what
   the class is actually missing (finding 2).

   **Independent convergence, vintage-bound (WHI-1236) — third-party reported, not measured
   under §3.3's protocol.** A second, unrelated codebase reached the same conclusion from
   the opposite direction and made it structural: `008`'s source
   (`houseofjiao/prop-amm-challenge`, source pinned at `152697153d`; added post-freeze by
   exception per §6.2/WHI-1237) reports, on that author's own
   **pre-2026-02-16 harness**, in its own `LEARNINGS.md` § "Signed Flow Price Estimation",
   that making *their* price estimate more accurate reduced *their* edge, because in their
   mechanism the deviation term `(spot - p_ref)^2` **is** the fee signal — a lagging
   estimate is load-bearing there, not a defect. Two codebases on two different harness
   vintages, two different mechanisms, the same conclusion stated at class level rather than
   at the level of one family: a more accurate volatility or price estimate is not
   automatically a better strategy for this class of fee — this finding's own `005b`
   measurement (immediately above) is the one that counts as evidence for *this* harness;
   the second is corroboration only.

   **`008`'s own port and measurement (WHI-1236, complete) is a strong result for a
   mechanism *built around* the lagging-estimate premise, though not itself an ablation of
   it.** The lagging VWAP is not incidental to `008`'s mechanism — the source's own
   `LEARNINGS.md` states it is deliberately kept lagging for the same reason cited above —
   but `008`'s own measured edge is for the mechanism as a whole (the lagging VWAP, the
   directional classifier built on it, the fee ratchet, and the 3-profile ensemble
   together); no lag-vs-accurate-estimate ablation was run here, so this number is not
   directional evidence isolating the lag's own contribution the way the two corroborations
   above are. What it *is* — carrying full §3.3 own-measurement provenance, unlike either
   corroboration — is measured, first-party evidence that a family holding this premise
   performs strongly on this harness: validation avg edge **503.907498**, the largest margin
   over the current leader of any M1 entry (paired **+57.61** `[53.21, 62.01]` over `004`,
   n=1,000).
5. **Two axes measured net-harmful, so nobody retries them.** Concentration / virtual-reserve
   amplification (`007`, WHI-1219): net-harmful at every tested point; the family closed at
   its own `k = 1` CPMM boundary (`results/2026-08-21-grid-007-dodo-pmm.md`,
   `strategies/007-dodo-pmm/NOTES.md` § Negative result) — this also closes the v0.2.0
   earmark §6.2 recorded when `002` was cancelled, per §6.2's own update above.
   Event-driven shock surcharge (`004`'s own search): ablated to exactly 0 by the search
   (`SHOCK_FEE_PER_STEP_BPS = 0` at the fitted point, `results/2026-08-21-fit-004-ewma-shock-decay-fee.md`;
   `strategies/004-ewma-shock-decay-fee/NOTES.md` § Search), with a mechanism — it re-arms
   on large *retail* prints, raising the fee right after uninformed flow, and its per-trade
   decay makes the surcharge's duration a function of arrival rate. Any repair needs an
   arb-vs-retail classifier, which needs the fair price, which the interface does not
   provide — so a repair would be a new strategy family, not a variant.

   **`008` (WHI-1236, complete) is a new family holding the named classifier, though not a
   full repair of this finding's own shock-surcharge complaint specifically.** Its
   `compute_swap` classifies each quote's direction against a deliberately lagging
   Kalman-filtered VWAP — the fair-price substitute this finding says the interface
   otherwise withholds — surcharging the presumptively-informed direction (`+arb_k*dev^2`)
   and rebating the presumptively-uninformed one (`-counter_k*(...)*dev^2`)
   (`strategies/008-lagging-vwap-fee/lib.rs::quote_profile`). But the classifier only gates
   that `dev^2` term: the raw `shock_read_k*shock_hat`/`tox_read_k*tox_hat` additive reads
   in the same formula are unconditional, applied to both directions exactly like this
   finding's own complaint about `004`'s undirected shock surcharge — so `008` adds a
   directional layer alongside the undirected one this finding named, rather than replacing
   it. Measured at the source's own unmodified anchor point: validation avg edge
   **503.907498**, a paired **+57.61** `[53.21, 62.01]` over the current leader `004`
   (n=1,000, CI excludes zero) and a 26-of-27 win rate on the grid fragility matrix against
   `001-cpmm-fee` — the largest margin of any M1 entry, and this project's own measurement
   under §3.3's protocol. It does not, by itself, isolate how much of that margin comes from
   the directional classifier specifically versus the rest of the mechanism (the still-
   undirected tox/shock reads, the fee ratchet, the 3-profile ensemble) — no ablation
   separating them was run — but it is direct evidence that a family holding a version of
   this finding's own named gap performs strongly here, which is more than this finding had
   before `008`.

*Protocol lesson*

6. **Pre-registered stop rules saved most of the search budget.** Three of the nine
   non-baseline strategy entries (§6.2 plus its variants, excluding the `000`/`001`
   baselines) closed on a pre-search measurement without running their 300-point search:
   `004b`'s numeric kill rule literally fired (WHI-1223); `005b`'s Probe A survived its own
   literal kill conditions, but a paired `bench compare` CI computed at the probe stage
   showed a real net loss (WHI-1225); `007` closed at its own Step 0.5 boundary hit
   (WHI-1219). In each of these three, a number produced before the search phase — not a
   subjective argument — is what closed the lane. `002` is a fourth, differently-mechanised
   case: it never reached a probe at all, being struck by **owner decision** before any
   measurement (§2.10's removal clause, §6.2) — worth keeping distinct from the other three,
   since a governance call and a numeric kill rule are not the same protection. `003b` is the
   counter-case that justifies the two-tier band: its P3 probe landed at 421.673537, in the
   discretionary range (`strategies/003b-wider-band-deeper-book/NOTES.md`; nearby search
   points in the same region are in `results/2026-08-22-fit-003b-wider-band-deeper-book.md`),
   and the full search then produced the portfolio's best number. Also worth recording:
   bit-exact containment of the parent's fitted point caught real problems, and the one
   case where it could not be bit-exact (`005b`, because the estimator itself changed) is
   exactly where the near-exact fallback and its predicted-gap threshold earned their place
   — the 0-line point reproduces at 385.63 against a predicted 385.97 ± 1, deviation −0.34,
   well inside the family's own tolerance (`strategies/005b-elapsed-steps-divisor-fix/NOTES.md`
   § Probe B).

7. **A pre-search anchor can beat the search that was supposed to refine it — the inverse
   of lesson 6, first seen in `008` (WHI-1236).** Every other entry's P0-style probe either
   killed the lane (lesson 6) or handed off to a search expected to improve on it. `008`'s
   own 300-point search converged (172/300 points, 0 invalid) to a point measurably *worse*
   on every segment than the source's own unmodified anchor, which the search's coarse grid
   never evaluates at all (`strategies/008-lagging-vwap-fee/NOTES.md` § 5) — because the
   family's own profile-ensemble switcher makes changing the searched profile's constants
   also change how often that profile gets selected against two other profiles held fixed
   at the source author's own jointly-tuned values, a differently-shaped surface than the
   one those values were tuned against. The anchor was committed instead of the search's
   own winner, with the reasoning recorded rather than silently substituted. Worth watching
   for in any future ensemble-shaped family: an "improve one member, hold the rest fixed"
   search can converge to a real but inferior local optimum while a jointly-tuned anchor sits
   unexplored in the same declared space.

## 9. v1 Closure — M2 Test-Segment Result (WHI-1226)

§2.10 step 4, the last step of v1: rank the frozen list on validation, spend the `test`
segment exactly once on the winner and runner-up, report the paired interval and regime
slices, and check off §1.4. This section is that record.

### 9.1 Candidate set — corrected before the test segment was touched

WHI-1226's own working comment (posted ~12:05 +0800, 2026-08-22) named the pair as `003b`
(winner, 447.22 validation) vs `004` (runner-up, 446.30), and framed the whole report around
a near-tie "indistinguishable on held-out data" outcome. Strategy `008` (WHI-1236/1237, a
second post-freeze §2.10 exception, approved while `test` was still unspent) merged
~3 hours **after** that comment (`77bef05`) and is not in its table. Recomputing the
validation ranking from every committed `results/` fit report (per this issue's own "do not
rely on this issue's table — verify it" instruction) gives:

| Strategy | validation avg edge | source |
| --- | --- | --- |
| **008** Lagging-VWAP Fee | **503.907498** | `results/2026-08-22-fit-008-lagging-vwap-fee-anchor.md` |
| **003b** Wider Band x Deeper Book | **447.224660** | `results/2026-08-22-fit-003b-wider-band-deeper-book.md` |
| 004 EWMA Dynamic Fee | 446.297129 | `results/2026-08-21-fit-004-ewma-shock-decay-fee.md` |
| 003 Piecewise Linear | 432.445900 | `results/2026-08-21-fit-003-piecewise-linear.md` |
| 005 Vol-Adaptive CPMM Fee | 425.946116 | `results/2026-08-21-fit-005-vol-adaptive-cpmm-fee.md` |
| 007 DODO PMM | 403.26 | `strategies/007-dodo-pmm/NOTES.md` § Consolidated segment table — no dedicated `fit-007` report exists, since `007` closed at its own Step 0.5 boundary hit without running the 300-point search (§2.9, §8 finding 6) |
| 001 CPMM @66 (0-line) | 401.800851 | `results/2026-08-20-fit-001-cpmm-fee.md` |
| 006 Hedged PnL | 379.350266 | `results/2026-08-21-fit-006-hedged-pnl.md` |

`008` is eligible: added post-freeze by a recorded owner-approved exception while `test` was
unspent (§6.2, §2.10), reached a terminal state (fitted, committed at the source's own P0
anchor — `strategies/008-lagging-vwap-fee/NOTES.md` §5, §8 lesson 7 above), and its
503.907498 is our own §3.3-provenance measurement verified to match the committed `lib.rs`
constants exactly — not a citation of the source repository's own numbers, which are barred
from evidentiary use for an unrelated reason (their pre-fix harness vintage, §8) and which
this ranking does not rely on. **The real test-segment pair is `008` (winner) and `003b`
(runner-up)**, recorded as a correction on WHI-1226 before `bench compare --segment test`
was invoked, per this issue's own "decide before running" rule. The superseded comment's
`003b`-vs-`004` "inside the noise floor" framing still holds as a true statement about
second vs. third place (per the corrected table above); it does not describe the actual
pair below, and it is exactly why `003b`'s own runner-up status carries a caveat — see §9.6.

§2.10 step 3 ("open one variant for each of the top three") was already satisfied before
this issue started: `004b`, `003b`, and `005b` are exactly the variants opened for the
three strategies that led validation once M1 had run (`004` 446.30, `003` 432.45, `005`
425.95) — see each variant's own porting issue and `NOTES.md`
(`004b-floor-subtracted-ewma-fee`, `003b-wider-band-deeper-book`,
`005b-elapsed-steps-divisor-fix`); §8 finding 6 discusses all three variants' (`004b`,
`005b`, `003b`) and `007`'s outcomes but is not itself the record of when each was opened.

### 9.2 The test-segment run — spent exactly once

```
cargo run -p prop-amm-bench --release -- compare \
  --candidate strategies/008-lagging-vwap-fee/lib.rs \
  --reference strategies/003b-wider-band-deeper-book/lib.rs \
  --segment test --i-am-spending-the-test-segment
```

Run from a detached scratch worktree at `77bef05` (`bench compare`'s isolated-build path
requires this outside a nested `.claude/worktrees/` checkout). Result, committed at
`results/2026-08-22-compare-008-lagging-vwap-fee-vs-003b-wider-band-deeper-book-test.md`:

- `008` (candidate) avg edge **530.83**; `003b` (reference) avg edge **469.09**.
- Paired mean difference **+61.743094**, 95% CI **[56.869632, 66.616555]**, n=1000.

**The margin is decisively outside the ~±4–5 cross-family noise floor this issue itself
cites** — the point estimate (+61.74) is roughly 12–15x that floor, and even the CI's own
lower bound (56.87) sits more than 11 floor-widths from zero; the floor describes how far a
truly-tied pair's margin would plausibly land from zero, not the width of this CI (which is
a separate quantity, ±4.87, of the same rough magnitude as the floor and not itself
evidence either way). This is **not** the "indistinguishable" outcome the superseded
candidate pair was headed toward: **`008` beats `003b` on held-out data, clearly.**

Before spending `test`, the same invocation was rehearsed once on `--segment validation`
(reusable) to confirm the harness end-to-end: it reproduced `008`'s and `003b`'s
already-committed validation numbers exactly (503.91 / 447.22), and additionally produced a
direct paired validation-segment comparison of the two (previously only compared as
committed averages against each other, not as a paired `bench compare` run: `008` had been
paired via `bench compare` against `004` only, and `003b` had only ever been paired against
`001` at grid mode's per-cell resolution, not as a single whole-segment aggregate) — paired
**+56.682838** `[52.176061, 61.189614]`, n=1000, committed at
`results/2026-08-22-compare-008-lagging-vwap-fee-vs-003b-wider-band-deeper-book-validation.md`.
The test-segment margin (+61.74) is consistent with, and slightly larger than, the
validation-segment margin (+56.68) for the same pair — no sign of overfitting to validation.

### 9.3 Regime slices

Two different measurements bear on this, and they answer different questions — kept
separate rather than merged into one "leads/loses" claim, since they disagree on where (if
anywhere) `008` fails to lead:

- **The direct paired comparison** (the `test`-segment `compare` run above, 27
  sampling-tercile bins spanning the *range* of each axis, not a single point) shows `008`
  with a **positive point estimate in all 27 bins** — on this measurement it does not lose
  to `003b` outright anywhere, corner included. Two bins' 95% CIs include zero, meaning the
  pairing does not statistically separate the two families there even though the point
  estimate still favours `008`: `fee=High liq=High sigma=Low` (n=39, diff 3.69
  `[−2.27, 9.64]`) and `fee=High liq=High sigma=High` (n=45, diff 7.39 `[−2.92, 17.69]`). The
  adjacent `fee=High liq=High sigma=Mid` bin *does* separate them (diff 7.20
  `[3.73, 10.67]`, excludes zero) — so this is not a uniformly unresolved corner of the
  tercile grid, only two of its bins are ties, and one of those two (`sigma=Low`) is a
  different, calmer regime from the high-sigma corner discussed next, not the same address.
- **The near-unsolved cell** — `norm_fee_bps = 80` x `norm_liquidity_mult = 2.0` x
  `gbm_sigma = 0.0070` (grid cell 26, the exact deterministic address — not the same thing
  as a tercile bin above, which buckets a *range* of values rather than this one point) —
  stays negative against the `001` 0-line for every fee-dynamics entry in the portfolio
  except one: `003` −19.00, `004`/`004b` −41.62, `005` −120.89, `006` −191.24, `003b`
  −13.65 (`[−19.55, −7.76]`), `008` −35.99 (`[−43.10, −28.88]`). The sole exception is `007`,
  which collapses to a near-CPMM boundary (`k≈1`) at this cell and edges `001` by a narrow
  **+0.87** `[0.69, 1.05]` (`results/2026-08-21-grid-007-dodo-pmm.md` cell 26) — consistent
  with `007`'s own closure at that same `k=1` boundary (§2.9, §8 finding 6) rather than a
  distinguishing feature of any fee curve. Both `008`'s and `003b`'s absolute edges here are
  positive (116.44 / 138.78) — the loss is relative to `001`, not an outright negative edge.
  This is not a defect introduced by the winner: `008` loses here by a wider margin than the
  runner-up does, worth stating plainly rather than glossing over because `008` otherwise
  wins everywhere else — including, per the point above, every bin of the direct
  `008`-vs-`003b` pairing.

### 9.4 Observation row (reporting only — not a decision input, §2.2)

`--segment observation` (reusable), same pair, committed at
`results/2026-08-22-compare-008-lagging-vwap-fee-vs-003b-wider-band-deeper-book-observation.md`:
`008` avg edge **501.57**, `003b` avg edge **446.60** (matching this issue's own prior
table exactly), paired **+54.963301** `[50.877495, 59.049108]`, n=1000. Leaderboard-comparable
figure only; it did not, and must not, influence the winner/runner-up choice above.

### 9.5 §1.4 success criteria — checked off

1. ✅ **Parity gate.** `tools/bench` reproduces `prop-amm run` per seed for the starter
   program (WHI-1193/1194, `results/2026-08-20-parity-001-cpmm-fee.md` and every
   subsequent `bench parity` run against a committed strategy).
2. ✅ **0-line established.** `001-cpmm-fee` fitted under the full protocol; fee↔edge is
   single-peaked (`results/2026-08-20-fit-001-cpmm-fee.md`, §2.8).
3. ✅ **Every frozen-list strategy reached a terminal state.** `002` Canceled by owner
   decision (§6.2); `003`, `004`, `005`, `006` fitted via the full 300-point search with
   train/validation numbers; `007` reached a terminal state via its own pre-registered
   Step 0.5 probe *without* running the search (§6.2, §8 finding 6) — §1.4 item 3 accepts an
   explicit recorded stop as terminal, not only a completed search. `008` ran its own
   300-point search (172/300 spent, 0 invalid) but is committed at the source's own
   pre-registered anchor rather than the search's own winner, which measured worse on every
   segment (§8 lesson 7, §9.6) — still fitted and terminal, just not at the search's own
   argmax. The §2.9 variants opened under §2.10 step 3 (above) are likewise terminal: `003b`
   fitted via its own search;
   `004b`, `005b` closed as pre-registered negatives with 0 of their 300-point budgets spent
   (§2.9/§2.10).
4. ✅ **Winner named from a single use of `test`.** `008`, paired **+61.74 `[56.87,
   66.62]`** over runner-up `003b`, n=1000, regime slices reported (§9.2–§9.3 above).
   **Does `008` beat the best fixed-fee CPMM?** Yes — by **+102.11 on validation**
   (503.907498 vs. `001`'s 401.800851), the same segment every other entry's 0-line margin
   in this project is reported on (§2.8); `001` is not re-measured on `test` since the
   protocol reserves it for the winner/runner-up pair only (§2.10 step 4 names that pair
   specifically; §2.2 establishes `test`'s single-use rule more generally; WHI-1226's own
   "one run, two candidates" restates both — a third candidate touching `test` would
   reintroduce the selection bias the segment exists to remove).
5. ✅ **`results/` holds the snapshot.** Three reports committed this issue, each carrying
   commit sha `77bef05`, segment, sim/step counts, and execution path (§3.3):
   `-validation.md`, `-test.md`, `-observation.md` (all three: `008` vs `003b`), on top of
   the pre-existing grid/fit reports cited throughout this section.

**v1 is done.**

### 9.6 What the ranking rests on — required annotations

- **`008`'s point is a pre-search anchor, not its own search's winner, and its provenance
  is the least clean of any M1 entry.** Committed at
  `(ARB_K_BPS=5200, COUNTER_K_BPS=1000, TARGET_BASE_BPS=16, SIZE_K_BPS=2900)` — the source's
  own unmodified `P0` anchor; the 300-point search converged to a point measurably *worse*
  on every segment (§8 lesson 7) and was correctly not adopted. The source itself
  (`houseofjiao/prop-amm-challenge`, pinned `152697153d`) is another **live** competitor's
  **current** submission, unlicensed (`license: null`, all-rights-reserved by default) —
  unlike `004` (a past submission) or `007` (a licensed library) (§6.2). No number from that
  repository is used as evidence anywhere in this ranking; `008`'s 503.907498/530.83/501.57
  above are this project's own §3.3 measurements at the anchor point.
- **`003b`'s number rests on three weaker links, stacked** — the noise-floor link changes
  what it is evidence *of*, but does not disappear now that the actual pair is `008`/`003b`
  rather than `003b`/`004`: its parent's parameter space was re-frozen once after the
  original range lost catastrophically everywhere (screening −17,027 to −20,053,
  `results/2026-08-21-fit-003-piecewise-linear-original-range-rejected.md`); its own
  pre-registered probe landed in the discretionary band (`max(P1..P5) = 421.67`, against a
  426 unconditional-open threshold), requiring an explicit owner decision to proceed rather
  than clearing on the numbers alone; and `003b`'s own **runner-up identity** rests on
  beating `004` by a margin squarely inside the ~±4–5 noise floor (+0.92 validation, +2.85
  observation, §9.1) — a small perturbation in either family's number could have made `004`
  the runner-up instead, which the winner/runner-up margin reported in §9.2 says nothing
  about, since that margin is `008` vs. `003b`, not `003b` vs. `004`.
- **Boundary hits are not interior optima, and neither finalist rests on one.** `003`'s
  `W_BPS` (WHI-1207) and `005`'s `FEE_LO` (WHI-1209) each sit on their own frozen bound
  (`strategies/README.md`; each family's own `NOTES.md` § Search) — but neither `003` nor
  `005` is in the final pair. `003b`'s fitted point is fully interior
  (`S0_BPS=57, W_BPS=2423, DELTA_RESERVE_BPS=763`, none at a range edge); `008` is committed
  at a fixed pre-registered anchor rather than a search result at all, for the reason
  above.

## 10. Out-of-competition ceiling lane

`bench ceiling` (WHI-1247, extended by WHI-1248) grants a candidate curve something no
submittable strategy can actually have — a price re-anchor the arbitrageur cannot
front-run, quoting directly off the replayed GBM fair-price path
(`tools/bench/src/oracle.rs`) — to measure how much edge is reachable by better
re-anchoring before anyone spends real search budget chasing the same gap with a real,
front-runnable mechanism. WHI-1247 measured a trade-triggered cursor (re-anchor moves to
the last *executed* trade's step); WHI-1248 adds a second, exact-step "fingerprint" cursor
that advances only on a step-for-step match against the arbitrageur's own replayed probe
sequence, giving a fixed-lag deployment analogue (`L=1`) and a clairvoyant upper diagnostic
(`L=0`) alongside the first cursor. Full mechanism, adaptations, and measured numbers live
in `ceilings/<id>-<slug>/NOTES.md`, not here.

**Why this is not a strategy.** Nothing under `ceilings/` compiles to BPF, links
`crates/submission-sdk`, or has a `lib.rs` at all (`tools/bench/tests/ceiling_guards.rs`
enforces the no-`lib.rs` invariant mechanically); no ceiling report enters a §6.2 row or a
`compare.rs`-shaped ranking. It exists only to bound headroom for M1/M2 search decisions,
never to be submitted.

**Four mechanical guards**, all in `bench ceiling` itself: (a) `ceilings/**` never contains
a `lib.rs`; (b) every report's own first line is the literal `out_of_competition: true`,
and its tables are shaped differently from `compare.rs`'s ranked
`| regime | n | mean diff | 95% CI |` table, so a ceiling report is never mistaken for a
ranked comparison at a glance; (c) `--reference` is allowlisted to `000-normalizer`
(`--self-check` only) or `001-cpmm-fee` (the 0-line) — it can never silently compare
against a stronger, more recent strategy and be read as beating the real 0-line; (d)
`--segment test` is refused unconditionally, even with
`--i-am-spending-the-test-segment` — nothing out-of-competition here has a ranking claim
for that flag to protect.

**Exemptions, and why each cited section cannot apply here instead of the guards above:**

- **§2.6 (compile paths and the parity gate) does not apply, because there is no BPF path
  to keep in parity.** The lane runs entirely host-side/native (`tools/bench/src/oracle.rs`)
  and never compiles a candidate to BPF, so §2.6's fast-path/reference-path parity gate has
  nothing to check here. The lane substitutes its own parity anchor instead
  (`bench ceiling --self-check`, `tools/bench/src/commands/ceiling.rs::run_self_check`) —
  not a waiver of §2.6, a different gate for a path §2.6 was never written to cover.
- **§2.5 (search protocol) is not exempted — it is reused unmodified.** `bench ceiling
  --fit` shares §2.5's own search machinery (coarse-grid-then-descent, the 300-point cap,
  common random numbers on `screening`) rather than a lighter-weight substitute. What
  distinguishes the lane from a §6.2 strategy is not a relaxed search protocol; it is that
  a ceiling probe's fitted point never enters the ranking §2.5 exists to feed.
- **§2.10 (convergence / the frozen strategy list) does not apply, because the lane is not
  an entry on that list and never becomes one.** It is a deliberate, permanent
  out-of-competition instrument, not an abandoned or forgotten strategy directory — this is
  the same statement `ceilings/README.md` already makes, and it is why the lane is
  explicitly not a §2.10 orphan: §2.10 governs additions to and removals from the frozen
  v1 list, and the lane was never a candidate for that list in the first place.

**Segments the lane may read: `screening`, `train`, `validation` only.** `screening` backs
the search inner loop (shared with §2.5); `train`/`validation` back a fitted point's final
re-evaluation and its paired comparison against the 0-line, mirroring §2.5's own "final
point evaluation" step. `test` is refused unconditionally by guard (d) above, per §2.2's
single-use rule. (WHI-1247's own committed ceiling number used `observation` instead,
before this section existed to declare the policy — that already-committed,
reporting-only measurement is not retroactively invalidated by this list; going forward,
a headline ceiling number is reported against `screening`/`train`/`validation`.)

**Three honesty constraints bound what any ceiling number means** (WHI-1247 § Context,
restated in every committed `ceilings/**/NOTES.md` and `results/*.md` report): (1) it is a
one-sided **lower bound** on what perfect price knowledge is worth, not the maximum of the
perfect-information class, so it does not bound the remaining headroom above a stronger
submission from above; (2) most of the number is `retail volume x captured spread x flow
share(spread)` once the quote stops being front-runnable — the only genuinely non-closed-
form content is the flow-share-vs-spread curve the router grants against the normalizer's
own sampled fee/liquidity; (3) that content generalizes to any oracle-centered quoter and
carries little content specific to the ported curve itself. Every reported ceiling number
must repeat constraint (1) explicitly — a ceiling read as a two-sided bound would overstate
how much headroom a real, front-runnable mechanism could actually reach.
