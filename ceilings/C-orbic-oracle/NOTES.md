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

- Commit: `e319f56` (the committed `results/*.md` reports carry this as their own
  `Commit:` line; the underlying measured numbers are unchanged from the original
  `49509a3` run — round-1 review only changed report/NOTES.md text, not simulation
  logic, and both runs were independently re-verified byte-identical on the numeric
  fields before being recommitted)
- Segment: `observation` (1000 sims, 10,000 steps each, reporting-only —
  `docs/DESIGN.md` §2.2 — never a decision input)
- Execution path: native (host-side, never BPF-compiled — this lane is not submittable)
- The fit search itself runs on the fixed `screening` segment (200 sims,
  `config/bench.toml`'s `[search] max_points = 300`), per `tools/bench/src/commands/ceiling.rs`'s
  `run_fit`, independent of which segment the final measurement reports against.

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
