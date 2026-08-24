# C-orbic-oracle

**Out-of-competition (`ceilings/README.md`, WHI-1247).** Nothing in this file describes a
submittable strategy — there is no `lib.rs` here, and no number below is a ranked M1/M2
result. This is a *ceiling* probe: how much edge is on the table when a curve is granted a
price re-anchor the arbitrageur cannot front-run, measured against `001-cpmm-fee` (the
0-line), before anyone spends real search budget chasing the same gap with a submittable,
front-runnable mechanism.

**Three honesty constraints bound what the number below means** (WHI-1247 § Context,
restated in full in the committed `results/*.md` reports): (1) it is a one-sided **lower
bound** on what perfect price knowledge is worth, not the maximum of the perfect-
information class, so it does not bound the remaining headroom above a stronger submission
from above; (2) most of the number is `retail volume x captured spread x flow
share(spread)` once the quote stops being front-runnable — the only genuinely
non-closed-form content is the flow-share-vs-spread curve the router grants against the
normalizer's own sampled fee/liquidity; (3) that content generalizes to any oracle-centered
quoter and carries little content specific to the Orbic curve itself.

## Provenance

**One edit beyond the issue's own enumerated "only edits to existing files" list, surfaced
here per that same section's instruction** (round-2 review of this issue): the issue body's
"What must not change" says the only edits to existing files are the two additive lines in
`commands/mod.rs` and `cli.rs`, and separately lists a fixed set of files as **zero-edit**.
`tools/bench/src/main.rs` is on neither list, yet it gained a third line, `mod oracle;` —
mechanically required, the same pattern every existing `mod <name>;` line in that file
already follows, so `oracle.rs` (step 1's new price-path/curve module) could link into the
binary at all. Not a judgement call, not deferred, and not something a fix would remove —
just not literally on the "only" list, so it is disclosed rather than left for a future
reader to notice as an unaccounted-for diff.

Source form: **ported (Solidity → Rust), from the same pinned material already on file for
the canceled `002` porting issue** (`docs/DESIGN.md` §6.2: "`002` | Orbic — **Canceled
(WHI-1206)**"; `docs/references/002-orbic-flashbots/README.md`) —
Flashbots' `ExamplePropAmm.sol`, pinned at commit `da53117870c7bec96d71caebe1b3f94370aba3d6`,
MIT-licensed, itself stating it was "Adapted from https://github.com/fahimahmedx/prop-amm".
No new fetch was needed: `002`'s own freeze-time snapshot already covers the mechanism this
lane ports.

`002` was **Canceled**, not ported as a submission, because of "quantization jitter" —
probabilistic shape-check panics across seeds rather than across parameter values
(`docs/DESIGN.md` §2.4, the paragraph naming WHI-1206 as the retained example of that
general phenomenon). This lane revives the mechanism for a different purpose — measuring a
ceiling, not shipping a submission — and the same jitter shows up here too: the anchored
fit's own search (below) evaluated **184 of its 300-point budget** (`search.rs`'s
`points_evaluated` counts every point actually compiled and simulated, valid and invalid
alike, and stops there because `screening`'s own coarse-grid-then-descent search converged
before spending the rest of the budget) — of those 184, **110 panicked (caught as invalid)
and 74 were valid**. That is treated as expected and load-bearing evidence *for* why
`002` was right to cancel as a submission candidate, not as a defect in this lane — every
invalid point is caught (`std::panic::catch_unwind`, mirroring `crates/sim`'s own
`curve_checks.rs` shape-check panics per `docs/DESIGN.md` §2.4) and excluded from the peak
search rather than either scored `-inf` or silently skipped.

The same jitter also surfaced one level up, past the search itself: the *final*
re-evaluation of a fitted point (on `observation`, not `screening`) is a second, separate
call into the curve, and `ceiling.rs`'s first implementation did not guard that call the
way `run_fit`'s search loop already guarded its own evaluations. Running the real
`--variant floating --fit --concentration 2.33 --segment observation` measurement hit
exactly that gap — a panic during `observation`'s re-evaluation, after a clean pass on all
of `screening`'s points — and crashed the whole process before a report could be written.
(The captured panic payload was non-string, which defeats this lane's own
`panic_message` downcast — see the `floating` section below — so at the time this gap was
found, the specific cause was not pinned down beyond "a panic occurred," only that it
originated somewhere in the simulated batch. Round-2 review's location-capturing panic
hook (`ceiling.rs::catch_panicking`, below) has since confirmed it: the re-measured
`floating` run's panic message now cites `crates/sim/src/curve_checks.rs:23:9` — the exact
monotonicity/concavity check `docs/DESIGN.md` §2.9 names as this simulator's shape-check
panic site.) Commit `49509a3` closes that gap with a second, staleness-
preserving `catch_unwind` around the final measurement, mirroring `commands/fit.rs`'s own
established pattern for its analogous train/validation re-evaluation step (docs/DESIGN.md
§2.4/§2.5, WHI-1213): on a caught panic, the evidence gathered so far is still written —
marked `INVALID` rather than omitted — and the command exits with an informative error
instead of an unhandled panic. The `floating` section below is exactly that INVALID case,
not a gap in this write-up.

## Mechanism

Ported from `docs/references/002-orbic-flashbots/README.md`'s own mechanism summary:

- `v0 = target_x * concentration`
- `k = v0^2 * price` (the source's `K = v0^2 * multX / multY`; here `price` stands in for
  `multX / multY`'s ratio, since this simulator has no separate `multX`/`multY` — see
  "Adaptations" below)
- `base = v0 + reserve_x - target_x`
- swap math otherwise unchanged from the source's `X→Y` / `Y→X` formulas

**What is deliberately not ported:** the source's `_isTargetYLocked` 5%-move emergency
circuit breaker. It freezes trading when a derived `targetY` value drifts more than 5% below
its own running maximum — a Flashbots-specific defense against a *stale oracle read*
(`docs/references/002-orbic-flashbots/README.md` calls this "not a pricing input" and
explicitly leaves porting it to the porting issue's judgment). This lane's oracle is never
stale in that sense — it replays the simulator's own GBM fair-price path directly
(`tools/bench/src/oracle.rs`), so there is nothing analogous to lock against. Recorded here,
not silently dropped, per that same README's fidelity note.

**What is new, not in the source at all:** the spread axis (`spread_bps` / `SPREAD_BPS`,
`tools/bench/src/oracle.rs`'s own doc comment calls this out as "this issue's own new axis").
The source's oracle-published `multX`/`multY` pair has no simulator analogue to read from
(`docs/references/002-orbic-flashbots/README.md`'s fidelity note: "the market-maker-gated
liquidity model doesn't map cleanly onto this challenge's ... interface" — exactly the gap a
porting issue would have had to resolve). This lane's answer, specific to being a ceiling
probe rather than a submission: quote a symmetric spread around the oracle price instead —
buys priced at `p_oracle * (1 + spread)`, sells at `p_oracle * (1 - spread)` — giving the
curve a second free axis to trade off against `concentration` without inventing a fictitious
`multX`/`multY` sampling process this simulator doesn't have.

**Two `target_x` variants** (`tools/bench/src/oracle.rs`'s `OracleVariant`):

Both variants quote off the same `p_oracle` price re-anchor (`tools/bench/src/oracle.rs`) —
that primitive is never withdrawn from either one. What differs between them is only
whether the curve's *inventory* target (`target_x`) is itself anchored:

- **`anchored`** (headline): `target_x` is pinned to the pool's own initial `reserve_x`
  (`ANCHOR_X`, captured once per simulation), on top of the shared price re-anchor — this
  is the variant with pure oracle quoting *and* fixed inventory management.
- **`floating`** (degenerate diagnostic): `target_x = reserve_x` on every swap — the price
  re-anchor is still present, but there is no fixed inventory target: `base` collapses to
  `v0` identically (`tools/bench/src/oracle.rs`'s own doc comment), and the curve's own
  trading activity feeds back into its shape parameter on every swap. Included to isolate
  how much of `anchored`'s edge comes from the concentration/spread shape and the price
  re-anchor alone versus from the *additional* fixed inventory target.

## Frozen parameter space (declared before any search runs — docs/DESIGN.md §2.4)

**concentration ∈ [1.00, 100.00]** (`concentration_x100 ∈ [100, 10_000]`, `i128`, 2 implied
decimal places) and **spread_bps ∈ [0, 1_000]** (0%–10%).

Rationale:

- **concentration lower bound 1.00** — the source contract's own `InvalidConcentration`
  floor is `concentration >= 1`
  (`docs/references/002-orbic-flashbots/README.md` § Known parameters).
- **concentration upper bound 100.00**, well inside the source's own `< 2000` ceiling — this
  lane is a ceiling probe on a fixed 300-point budget shared with the spread axis, not an
  exhaustive sweep of the source contract's entire legal range; a coarse-grid-then-descent
  search (`tools/bench/src/search.rs`, same mechanism `001`/`003`/etc. use) needs the
  interior of *this* range to be worth searching, and 100x concentration is already an
  extremely tight virtual-reserves band relative to `target_x`.
- **A declared fidelity adaptation: 2 implied decimal places, not the source's plain
  integer.** `docs/references/002-orbic-flashbots/README.md` § Known parameters records
  the source contract's own constraint as `concentration` **integer**,
  `1 <= concentration < 2000` — its `InvalidConcentration` check runs as on-chain integer
  arithmetic, because the source is a BPF program. This lane's `concentration` is not: it
  runs entirely host-side over `f64` (`tools/bench/src/oracle.rs`, "Execution path: native
  (host-side, never BPF-compiled)" in every committed report below), so the on-chain
  integer constraint has no BPF-arithmetic reason to bind it here. The search grid is
  therefore encoded as `concentration_x100` — an integer grid over `[100, 10_000]`,
  divided by 100 (`tools/bench/src/commands/ceiling.rs::concentration_spec`) — giving the
  fit 2 decimal places of resolution (the fitted point below, `2.33`, is not a whole
  number) instead of only the ~1999 whole-number rungs the source contract would accept.
  Declaring this explicitly, the same way step 2 requires `SPREAD_BPS` to be declared: this
  is an intentional adaptation to a host-side diagnostic that never compiles to BPF and is
  never submittable, not an unnoticed fidelity slip.
- **spread_bps upper bound 1_000 (10%)** — an order of magnitude above the reference's own
  fee (`001-cpmm-fee`'s frozen range tops out at 500bps / 5%, `strategies/001-cpmm-fee/NOTES.md`),
  giving the search headroom to find an interior optimum rather than hit a boundary, while
  still being small enough that the oracle-anchored quote stays economically a "spread," not
  an arbitrary markup.
- **spread_bps lower bound 0** — a spread of exactly zero is a legitimate point (pure
  concentration effect, no spread contribution) and must be reachable, not excluded.

## Parity anchor (`bench ceiling --self-check`)

Round-2 review of this issue caught that this mode existed and was correct in shape
(`tools/bench/src/commands/ceiling.rs::run_self_check`) but had no recorded result — the
lane's own substitute for docs/DESIGN.md §2.6's `prop-amm validate`/`run` parity gate (step
6 above) had never actually been run and written down, so it was documentation of a design,
not evidence.

Run (any segment; `observation` used here to match the measurements below):

```
cargo run -p prop-amm-bench -- ceiling --self-check --segment observation
```

Result: `self-check PASSED: 1000 seeds agree within 0.000001 between the trusted
compile+run path and this lane's own native batch loop` — `strategies/000-normalizer`
compiled and run through `compile::build_and_load` (the trusted path every other bench
subcommand uses) agrees with the same normalizer run through this lane's own
`oracle::run_batch_native_loop` on all 1000 `observation` seeds, within the `1e-6`
tolerance `run_self_check` enforces. This confirms the custom per-seed loop this lane had
to write (§ What must not change forbids reusing `runner::run_batch_native`'s own loop
plumbing for a host-side fn) does not itself move the number — a mismatch here would mean
the loop, not the oracle curve, produced any observed edge difference.

## Fitted point

Reproducibility per `docs/DESIGN.md` §3.3 (segment, sim/step counts, execution path, commit
sha) — both runs below share:

- Commit: `831284c` (the committed `results/*.md` reports carry this as their own
  `Commit:` line; the underlying measured numbers are unchanged from the original
  `49509a3` run and from the `e319f56`-stamped round-1/round-2 regenerations — this
  final regeneration re-ran both `--fit` invocations in release mode from a detached
  scratch worktree at `831284c` and reproduced the anchored point's edge/staleness
  figures byte-identical to the earlier `e319f56` report; only the report/NOTES.md
  provenance line moved, not the simulation logic or any measured field)
- Segment: `observation` (1000 sims, 10,000 steps each, reporting-only —
  `docs/DESIGN.md` §2.2 — never a decision input)
- Execution path: native (host-side, never BPF-compiled — this lane is not submittable)
- The fit search itself runs on the fixed `screening` segment (200 sims,
  `config/bench.toml`'s `[search] max_points = 300`), per `tools/bench/src/commands/ceiling.rs`'s
  `run_fit`, independent of which segment the final measurement reports against.
- **Reproducibility caveat: both `--fit` runs below were executed from a detached
  scratch worktree (`git worktree add --detach /tmp/<name> <sha>`), not from
  `.claude/worktrees/whi-1247` itself.** `ceiling --fit`'s reference-path compile goes
  through `tools/bench/src/compile.rs::build_and_load`, which shells out to `cargo run
  -p prop-amm -- build` — that in turn drives `crates/cli/src/commands/compile.rs`'s
  `ensure_build_dir` (upstream-owned, out of scope for this issue), which creates an
  isolated build package with no `[workspace]` table of its own. `tools/bench/src/
  compile.rs`'s own test doc comment (`starter_over_observation_segment_is_bit_
  identical_with_and_without_telemetry`, added under WHI-1205) already documents the
  consequence: when the repo root a test or command runs from is itself nested inside
  another git worktree of the same repo — exactly `.claude/worktrees/<name>`, this
  repo's own mandated layout for issue work — cargo's ancestor search resolves the
  *primary clone's* workspace instead of the isolated package's own manifest, and
  fails with `current package believes it's in a workspace when it's not`. This is not
  new to this issue: `tools/bench/src/fast_compile.rs`'s own fast build path hit the
  identical error (WHI-1205) and was fixed there with an empty `[workspace]` table in
  its generated `Cargo.toml`; that fix was never extended to the reference path
  `ensure_build_dir` uses, because that path lives in `crates/cli`, upstream-owned code
  this repo edits only through an upstream sync (`AGENTS.md`). Anyone re-deriving the
  numbers below **from inside** `.claude/worktrees/whi-1247` will hit that exact build
  error on the `--fit` step; re-derive from a detached scratch worktree instead (as
  done here), or wait for an upstream-sync-lane fix. Tracked as a deferred issue below
  rather than fixed in this PR, since the fix does not belong to this issue's scope.

### `anchored` (headline) — joint fit of `(concentration, spread_bps)`

Run via `cargo run -p prop-amm-bench -- ceiling --variant anchored --fit --segment observation`.
Committed report: `results/2026-08-23-ceiling-anchored-trade-triggered-observation-orbic-oracle-vs-001-cpmm-fee.md`.

- **concentration = 2.33, spread_bps = 103.0000 (1.03%)**
- Search budget spent: 184 of 300 evaluated (74 valid, 110 invalid/panicking, as discussed
  in Provenance above), not exhausted
- Best avg edge on `screening`: 473.787933
- **Mean edge diff (oracle − `001-cpmm-fee`) on `observation`: 87.234885**, 95% CI
  `[81.016163, 93.453606]`, n=1000, std error 3.172875 — this is *a* ceiling number, not
  *the* ceiling on this mechanism family (see the three honesty constraints above): the
  ceiling lane's oracle-anchored curve beats the 0-line by ~87 edge/sim on average when
  granted a re-anchor no submittable strategy can actually have, and that is a one-sided
  lower bound on what perfect price knowledge is worth, not its maximum.
- Trade-triggered cursor staleness (steps since the oracle cursor's last executed trade):
  mean of per-sim means 3.886, median of per-sim means 3.144, **p95 of per-sim means
  8.437** (round-2 review's fix for a genuine naming bug — the pre-existing "p95 of p95"
  figure was actually computed via `f64::max`, not a percentile; renamed to `max of
  per-sim p95` below and a real `p95_of_means` percentile added alongside it,
  `tools/bench/src/commands/ceiling.rs::aggregate_staleness`), max of per-sim p95 109.000,
  max of per-sim max 205.000 — the re-anchor is current almost all the time (single-digit
  mean staleness, and even the 95th percentile of per-sim means is under 9 steps) but has
  a long tail of quiet stretches up to ~200 steps in the worst simulated path.

### `floating` (degenerate diagnostic) — spread-only re-fit, concentration held at 2.33

Run via `cargo run -p prop-amm-bench -- ceiling --variant floating --fit --concentration 2.33 --segment observation`.
Committed report: `results/2026-08-23-ceiling-floating-trade-triggered-observation-orbic-oracle-vs-001-cpmm-fee.md`.

- **spread_bps = 77.0000 (0.77%)** — the best point the search found on `screening` before
  the point below turned out to be **INVALID**
- Search budget spent: 165 of 300 evaluated (156 valid, 9 invalid/panicking on
  `screening`), not exhausted
- Best avg edge on `screening`: 639.575681
- **Final re-evaluation on `observation`: INVALID.** Re-running this exact point
  (`concentration = 2.33, spread_bps = 77.0`) against `observation`'s 1000 seeds — a
  different, larger seed set than the 200 `screening` seeds the search used — triggered a
  panic, caught rather than crashed (see Provenance above). The captured panic payload
  itself is still non-string (`panicked with a non-string payload`), which defeats this
  lane's own `panic_message` downcast (it only recognizes `&str`/`String` payloads) — but
  round-2 review's fix to `catch_panicking` (`ceiling.rs`, below) now also captures the
  panic's source location, and this re-measurement's captured message is
  `panicked with a non-string payload (at crates/sim/src/curve_checks.rs:23:9)`. **That
  confirms the cause**: `crates/sim/src/curve_checks.rs:23` is exactly the
  monotonicity/concavity panic `docs/DESIGN.md` §2.9 documents as this simulator's own
  shape-check site — the same site every M1 strategy's `bench fuzz` gate exists to probe
  for before a search is trusted. No mean edge diff, no CI, and no staleness numbers exist
  for this point; the search's own clean pass on `screening` was not evidence this point
  was safe on a different seed set. This is the documented failure mode itself
  (docs/DESIGN.md §2.4/§2.5/WHI-1213) landing on the documented panic site, not a bug in
  the point chosen or in this lane's own harness.
- **A separately captured run names the specific violation, with an explicit provenance
  caveat.** An earlier run of this exact command (`ceiling --variant floating --fit
  --concentration 2.33 --segment observation`, release profile) at commit `1992c09`
  (this lane's first commit, before round-1/2/3 review added `ceiling.rs`'s own
  catch-and-report path in `49509a3`) hit the same panic site with no catching harness in
  front of it, so Rust's default panic hook printed the payload verbatim instead of this
  lane's `panic_message` downcast swallowing it:

  ```
  thread '<unnamed>' panicked at crates/sim/src/curve_checks.rs:23:9:
  submission shape violation during arbitrage sell search: monotonicity violated:
  input 0.686681 -> output 51.183366, input 0.740636 -> output 0.000000
  ```

  This is cited here **as evidence from a different, earlier commit, not as this run's
  own re-verification** — the fitted point recorded above (`831284c`) never itself printed
  this string; its own downcast failed and produced the non-string placeholder instead.
  `git diff 1992c09 831284c -- crates/sim/src/curve_checks.rs` is empty, and
  `tools/bench/src/oracle.rs`'s `oracle_swap` diff between the two commits (round-3
  review, standards finding #4) only reorders the `side`-match arms to drop an
  unreachable second match — same `price`/`k`/`out` formulas, not a behavior change per
  its own commit message — so this violation is good-faith evidence of the same
  mechanism (a larger input returning `0` because the quote exceeded the reserve,
  breaking monotonicity — exactly what the `floating` variant's `base == v0` degeneracy
  predicts), not proof that it is the bit-identical violation the `831284c` run's search
  or final re-evaluation actually hit. No re-run was performed to close that gap; it is
  tracked in `docs/DEFERRED_ISSUES.md` instead.

**Step 3(b)'s prediction was never tested, and that tension is worth stating
explicitly.** WHI-1247 step 3(b) predicted "expect `concentration` to run to its upper
bound and the axis to be degenerate" for this variant. Step 10 instead freezes
`concentration` at `anchored`'s fitted 2.33 and re-fits `spread_bps` only ("re-fit
**spread only**"), so `concentration` is never varied under this budget and step
3(b)'s prediction about that axis is untestable here, not confirmed or refuted. This
tension was not previously noted in this document or in the committed reports; see
`docs/DEFERRED_ISSUES.md` for the tracked entry.

### Anchored vs. floating — what the contrast shows

The two variants land in qualitatively different places, and that gap is itself the
finding this diagnostic exists to produce (WHI-1247 step 3): `anchored`'s `target_x`,
pinned once per simulation to the pool's own starting reserves, keeps the curve's shape
stable enough to search *and* to re-evaluate cleanly on a seed set five times the size of
the one the search used — 110 of the 184 candidate points the search actually evaluated
were invalid, but the chosen point still held up on `observation`. `floating`'s
`target_x = reserve_x` on every swap lets the curve's own trading activity feed back into
its own shape parameter on every single swap, not just at simulation start; that
closer-to-the-edge dynamic degenerates harder — fewer invalid points appeared during the
`screening`-segment search (9 of 165 evaluated) than for `anchored`, yet the one point the
search settled on still failed to survive re-evaluation on a five-times-larger, different
seed set. **The comparison is not about which variant scores higher** — the only figures
the two variants share a unit with are the `screening`-segment best-avg-edge numbers
(`floating` 639.575681 vs. `anchored` 473.787933, so if anything `floating` screened
*higher*, not lower), and that is not the same quantity as `anchored`'s headline
paired-vs-0-line figure (87.234885 edge/sim on `observation`) — the two are not
comparable side by side. What the contrast actually shows is a *robustness* gap, not an
edge-magnitude one: `anchored` produced a number that survived re-evaluation on a
seed set five times larger than the one the search used, and `floating` did not,
despite screening cleaner (fewer invalid points during search) and scoring higher on
that same screening segment. A screening-segment score is not evidence of that kind of
robustness either way. That is exactly why `floating` was scoped from the start (see
"Two `target_x` variants" above) as
"never a result on its own," and it is exactly the Orbic family's own quantization jitter
(`docs/DESIGN.md` §6.2, `002`, WHI-1206, Canceled) showing up a second time, one level
removed from the search itself. The ceiling number this lane exists to produce is
`anchored`'s 87.234885 edge/sim above; `floating` contributes no comparable number, only
the confirmation that the *fixed inventory target* specifically — not the shared price
re-anchor both variants have, and not just the concentration/spread shape — is load-bearing
for even having a well-behaved curve to measure.

**Considered and rejected: recovering a number for `floating` anyway.** Round-2 review of
this issue asked why nothing was tried to recover `floating`'s two required numbers — e.g.
excluding the panicking point from the search's own acceptance and re-fitting, or reporting
the runner-up point the search passed over. Both were considered and rejected, not
overlooked: either one would substitute a point the search did **not** actually select for
the one it did, which manufactures a number for a variant this issue's own step 3 already
scoped as "never a result on its own" — the empty result *is* the diagnostic. A runner-up
or a re-fit-around-the-failure could easily land on another point that *also* fails on a
different, larger seed set (this is exactly the jitter `docs/DESIGN.md` §6.2's `002` entry
describes as probabilistic across seeds, not deterministic per parameter value), silently
trading one uncaught failure mode for a laundered one that merely didn't trip on this
particular re-evaluation. Reporting that as if it were `floating`'s real number would be
worse than reporting nothing.

## WHI-1248 — exact-step fingerprint cursor: `L=1`/`L=0` closed as a negative result

**What was built and works as designed.** `tools/bench/src/oracle.rs` gained a second
cursor mode (`CursorMode::Fingerprint`, alongside WHI-1247's `TradeTriggered`): per seed,
`build_fingerprint_targets` replays `Pcg64::seed_from_u64(cfg.seed.wrapping_add(2))` through
the identical `LogNormal` construction `Arbitrageur::new` uses (`arbitrageur.rs:53-58`),
applies the same floor clamps, and nano-quantizes the result into a per-step
`(buy_probe, sell_probe)` target pair. `maybe_advance_fingerprint_cursor` advances a
monotone cursor by exactly one whenever a live `oracle_swap` call's probe matches the
*next* step's target, gated by a "seen a `side == 1` call since the last advance" predicate
(hardening check (c)). Two more hardening checks run at measurement time
(`commands/ceiling.rs::run_fingerprint_final_eval`): a per-trade assertion that the cursor
already equals the executed trade's own step by the time its `after_swap` fires (check
(a)), and a terminal assertion that the cursor reached the simulation's final step (check
(b)). All three are unit-tested directly (`oracle::tests::per_trade_assertion_fires_on_a_
corrupted_cursor`, `oracle::tests::terminal_assertion_fires_when_cursor_never_reaches_the_
final_step`, both `#[should_panic]` tests that pass), and a curve_checks-shape-violation
panic caused by an early false advance is classified distinctly from a genuine
`CursorAssertion` trip or an unrelated `"Other"` panic
(`commands/ceiling.rs::classify_fingerprint_panic`, unit-tested). The panic-payload downcast
fix (`ca8b865`, `commands/ceiling.rs::panic_message`) peels rayon's cross-thread
`Box<dyn Any + Send>` re-wrap before attempting the `&str`/`String` downcast, and is
directly demonstrated below to produce a fully readable message on a real, reproduced
panic — not the earlier session's opaque `"panicked with a non-string payload"` placeholder.

**What does not work: the exact-match reconstruction itself, once floor-clamping is in
play.** A live `--fit` run measured a per-seed hardening-check trip rate of **0.564**
(564 of the 1000 seeds on `validation` tripped) — a per-**seed** rate, not directly
comparable to this design's own per-**comparison** expectation (~1e-10, treating a match
as a random continuous-value collision) without converting it to matched units first. An earlier draft of this note
(WHI-1248) compared the two figures directly and reported "roughly nine orders of
magnitude" — wrong, because it never converted the per-seed rate down to a
per-comparison one before comparing (WHI-1249 fixes this). The conversion: the
reproduction below hits `oracle.rs`-call-index 121973 by step 3201, ≈38 comparisons/step;
scaled to a full `n_steps=10_000` run that's ≈3.8e5 comparisons/seed, rounded here to
**4e5** for this order-of-magnitude conversion. Dividing, `0.564 / 4e5 ≈ 1.4e-6` — at
matched units, the implied per-comparison rate is **~1.4e-6**, roughly **four** orders of magnitude above the
~1e-10 design expectation (1.4e-6 / 1e-10 ≈ 1.4e4). That gap was traced to a specific
mechanism, reproduced deterministically on seed `2_000_011` (`validation` segment,
`concentration=94.33, spread_bps=102.0`, variant (a), full `n_steps=10_000`):

1. The cursor advances correctly for 3201 consecutive steps.
2. At `oracle.rs`-call-index 121973 — 26 calls into step 3201's *own* sell-side search — a
   probe of exactly `f64_to_nano(0.001) = 1_000_000` matches step **3202**'s own
   `sell_target`, which had independently floor-clamped to the identical value (its own
   `start_y ~= 0.1118`, `fair_price[3202] = 116.58`, `start_x = start_y / fair_price ~=
   0.00096 < FP_MIN_INPUT`). The cursor advances one step early.
3. The very next `after_swap` (step 3201's genuine trade) fires the per-trade assertion
   verbatim: `WHI-1248 fingerprint cursor hardening check (a): the fingerprint cursor
   (3202) must already equal the executed trade's own step (3201) ...` — the readable
   message the panic-payload fix produces, no downcast failure.
4. That `input=1_000_000` probe is not a coincidence between two independent draws — it is
   `arbitrageur.rs::golden_section_max`'s own first internal evaluation, `objective(left)`
   where `left == lo`, and `bracket_maximum`'s common early-return paths (`if mid_value <=
   0.0 { return (lo, mid); }`; `if hi_value <= mid_value || hi >= max_input { return (lo,
   hi); }` — both return before the `for` loop's own first `lo = mid;` reassignment) hand
   back `lo` unchanged at the original floor constant. For the sell side that floor is
   `min_sell_input_x(fair_price) == FP_MIN_INPUT == 0.001` whenever `fair_price >
   MIN_ARB_NOTIONAL_Y / MIN_INPUT == 10` (true for essentially this entire run); for the
   buy side it is the unconditional constant `min_buy_input_y() == FP_MIN_ARB_NOTIONAL_Y ==
   0.01`, always. **Every step's own search therefore routinely re-probes a fixed floor
   value, independent of that step's own draw** — a direct count over this seed's
   `FP_TARGETS` found exactly 2 of the 10,000 steps' `sell_target`s independently floor to
   that same constant, each one a near-guaranteed false-match trap the moment the cursor's
   `next` pointer reaches it.
5. `SEEN_SIDE1_SINCE_ADVANCE` (hardening check (c)) does not block this: its own doc
   comment already named this exact residual case ("within a step's own sell-side search
   evaluating a value that happens to equal the next step's own sell probe") as
   unprotected — measurement now shows it is the *dominant* failure mode, not a residual
   one.

Full trace and code-level detail: `tools/bench/src/oracle.rs::build_fingerprint_targets`'s
doc comment, limitation 3.

**Why this rules out reporting a number, not just a threshold tweak.** A tripped seed is a
deterministic function of that seed's own RNG stream — re-running the identical seed
reproduces the identical trip every time, so "re-run" cannot mean "check for a fluke" here.
Worse, floor-clamp exposure is correlated with the RNG's own low-draw episodes, not
independent of the quantity being measured — so averaging a paired statistic over only the
seeds that happened not to trip is a **selection-biased** estimate, not a smaller-`n`
version of the same estimate, regardless of how carefully the surviving sets are index-
paired between the candidate and the `001-cpmm-fee` reference (`run_fingerprint_final_eval`'s
own `reorder_oks_to_configs_order`/`filter_batch_to_seeds` machinery does that part
correctly — pairing integrity was checked and is not the problem). This is not a
miscalibrated hardening-check threshold: the checks are working exactly as intended,
correctly catching a real, structural defect in the underlying reconstruction method.

**WHI-1249 correction: this is not "a materially different and more expensive
reconstruction strategy" — a cheap, bounded fix exists, tracked as WHI-1250.** On the
**sell** side the false-matching probe and a step's own genuine probe are, at the
interface this replay observes, the same value arriving through the same call, with no
additional signal available at match-time to tell them apart. Two different rates are at
play on the sell side and must not be conflated: the floor's *formula* is
`min_sell_input_x(fair_price) == FP_MIN_INPUT == 0.001` whenever `fair_price >
MIN_ARB_NOTIONAL_Y / MIN_INPUT == 10`, which is true for essentially this entire run — but
that only fixes *which constant a clamp would land on*, not *how often the draw actually
gets clamped to it*. The floor only actually *binds* (the drawn `start_x` falls at or below
that constant) at roughly P ≈ 1e-4/step, consistent with the direct per-seed count above of
2 floor-clamped `sell_target`s out of 10,000 steps. The **buy**
side is different in kind, not just degree: its floor (`start_y = draw.max(0.01)`, i.e.
`min_buy_input_y() == FP_MIN_ARB_NOTIONAL_Y == 0.01`) clamps far more rarely (P ≈
1e-8/step), and the very first `compute_swap` call of *every* step is always the buy-side
probe (`arbitrageur.rs::plan_arb_buy_x` runs before the sell-side search ever starts). A
cursor-advance rule restricted to buy-side matches only would therefore remove the
dominant ambiguity class by construction — at the cost of losing the buy side's own
(much rarer) corroboration signal, which is a real trade-off but a cheap one to implement
and measure, not a different reconstruction strategy. That change (and the re-measurement
it would enable) is out of scope for this issue and is tracked as the follow-up
WHI-1250.

**Decision: `L=1` (variant (a), the headline rung) and `L=0` (variant (a), the diagnostic
rung) are closed as a documented negative/method-level result — no mean-edge-diff number,
no CI, and no per-sigma slice is reported for either.** This mirrors the precedent already
in this file for `floating`'s own diagnostic ("never a result on its own") and this repo's
own prior-established precedent (005b's ablation closing with no committable point) — an
honest negative result, not a gap to paper over. A previously generated report from this
investigation
(`results/2026-08-23-ceiling-anchored-fingerprint-l1-validation-orbic-oracle-vs-001-cpmm-fee.md`,
written to a detached scratch worktree, never committed to this repo — see Out of scope in
WHI-1249, which deliberately does not commit it either) reported `mean edge diff =
225.124282, 95% CI [204.396653, 245.851912], n=436, 564 tripped seeds`. Its own headline
was a **survivors-only mean over n=436 of the segment's 1000 seeds** — a fact the rejected
report itself disclosed only in a table cell (`n | 436`), never stating in its own prose
that 564 seeds (56.4%) were excluded from the number it reported as the fitted point's
edge.

**WHI-1249: why 436-of-1000 is not "a smaller-n version of the same estimate," in the
rejected report's own numbers.** Its per-sigma-tier table (§ above's slicing machinery,
applied to the biased survivor set) was:

| sigma tier | survivors / ~333 per tier | mean edge diff | 95% CI |
|---|---|---|---|
| Low | 212 | +278.630199 | [259.750725, 297.509673] |
| Mid | 197 | +244.485003 | [219.961700, 269.008307] |
| High | 27 | **−336.257804** | [−441.136730, −231.378878] |

The survivor set deletes 92% of the High-sigma tier (27 of ~333 survive) — precisely the
one regime tier where this fitted point *loses* to the 0-line — while Low and Mid survive
at their full rate. A crude equal-weight post-stratification (weighting each tier's own
mean by 1/3, undoing the survivor-count imbalance rather than the report's own n-weighted
pooling) gives `(278.630199 + 244.485003 − 336.257804) / 3 ≈ +62.29` — **below** the kept
trade-triggered result's +87.234885, not above it. (Note the segments differ: the rejected
report and its ≈+62.29 debias are on `validation`, per the report filename above, while
+87.234885 is `observation`'s own headline, line 239 above — this comparison is cross-
segment, which is why it can only support the qualitative "not a smaller-n version of the
same estimate" conclusion below, not a same-segment apples-to-apples delta.) The
225.124282 headline is therefore not merely noisier for having a smaller n; it is a
different (and larger) number than any credible correction of the same estimate, in the
direction that would have overstated this lane's ceiling had it been reported as-is.

**This ≈+62.29 figure is illustrative arithmetic about the rejected report's own bias, not
a prediction of any future measurement.** It is a crude equal-weight re-pooling of three
tier means that were themselves computed on a biased survivor subset, evaluated at that
subset's own contaminated fitted point (`concentration = 94.33`) — a point WHI-1249 names
above as a likely product of the same search-time contamination, not a point any
non-contaminated search would have chosen. WHI-1250's fresh, non-contaminated re-fit lands
at a different point entirely (`concentration = 62.4200`, confirmed distinct from `94.33`
below), so there is no basis for reading ≈+62.29 as an estimate of what that re-fit would
find, and it is not treated as one anywhere in this file. Its only role is the qualitative
one stated above: showing that the rejected 225.12 headline itself cannot be trusted, not
forecasting a replacement number.

**The rejected report's own "Converge/fan-out verdict: HELD" must not be cited.** That
verdict — Low tier's CI width (37.758948) narrower than High's (209.757852), read as
consistent with "converge at low sigma / fan out at high sigma" — was computed entirely on
this same biased survivor set. A verdict computed on a selection-biased sample is not
evidence for or against the underlying converge/fan-out prediction either way; it is simply
inadmissible, for the same reason the headline mean is.

**The pairing arithmetic itself was correct — the flaw is the non-random subset, not
broken pairing.** The reference (`001-cpmm-fee`) side of this comparison was filtered to
the identical set of surviving seeds via `filter_batch_to_seeds`, and `stats::paired_stat`
bails on the first index where the two inputs' seeds diverge, per element, while it computes
the diff vector (`stats.rs::paired_stat`'s `.map()` over `candidate.iter().zip(reference
.iter())`) — so any pairing break would have surfaced as an error rather than silently
mispairing, and none did: candidate and reference were correctly index-paired throughout. A
future reader re-deriving this number
should not go looking for a pairing bug; there isn't one. The defect is entirely that the
436 surviving seeds are not a random subset of the original 1000 (WHI-1249).

**`L in {5, 25}`**: out of scope per the issue regardless of this outcome (the `L=1` vs.
trade-triggered gap was never established as "surprising" enough to justify going beyond
`L=1`, since `L=1` itself could not be measured).

**Analytic envelope — formula re-derived for variant (a), no concrete comparison
possible.** `commands/ceiling.rs::analytic_envelope_l0_upper_bound` is a pure closed-form
calculation (`sum(retail volume_y) x captured spread`, at 100% flow share and zero adverse
selection) that does not itself depend on the fingerprint cursor — its doc comment
re-derives the bound for variant (a) explicitly: inventory drift away from `target_x ==
ANCHOR_X` (fixed, unlike variant (b)'s `target_x == reserve_x`) can only ever *reduce* the
realized captured spread relative to this flat-spread idealization (a standard AMM
inventory-skew effect), so the bound remains valid, just looser than variant (b)'s. It is
unit-tested for its own boundary behavior
(`analytic_envelope_l0_upper_bound_is_zero_for_zero_spread`,
`_is_positive_for_positive_spread`). What cannot be produced is a **concrete number** to
compare it against: computing the envelope for a specific `spread_bps` requires a fitted
point, and the only fit pipeline available for `CursorMode::Fingerprint`
(`evaluate_fingerprint_point`) suffers the identical selection-bias problem — it may steer
the search itself toward parameter regions that happen to produce fewer floor collisions
rather than the true optimum, so even the *fitted point* (not just the final number) is
untrustworthy here.

**Superseded by WHI-1250, below.** This limitation was specific to the search-time
contamination this paragraph describes; WHI-1250's `score_fingerprint_survivors` fix
**bounds** it — caps how large the below-threshold survivors-only bias is allowed to be,
and kills the rung outright as a documented negative if it isn't — it does not remove
survivors-only averaging itself, which still runs on every point the search evaluates
whenever that point's own trip rate is at or under the 1% threshold. What that bound does
buy is a fitted point the reader can act on: the WHI-1250 section below reports the actual
fitted point, the actual simulated `L=0` number, and the actual envelope comparison (PASS)
that this paragraph says cannot be produced, each disclosed as a survivors-only mean with
its own named/classified exclusions. Nothing here is retroactively wrong — it correctly
describes why no number existed *at the time WHI-1248 closed* — but a reader should not
stop at this paragraph believing the envelope is still unevaluated, nor should they read
WHI-1250 as having eliminated the search-time incentive described two paragraphs below.

**WHI-1249: naming this as a distinct finding — search-time contamination, not just
reporting-time bias.** `evaluate_fingerprint_point` scores each candidate parameter point
by averaging over whichever seeds happen to survive the fingerprint hardening checks for
*that* point — the same survivors-only averaging that biased the headline mean above, but
here it runs inside the search loop itself, on every point the optimizer evaluates, not
just on the final reported one. A point that trips more seeds into floor collisions
(disproportionately the adverse ones, since the collision-prone paths correlate with the
regime that would otherwise pull the mean down — see the High-sigma tier's near-total
attrition above) has those adverse seeds silently dropped from its own score, making it
look *better* than a point that survives on the same seeds honestly. The search therefore
has a standing incentive to drift toward fragile points — ones that produce more floor
collisions, not fewer — rather than toward the true optimum. The visible symptom in this
investigation: the fingerprint-mode fit converged at `concentration = 94.33` (within 6%
of the declared upper bound of 100.00), while the kept trade-triggered fit — evaluated
through a scoring path with no comparable survivors-only averaging — converged at
`concentration = 2.33`, deep in the interior of the same search space. A fit pinned
against its own boundary is itself a warning sign independent of this mechanism; here it
has a concrete causal candidate. The fix (restricting cursor-advance to buy-side matches,
named above) reduces the false-match rate driving this, but the search-time averaging
itself is not restructured by that fix and is not addressed in this issue; the code
change belongs to WHI-1250. This paragraph is the record of the finding, not a fix.

There is therefore no simulated `L=0` number to check against the
envelope, and none is reported; the formula's own boundary behavior is unit-tested, and it
is a candidate for reuse once (if ever) a non-selection-biased fingerprint-mode measurement
pipeline exists — but "unit-tested for its own boundary behavior" is the extent of what has
been verified. One caveat applies regardless of the selection-bias problem above and
survives any future fix to it: the envelope is deliberately a **retail-flow-only**
quantity (`sum(retail volume_y)`, matching the issue's own literal wording), while a real
simulated `l0_avg_edge` is built from `submission_edge`, which also includes whatever the
arbitrageur itself contributes. The two are therefore never a strictly apples-to-apples
comparison — a future `PASS` against this envelope would be consistent with the bound
holding, not proof the two quantities were computed over identical volume. See
`analytic_envelope_l0_upper_bound`'s own doc comment in `commands/ceiling.rs`.

**Per-sigma slices: not performed.** `commands/ceiling.rs::slice_by_sigma_tier`/
`format_sigma_slices` are implemented and reuse `regime.rs`'s existing tier reconstruction
exactly as scoped (no new `[grid]` axis), but slicing a rung that has no valid headline
measurement produces no meaningful converge/fan-out verdict — there is nothing to slice.
No "converge at low sigma / fan out at high sigma" statement can be made for `L=1`/`L=0`
under this method.

**Acceptance criteria — explicit accounting (WHI-1248):**

- Hardening checks (a)/(b)/(c) implemented and unit-tested to fire on a corrupted
  cursor/stalled terminal state: **met**.
- Full lane run reports zero assertion failures, or names every tripped seed + re-run
  outcome: **met, but the outcome is the negative result above** — every tripped seed
  Phase 2 processes is individually named, classified, and either recovered or reported
  with its own message (`run_fingerprint_final_eval`); none is silently dropped. The
  *volume* of trips (not their individual handling) is what disqualifies the resulting
  average from being reported.
- `curve_checks` panic from an early cursor advance caught and labelled distinctly:
  **met** (`classify_fingerprint_panic`'s `"SuspectedEarlyAdvance"` bucket, unit-tested).
- Headline `L=1` variant (a) paired-by-seed mean diff + 95% CI on `validation`: **not
  met, deliberately** — see Decision above; any such number is selection-biased.
- `L=0` variant (a) diagnostic sits below the analytic envelope: **not evaluable** — no
  valid simulated `L=0` number exists to compare; the envelope formula itself is
  implemented, re-derived for variant (a), and unit-tested (boundary cases only — see the
  scope caveat above: it is a retail-flow-only bound, not a like-for-like quantity against
  `submission_edge`).
- Per-sigma slices for every rung with an explicit converge/fan-out statement: **not
  met** — no valid rung exists to slice; the slicing/formatting code is implemented and
  ready.
- `rand_pcg`/`rand_distr` pinned in `tools/bench/Cargo.toml` only: **met**
  (`rand = "0.8.5"`, `rand_pcg = "0.3.1"`, `rand_distr = "0.4.3"`, not touching the
  workspace `[workspace.dependencies]` table).
- Panic-payload downcast fix produces readable messages: **met and demonstrated** — the
  step 3201/3202 trace above shows the exact, readable
  `"WHI-1248 fingerprint cursor hardening check (a): ..."` assertion text recovered
  verbatim, not the pre-fix `"panicked with a non-string payload"` placeholder.
- One-sided-bound caveat repeated: **N/A for this rung** — no number is reported for
  `L=1`/`L=0`, so there is no ceiling figure to attach the caveat to; the caveat continues
  to apply, unchanged, to WHI-1247's own committed trade-triggered number above.

**What this does not reopen.** `002`'s own §6.2 row, §2.10's addition clause, and the
`test` segment remain untouched, exactly as scoped. `L in {5, 25}` was never attempted, for
the reason given above. WHI-1247's own trade-triggered `anchored` measurement
(87.234885 edge/sim above the 0-line, `observation`, n=1000) is unaffected by any of this
— it uses a different cursor mode entirely and is not implicated by the fingerprint-mode
finding.

## WHI-1250 — buy-probe-only cursor advance, and the pre-registered kill rule

**What changed.** `tools/bench/src/oracle.rs::maybe_advance_fingerprint_cursor` now
advances the fingerprint cursor only on a `side == 0` (buy) probe match against the next
step's own buy target. A `side == 1` (sell) call still sets the `SEEN_SIDE1_SINCE_ADVANCE`
gate (kept as the backstop it was written for — a buy-before-sell collision within the
same step), but can never itself advance the cursor any more. This directly attacks the
mechanism WHI-1248/WHI-1249 traced: the sell-side floor clamp (`min_sell_input_x`, binds at
roughly P ≈ 1e-4/step once `fair_price > 10`, true for essentially this entire run) was
being routinely re-probed as `bracket_maximum`/`golden_section_max`'s own early-return `lo`
value and false-matched against the next step's target; the buy-side floor
(`min_buy_input_y`, the constant `FP_MIN_ARB_NOTIONAL_Y`) binds roughly four orders of
magnitude more rarely, and the real arbitrageur's own first `compute_swap` call every step
is always the buy probe (`Arbitrageur::execute_arb`'s `Self::best_candidate(self.
plan_arb_buy_x(..), self.plan_arb_sell_x(..))` evaluates its first argument — the whole
buy-side search — before its second, by Rust's left-to-right argument evaluation), so a
buy-only advance rule removes the dominant ambiguity class by construction. Proven by a new
unit test, `oracle::tests::side1_probe_never_advances_the_cursor_even_on_an_exact_sell_
target_match`, which drives a `side == 1` call against an exact match of the *next*
target's own sell component with the gate already open — pre-WHI-1250 code would have
advanced here; post-WHI-1250 code does not, and only a subsequent `side == 0` match against
the next target's buy component does.

**The search-time contamination WHI-1249 flagged is bounded, not removed, in the
same change.** `commands/ceiling.rs::evaluate_fingerprint_point` previously scored every
candidate point by averaging over whichever seeds survived that point's own fingerprint
hardening checks, with no upper bound on how many seeds could be silently excluded from the
average — exactly the mechanism suspected of pulling the pre-fix fit to
`concentration = 94.33`, near its own declared upper bound (docs/DEFERRED_ISSUES.md's
WHI-1249 entry). Below the kill-rule threshold this fix still averages over survivors only
— `docs/DEFERRED_ISSUES.md`'s WHI-1249 entry asks for the search's scoring path to "stop
silently dropping tripped seeds (e.g. penalize instead of exclude, or score over the full
seed set with a fixed penalty for tripped ones)"; nothing here does that. What this fix
does do is cap *how large* that drop is allowed to be before the point is thrown out
outright (`Invalid`, no mean reported). Disclosure is asymmetric between the two callers
of this shared decision, and that asymmetry is deliberate, not a gap this fix missed:
`run`'s `CursorMode::Fingerprint` branch (the real final-evaluation headline decision) does
serially re-run every tripped seed to recover a named, classified message — that is where
"no longer silent" actually holds, and it is what makes the per-seed tables below possible.
`evaluate_fingerprint_point` (the `screening`-segment search loop, up to 300 candidate
points per fit) does not: its own doc comment states it "never serially re-runs a tripped
seed to recover its message" — paying that cost on every candidate point would be pure
waste for numbers that are never reported — so a below-threshold candidate point's dropped
seeds are still silent during the search itself, known only as a bare count, exactly the
mechanism `docs/DEFERRED_ISSUES.md`'s WHI-1249 entry describes. The fix factors the shared
bounded-exclusion-plus-invalidate decision into one pure function,
`score_fingerprint_survivors(oks, tripped_count, total_seeds)`, applied identically by both
callers — see the kill rule below for what it does — but "applied identically" is about the
kill-rule threshold and the `Invalid` cutoff, not about how much identifying detail either
caller has available to disclose. `commands/ceiling.rs`'s new unit tests exercise both
`Invalid` branches directly against synthetic survivor/trip counts (not a real
simulation's own trip rate, which this fix is specifically trying to lower) —
`score_fingerprint_survivors_is_invalid_when_trip_rate_exceeds_the_kill_threshold_even_
with_survivors` is the acceptance-criterion test: 9 survivors, 91 tripped (91%, well past
the threshold below) must score `Invalid`, not a mean over the 9 that happened to survive.

**The pre-registered kill rule (recorded here before any measurement was taken under this
fix — this section was written, and this repo's own commit history for this issue shows
it, before the first `--fit`/final-eval command below was run):**

> If the per-seed fingerprint hardening-check trip rate on the segment being measured
> exceeds **1% (strictly more than 10 of 1000 seeds on `validation`, or more than 2 of 200
> on `screening`** — the rule is a rate, not a hardcoded count, so it applies unmodified to
> either segment size), that point/run is scored `Invalid` and the rung is closed as a
> documented negative/method-level result: no mean-edge-diff number, no CI, and no
> per-sigma slice is reported. Trip data (every tripped seed's own seed number,
> classification, and message, via `format_tripped_seeds`) is reported as the evidence for
> the negative result instead. This threshold is not raised after seeing a measured trip
> rate, under any circumstance — the entire purpose of writing it down first is to remove
> that judgment call once a number is in hand. At or below the threshold, the point is
> scored `Valid` and its mean **is** reported — but that mean is still, and must always be
> disclosed as, a **survivors-only mean**: the exact survivor count out of the segment's
> total, and every excluded seed's own number and classification, are reported alongside
> it, never folded into a bare figure that reads as if the full segment survived.
> WHI-1249's own finding above is that a tripped seed is a deterministic,
> RNG-stream-correlated event, not an independent draw, so even a "small" excluded
> fraction is a biased one, not merely a noisier one. This threshold bounds how large that
> bias is *allowed* to be before the rung is killed outright as a documented negative —
> it does not make a below-threshold exclusion disappear, and it does not license
> reporting the survivors-only number as though every seed survived.

Implemented as `commands/ceiling.rs::FINGERPRINT_TRIP_RATE_KILL_THRESHOLD = 0.01` and
`fingerprint_trip_rate_exceeds_kill_threshold`, checked in both
`score_fingerprint_survivors` (search loop) and directly in `run`'s `CursorMode::
Fingerprint` branch (final evaluation), using strict `>` — exactly 10-of-1000 (1.0%) does
not trip the rule, matching the rule's own "exceeds 1%" wording; 11-of-1000 (1.1%) does.
Unit-tested at the boundary on both segment sizes
(`fingerprint_trip_rate_exceeds_kill_threshold_is_false_at_exactly_the_boundary`,
`_is_true_just_past_the_boundary`, `_applies_the_same_rate_to_a_smaller_segment`).

**Measurement outcome — recorded here once, immediately after it was taken, under the
rule above:**

<!-- WHI-1250-MEASUREMENT-PLACEHOLDER: now filled in below with the actual re-fit
     and final evaluation results; this comment marker is kept, unreplaced, as the
     literal record that the kill rule above was written and committed (`492df79`)
     before any measurement was taken (`85a05f5`), per the ordering constraint this
     section opens with — the marker's own presence is the evidence, not a to-do. -->

### `L=1` headline, `validation` (the substantive question)

Fitted point: `concentration = 62.4200, spread_bps = 178.0000` — genuinely distinct from
the pre-fix, search-time-contaminated fit's `concentration = 94.33` (WHI-1249), consistent
with `score_fingerprint_survivors` actually changing the search's own behavior and not just
gating the final number.

| field | value |
|---|---|
| n | 991 of 1000 |
| mean edge diff (oracle − `001-cpmm-fee`) | 161.468800 |
| std error | 6.465080 |
| 95% CI | [148.797477, 174.140123] |
| tripped seeds | 9 (0.9% of 1000) |

**The pre-registered kill rule did not fire.** 9 of 1000 seeds (0.9%) is at or below the
1% threshold recorded above, so per that rule — committed in `492df79`, before this
measurement was taken — the point is scored `Valid` and this mean is reportable.

**That is not the same thing as the acceptance criterion "all seeds surviving" being
met — it is not met.** n = 991, not 1000. The kill rule passing means this rung is not
*closed* the way WHI-1248's was; it does not mean the exclusion did not happen. This is,
plainly, a **survivors-only mean**, just at a 0.9% exclusion rate rather than WHI-1248's
56.4% — smaller by roughly two orders of magnitude, but not zero, and it is disclosed here
on exactly the same terms the kill-rule paragraph above requires: the exact survivor count,
and every excluded seed named and classified, not folded into a bare "n=1000"-looking
figure.

The 9 tripped seeds (Phase 2 serial re-run, each individually classified — none silently
dropped):

| seed | classification | message |
|---|---|---|
| 2000010 | SuspectedEarlyAdvance | submission shape violation during arbitrage sell search: monotonicity violated (`crates/sim/src/curve_checks.rs:23:9`) |
| 2000023 | SuspectedEarlyAdvance | submission shape violation during arbitrage sell search: monotonicity violated (`crates/sim/src/curve_checks.rs:23:9`) |
| 2000056 | Other | submission shape violation during arbitrage sell search: monotonicity violated (`crates/sim/src/curve_checks.rs:23:9`) |
| 2000088 | SuspectedEarlyAdvance | submission shape violation during arbitrage sell search: monotonicity violated (`crates/sim/src/curve_checks.rs:23:9`) |
| 2000319 | SuspectedEarlyAdvance | submission shape violation during arbitrage sell search: monotonicity violated (`crates/sim/src/curve_checks.rs:23:9`) |
| 2000335 | SuspectedEarlyAdvance | submission shape violation during arbitrage sell search: monotonicity violated (`crates/sim/src/curve_checks.rs:23:9`) |
| 2000573 | Other | submission shape violation during arbitrage sell search: monotonicity violated (`crates/sim/src/curve_checks.rs:23:9`) |
| 2000716 | SuspectedEarlyAdvance | submission shape violation during arbitrage sell search: monotonicity violated (`crates/sim/src/curve_checks.rs:23:9`) |
| 2000895 | SuspectedEarlyAdvance | submission shape violation during arbitrage sell search: monotonicity violated (`crates/sim/src/curve_checks.rs:23:9`) |

Full messages (input/output pairs) are in the committed report,
`results/2026-08-24-ceiling-anchored-fingerprint-l1-validation-orbic-oracle-vs-001-cpmm-fee.md`.
All 9 are, per their own report messages, genuinely the same class of trip WHI-1248/WHI-1249
already characterized (a sell-side floor false-match); the buy-probe-only fix reduced the
count from 564 to 9 (98.4% reduction) but did not, and was never claimed to, reduce it to
exactly zero. (A distinct, much rarer buy-side residual — the class the buy-probe-only fix
cannot remove by construction, since it only ever targets sell-side false-matches — shows up
instead in the `L=0` and `observation` rows below; see the notes there.)

**All 9 excluded seeds sit in the `High` sigma tier** (verified directly, seed by seed,
via `regime::classify_seed(&SimulationConfig::default(), seed).sigma` in a throwaway
scratch test against the same `SimulationConfig::default()` base the per-sigma-tier slices
below use — a precise per-seed result, not an inference). The tier-count tables below are
consistent with this but do not, on their own, prove exactly 9: `L=1`'s own tier slices are
339/349/**303** (n=991) against `L=0`'s 339/349/**311** (n=999, 1 tripped) — Low and Mid are
identical between the two runs, so the count delta alone bounds the structural difference to
"8 or 9 of the 9 excluded seeds are in High" (it pins down 8 unconditionally; the 9th depends
on which tier `L=0`'s own single excluded seed, `2000504`, itself falls in). The direct
`classify_seed` check above resolves that remaining ambiguity to exactly 9; it is the primary
evidence, and the tier-count delta is corroborating, not the reverse. Practically, this means
the exclusion is not scattered evenly across the sample: it draws entirely from the tier with
the highest positive mean (see the per-sigma table below, High ≈291 vs. Low ≈67). If — and
only if — the 9 excluded seeds' own (unmeasured, and unmeasurable in principle, since
exclusion is precisely why they have no reported mean) true values are drawn from
something like that tier's own distribution rather than the implausible −6,490 figure
below, this would, if anything, pull the true full-sample mean *above* the 161.4688
survivors-only figure, not below it — but this is a conditional illustration, not a claim
that the excluded seeds' true values are known or knowable from this data. This does not
license reporting a higher number; it is disclosed here as part of what the exclusion
structure actually looks like, not folded silently into the bias-bound paragraph that
follows.

**Bias bound on the 9 excluded seeds.** For the full-1000-seed mean to fall all the way to
`008`'s own `observation`-segment headline of +101.60, the 9 excluded seeds would have to
average, among themselves:

```
X = (1000 × 101.60 − 991 × 161.4688) / 9
  = (101600 − 160015.5808) / 9
  = −58415.5808 / 9
  ≈ −6490.62
```

i.e. each excluded seed would need to average roughly **−6,490** edge/sim — about **19.3×**
worse than the single worst per-tier mean ever observed anywhere in this investigation
(WHI-1248's biased High-sigma tier, −336.257804, itself already the tail of a
selection-biased sample). Nothing in this repo's own data suggests a plausible mechanism
for a per-seed mean anywhere near that magnitude; the 9 excluded seeds are far more likely,
on the evidence of what they actually are (a floor-collision false-match, not an economic
outlier), to sit somewhere in the same broad range as the 991 survivors than to average
−6,490. This bounds, but does not eliminate, how much the exclusion could be masking — it
converts "9 seeds were dropped" from an unquantified weakness into an arithmetically
bounded one. **This bound is presented as context, not as a substitute for the AC being
unmet**, and the `008` figure it references is `observation`-segment, not `validation` —
comparing a `validation`-segment number's own robustness against an `observation`-segment
threshold value is a deliberate choice of a concrete, previously-established benchmark, not
a same-segment paired comparison; §2.2's non-decision-input status for `observation` and
the "never a computed pair" constraint on the `008` comparison both continue to apply
unchanged (see the bracket-placement paragraph below).

**Per-sigma-tier slices, `L=1`/`validation`** (labels per WHI-1249's corrected
`regime::tier_bounds`):

| sigma tier | tier range | n | mean diff | 95% CI |
|---|---|---|---|---|
| Low | [0.0001, 0.0024) | 339 | 66.911770 | [47.006290, 86.817249] |
| Mid | [0.0024, 0.0047) | 349 | 140.536290 | [121.755982, 159.316599] |
| High | [0.0047, 0.0070) | 303 | 291.370713 | [271.149320, 311.592105] |

**Converge/fan-out verdict: HELD, but by a margin this section is not entitled to call
decisive** — Low tier's 95% CI width (39.810960) is narrower than High tier's (40.442785),
consistent with "converge at low sigma / fan out at high sigma," but the two widths are only
1.6% apart (WHI-1248's own precedent gap on this same file was 37.76 vs 209.76 — an order of
magnitude, not a few percent). CI width is a coarse dispersion proxy in the first place
(`stats::PairedStat` exposes no per-bin raw variance), so this verdict should be read as
"not contradicted," not as a strong confirmation of the converge/fan-out prediction. (These
991-seed tier slices carry the same survivors-only caveat as the headline above; they are
not re-derived on a hypothetical full-1000 sample.)

### `L=0` diagnostic, `validation` (analytic-envelope check)

Fitted point: `concentration = 99.6200, spread_bps = 95.0000`.

| field | value |
|---|---|
| n | 999 of 1000 |
| mean edge diff (oracle − `001-cpmm-fee`) | 400.295089 |
| std error | 6.514270 |
| 95% CI | [387.527354, 413.062823] |
| tripped seeds | 1 (0.1% of 1000) |

Also a survivors-only mean, disclosed on the same terms: 1 seed excluded
(`2000504`, `SuspectedEarlyAdvance`), 999 survive. Unlike the 9 seeds excluded from the
`L=1` headline above, this one is **not** the sell-side floor false-match class the
buy-probe-only fix targets — its own report message is "submission shape violation
during arbitrage **buy** search," the rarer buy-side residual the fix cannot remove by
construction (`min_buy_input_y`/`FP_MIN_ARB_NOTIONAL_Y` binds roughly four orders of
magnitude less often than the sell-side floor, per the buy-probe-only rationale above, but
not literally never). It also sits in the `High` sigma tier, same as every excluded seed
on every rung of this section (verified via `regime::classify_seed`).

**Analytic-envelope check — required by this issue, and a blocker if it had failed:**

| field | value |
|---|---|
| simulated `L=0` avg edge (raw, not the paired diff above) | 802.284247 |
| closed-form upper envelope (`analytic_envelope_l0_upper_bound`) | 1502.562225 |
| result | **PASS — sits below the envelope** (802.284247 ≤ 1502.562225) |

Scope caveat carried over unchanged from WHI-1248: the envelope is a retail-flow-only
quantity (`sum(retail volume_y)`); the simulated figure also includes whatever the
arbitrageur itself contributes, so this is not a strictly like-for-like comparison — a
`PASS` is consistent with the bound holding, not proof the two quantities are computed over
identical volume.

**Per-sigma-tier slices, `L=0`/`validation`:**

| sigma tier | tier range | n | mean diff | 95% CI |
|---|---|---|---|---|
| Low | [0.0001, 0.0024) | 339 | 263.483173 | [248.973806, 277.992540] |
| Mid | [0.0024, 0.0047) | 349 | 358.504837 | [343.339849, 373.669826] |
| High | [0.0047, 0.0070) | 311 | 596.320931 | [576.507105, 616.134758] |

**Converge/fan-out verdict: HELD** — Low tier's CI width (29.018734) is narrower than High
tier's (39.627653), same direction as `L=1`'s verdict above.

### `observation` context row (§2.2 — non-decision-input, reported for continuity only)

`--variant anchored --fit --segment observation --cursor fingerprint --lag 1`. Same
screening-fit point as the `validation` headline above (`concentration = 62.4200,
spread_bps = 178.0000` — the search fits on `screening`'s fixed 200 seeds regardless of
which segment the final evaluation runs against, so this is expected, not a coincidence).

| field | value |
|---|---|
| n | 992 of 1000 |
| mean edge diff (oracle − `001-cpmm-fee`) | 157.807787 |
| std error | 6.284680 |
| 95% CI | [145.490040, 170.125534] |
| tripped seeds | 8 (0.8% of 1000, under the 1% kill-rule threshold) |

Also a survivors-only mean, disclosed the same way as the `L=1`/`validation` table
above — every excluded seed individually named and classified, not a disjunction:

| seed | classification | message |
|---|---|---|
| 125 | SuspectedEarlyAdvance | submission shape violation during arbitrage **buy** search: monotonicity violated (`crates/sim/src/curve_checks.rs:23:9`) |
| 212 | SuspectedEarlyAdvance | submission shape violation during arbitrage sell search: monotonicity violated (`crates/sim/src/curve_checks.rs:23:9`) |
| 243 | Other | submission shape violation during arbitrage sell search: monotonicity violated (`crates/sim/src/curve_checks.rs:23:9`) |
| 268 | SuspectedEarlyAdvance | submission shape violation during arbitrage sell search: monotonicity violated (`crates/sim/src/curve_checks.rs:23:9`) |
| 292 | Other | submission shape violation during arbitrage sell search: monotonicity violated (`crates/sim/src/curve_checks.rs:23:9`) |
| 609 | Other | submission shape violation during arbitrage sell search: monotonicity violated (`crates/sim/src/curve_checks.rs:23:9`) |
| 925 | Other | submission shape violation during arbitrage sell search: monotonicity violated (`crates/sim/src/curve_checks.rs:23:9`) |
| 933 | SuspectedEarlyAdvance | submission shape violation during arbitrage sell search: monotonicity violated (`crates/sim/src/curve_checks.rs:23:9`) |

Full messages (input/output pairs) are in the committed report,
`results/2026-08-24-ceiling-anchored-fingerprint-l1-observation-orbic-oracle-vs-001-cpmm-fee.md`.
7 of the 8 are the dominant sell-side floor false-match class; seed `125` is, per its own
report message, the same rarer buy-side residual class as `L=0`'s `2000504` above, not
the sell-side class — the buy-probe-only fix does not and cannot remove this residual by
construction. All 8 excluded seeds sit in the `High` sigma tier — verified directly via
the same `regime::classify_seed` scratch check as the `L=1`/`validation` rung above, the
same concentration pattern found there. Unlike that rung, this claim has no independent
structural cross-check available (there is no sibling `observation`-segment run at a
different fit point to compare tier-count deltas against, the way `L=1` and `L=0` on
`validation` cross-check each other above); it rests on the direct per-seed verification
alone.

Per-sigma slices: Low n=350 mean=71.111180 CI[51.874998, 90.347361]; Mid n=327
mean=118.839503 CI[101.542684, 136.136321]; High n=315 mean=294.590140
CI[275.141342, 314.038938] — Low CI width (38.472363) narrower than High's (38.897595), a
1.1% margin — **HELD, same direction as `validation`, with the identical small-margin
caveat given there** (not a decisive confirmation, not contradicted either). **Non-decision-input per §2.2: this row is
reported for continuity with WHI-1247's own `observation`-segment headline and is not
used to decide anything in this issue** — the `validation`-segment measurement above is
what the kill rule, the bias bound, and the bracket placement below are all computed
against.

### Where this places the substantive question

This issue opened with an informal, pre-measurement bracket for `L=1` of
`[≈+87.23, ≈+1,632]` — WHI-1247's trade-triggered +87.234885 as the floor this fix had to
clear or fall short of, and a rough estimate of the `L=0` diagnostic ceiling as the
informal top, before any `L=0` number existed to compute one from. That top is now
superseded by an actual measurement, not a re-estimate chased after the fact: the
closed-form envelope computed above is **1502.562225** (≈+1,503), and the `L=0` diagnostic
sits at 802.284247, comfortably under it. The bracket is therefore restated using the real
number now in hand: **`[≈+87.23, ≈+1,503]`**. The measured `L=1`/`validation` figure,
**+161.468800** (survivors-only, n=991/1000, 0.9% excluded, kill rule did not fire), lands
inside that bracket, above the trade-triggered floor and well below the `L=0` ceiling —
consistent with "an every-block re-anchor beats a trade-triggered one, but by less than the
idealized, never-front-run `L=0` case." As **context only**, `008`'s own
`observation`-segment headline is +101.60 — the measured `L=1`/`validation` figure of
+161.4688 is larger, but this is never reported as a computed pair: the two numbers sit on
different segments (`validation` vs. `observation`), so this placement is qualitative, not
a claim that this fix "beats `008` by 59.87."

The `observation` context row measured above (+157.807787) is a narrower case in one
respect and a weaker one in another. Narrower: it is on the *same* segment as `008`'s own
headline, so — unlike the validation-based headline above — the two numbers are at least
measuring the same thing. Weaker: +157.807787 is itself a survivors-only mean over n=992,
with all 8 exclusions concentrated in the highest-mean `High` sigma tier (disclosed above),
while `008`'s own +101.60 is that report's full-sample headline; a subtraction across those
two would not be an apples-to-apples "beats `008` by N" figure without first restating one
side or the other on a matching basis, which this document does not do. For that reason no
such subtraction is computed or reported here, on top of the independent §2.2 reason that
`observation` is non-decision-input for this issue. Both reasons hold at once; neither is
being used to paper over the other.

Whether an every-block oracle re-anchor beats what `008` achieves with no oracle at all —
the question this issue exists to close — is answered **yes, on the evidence available
here**, resting on the `validation`-segment reading alone (+161.468800, survivors-only,
n=991/1000, cross-segment against `008`'s `observation` headline, kill rule did not fire).
The `observation` context row is reported for continuity per §2.2 and happens to point the
same direction, but it is **not** cited as a second, corroborating basis for the "yes"
answer — doing so would use a declared non-decision-input row to help decide the very
question §2.2 excludes it from deciding. The caveats above (survivors-only, cross-segment,
one-sided lower bound, retail-flow-only envelope) attach to that "yes," not hidden behind
it. The reference allowlist for this comparison remains unchanged: `000-normalizer`,
`001-cpmm-fee`.
