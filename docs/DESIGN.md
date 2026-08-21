# prop-amm-challenge — Design Document / PRD

> This file is the **spec of record** for this project. Every issue, architectural
> decision, and parameter traces back to a section here. It is produced by grilling the
> idea into shape (`/grill-me`) and formalizing the result (`/to-spec`) — do not skip
> straight to code with this document empty.
>
> **State of this document:** §1–§5, §7 and §8 are written. §4.1/§4.2 describe both
> inherited upstream code and our own layer. **§6 is deliberately incomplete**: the
> milestone structure is fixed, but the *frozen strategy list* M1 iterates over is an
> owner input that has not been supplied yet — see §6.2. M1 cannot be ticketed until it is.

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
  Original designs are v0.2.0 work.

### 1.4 Success criteria

v1 is done when all of the following hold:

1. `tools/bench` reproduces `prop-amm run` **per seed**, not just in aggregate, for the
   starter program (§2.6 parity gate).
2. The **0-line** is established: the CPMM fee family (`strategies/001-*`) is fitted under
   the full protocol, and its fee↔edge response is single-peaked (§2.8).
3. Every strategy on the frozen list (§6.2) has reached a terminal state: a fitted point
   with train/validation numbers, or an explicit `wontfix` with a recorded structural
   reason (§2.9).
4. A winner is named from a **single** use of the test segment, reported with a paired
   confidence interval and its regime slice table, and the answer to "does it beat the best
   fixed-fee CPMM?" is stated explicitly — including if the answer is no.
5. `results/` holds the snapshot backing that claim, carrying the commit sha it was
   produced at.

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
3. the fitted point, committed as the strategy's `lib.rs`,
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
  budget, and `run` refuses it without `--no-report`: a bounded run can never produce
  `results/` evidence, so this equal-budget invariant still holds for every committed
  point.
- **Known and accepted consequence:** an equal point budget favours low-dimensional
  families. A 4-parameter family is covered far less densely by 300 points than a
  1-parameter one. This is not a defect to correct — being hard to tune is a real drawback
  for a curve meant to be deployed — but it must be stated whenever a high-dimensional
  family loses (§8).
- **Final point evaluation:** the search winner is re-evaluated on the full train segment
  (1,000 sims) and on validation (1,000 sims) before entering the ranking.

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
  verification) came in at **7 warm compiles: min=0.320s, mean=0.327s, max=0.343s**,
  meeting the original estimate's order of magnitude once the build's `dlopen`/tempfile
  load step is counted alongside the `cargo build` itself. The 9x gap did not reproduce
  on the same machine; the fast path's design was never at fault — it never rebuilds
  `pinocchio`/`wincode`/`prop-amm-submission-sdk` after the directory's first use, only
  `user_program` itself relinks per point. Either figure is a large improvement over the
  reference path below.
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

### 2.10 Convergence

1. Freeze the strategy list (§6.2). Nothing is added to v1 after this point.
2. Run every listed strategy through M1 to a terminal state.
3. Rank on validation. Open **one** variant for each of the top three (§2.9).
4. Use the test segment **once**: winner and runner-up, paired interval, regime slices.
5. v1 ends. Later ideas belong to v0.2.0, with a fresh test segment.

The freeze point is the mechanism, not the good intentions. Without it the list keeps
growing — every strategy can spawn a variant — validation gets consumed indefinitely, and
sooner or later someone peeks at test. Only a declared "stop adding now" prevents that.

## 3. Cross-cutting Policies

### 3.1 Parameter provenance

Every tunable falls into exactly one bucket:

| Bucket | Where it lives | Who may change it |
| --- | --- | --- |
| Challenge-owned (volatility range, normalizer sampling, step count, edge formula) | `crates/shared/src/config.rs` | upstream sync only. **Never** tuned to improve a local number — the grader runs upstream's values. |
| Ours, protocol-level (seed segments, search budget, grid levels, sim counts) | `config/bench.toml`, typed + fail-fast validated | a PR that also updates §2 |
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
└── bench.toml             ours — seed segments, search budget, grid levels (§3.1)
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
| **M1** | Every listed strategy at a terminal state | each has a fitted point with train/validation/parity numbers, or a recorded `wontfix` |
| **M2** | A named winner | one test-segment use; paired interval; regime slices; `results/` snapshot; §1.4 satisfied |

M0 is split into three issues so the measurement layer is validated *before* anything is
built on top of it (`WHI-1193` → `WHI-1194` → `WHI-1195`). The ordering is the point:
`WHI-1193` ends by cross-checking bench against the upstream CLI, and starter's 210.50 is
the only anchor produced by an upstream code path — the one chance to catch a bench that is
wrong in the same direction as its own tests.

### 6.2 The frozen strategy list — **OWNER INPUT REQUIRED**

M1 iterates over a list that does not exist in this repo yet. `docs/references/` is empty;
the collected material (source, Solidity, prose) is outside the repository.

**M1 cannot be ticketed until this list is supplied.** Each entry needs:

| Field | Why |
| --- | --- |
| Name | the `NAME` constant and the `strategies/NNN-<slug>` directory |
| Source form | source / Solidity / prose / leaderboard-name-and-score only — sets the porting effort and the fidelity bar (§2.9) |
| Original material location | filed under `docs/references/` so provenance survives |
| Known parameters | seeds the frozen space of §2.4 |

Until then §2.10's freeze step has nothing to freeze, and M1 has no issues. M0 is fully
specified and unblocked.

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
   absorb unbounded effort. *Mitigation:* the `wontfix` terminal state (§2.9).

**Open questions**

1. **The frozen strategy list** (§6.2) — blocks M1. Owner input.
2. **Whether the leaderboard is still accepting submissions.** Does not affect v1 (§1.3),
   but decides whether §1.4's winner ever gets submitted from a tag.
3. **Does the grader use seeds `0..=999`?** Assumed from `BASELINE_SIMS`/`BASELINE_STEPS`
   defaults, not confirmed. Only affects how the observation row is interpreted.
4. **Are `retail_arrival_rate` and `retail_mean_size` worth adding to the grid?** Held at
   defaults for readability (§2.3); a flow-sensitive strategy might need them.
5. **When does the duplication in §4.2 justify generation?** Trigger set at ≥5 strategies
   sharing non-trivial numeric helpers; the count is a guess.
6. **Is L2 needed?** Deferred (§2.7). The trigger is a strategy whose loss cannot be
   diagnosed from flow share and edge per unit volume alone.
