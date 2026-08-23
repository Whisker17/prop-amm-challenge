# C-orbic-oracle

**Out-of-competition (`ceilings/README.md`, WHI-1247).** Nothing in this file describes a
submittable strategy — there is no `lib.rs` here, and no number below is a ranked M1/M2
result. This is a *ceiling* probe: how much edge is on the table when a curve is granted a
price re-anchor the arbitrageur cannot front-run, measured against `001-cpmm-fee` (the
0-line), before anyone spends real search budget chasing the same gap with a submittable,
front-runnable mechanism.

## Provenance

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
fit's own search (below) hit **110 invalid (panicking) points against 184 valid ones** out
of the 300-point budget. That is treated as expected and load-bearing evidence *for* why
`002` was right to cancel as a submission candidate, not as a defect in this lane — every
invalid point is caught (`std::panic::catch_unwind`, mirroring `crates/sim`'s own
`curve_checks.rs` shape-check panics per `docs/DESIGN.md` §2.4) and excluded from the peak
search rather than either scored `-inf` or silently skipped.

The same jitter also surfaced one level up, past the search itself: the *final*
re-evaluation of a fitted point (on `observation`, not `screening`) is a second, separate
call into the curve, and `ceiling.rs`'s first implementation did not guard that call the
way `run_fit`'s search loop already guarded its own evaluations. Running the real
`--variant floating --fit --concentration 2.33 --segment observation` measurement hit
exactly that gap — a monotonicity-violation panic during `observation`'s re-evaluation,
after a clean pass on all of `screening`'s points — and crashed the whole process before a
report could be written. Commit `49509a3` closes that gap with a second, staleness-
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

- **`anchored`** (headline): `target_x` is pinned to the pool's own initial `reserve_x`
  (`ANCHOR_X`, captured once per simulation) — this is the variant that actually uses the
  oracle re-anchor the arbitrageur cannot front-run.
- **`floating`** (degenerate diagnostic): `target_x = reserve_x` on every swap, i.e. no
  re-anchor at all — included to isolate how much of `anchored`'s edge comes from the
  concentration/spread shape alone versus from the re-anchor itself.

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
- **spread_bps upper bound 1_000 (10%)** — an order of magnitude above the reference's own
  fee (`001-cpmm-fee`'s frozen range tops out at 500bps / 5%, `strategies/001-cpmm-fee/NOTES.md`),
  giving the search headroom to find an interior optimum rather than hit a boundary, while
  still being small enough that the oracle-anchored quote stays economically a "spread," not
  a arbitrary markup.
- **spread_bps lower bound 0** — a spread of exactly zero is a legitimate point (pure
  concentration effect, no spread contribution) and must be reachable, not excluded.

## Fitted point

Reproducibility per `docs/DESIGN.md` §3.3 (segment, sim/step counts, execution path, commit
sha) — both runs below share:

- Commit: `49509a3`
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
- Search budget spent: 184 valid points (of 300; 110 invalid/panicking, as discussed in
  Provenance above), not exhausted
- Best avg edge on `screening`: 473.787933
- **Mean edge diff (oracle − `001-cpmm-fee`) on `observation`: 87.234885**, 95% CI
  `[81.016163, 93.453606]`, n=1000, std error 3.172875 — this is the ceiling number: the
  ceiling lane's oracle-anchored curve beats the 0-line by ~87 edge/sim on average when
  granted a re-anchor no submittable strategy can actually have.
- Trade-triggered cursor staleness (steps since the oracle cursor's last executed trade):
  mean of per-sim means 3.886, median of per-sim means 3.144, max of per-sim p95 109.000, max
  of per-sim max 205.000 — the re-anchor is current almost all the time (single-digit mean
  staleness) but has a long tail of quiet stretches up to ~200 steps in the worst simulated
  path.

### `floating` (degenerate diagnostic) — spread-only re-fit, concentration held at 2.33

Run via `cargo run -p prop-amm-bench -- ceiling --variant floating --fit --concentration 2.33 --segment observation`.
Committed report: `results/2026-08-23-ceiling-floating-trade-triggered-observation-orbic-oracle-vs-001-cpmm-fee.md`.

- **spread_bps = 77.0000 (0.77%)** — the best point the search found on `screening` before
  the point below turned out to be **INVALID**
- Search budget spent: 165 valid points (of 300; 9 invalid/panicking on `screening`), not
  exhausted
- Best avg edge on `screening`: 639.575681
- **Final re-evaluation on `observation`: INVALID.** Re-running this exact point
  (`concentration = 2.33, spread_bps = 77.0`) against `observation`'s 1000 seeds — a
  different, larger seed set than the 200 `screening` seeds the search used — triggered a
  `crates/sim/src/curve_checks.rs` monotonicity-violation panic, caught rather than crashed
  (see Provenance above). No mean edge diff, no CI, and no staleness numbers exist for this
  point; the search's own clean pass on `screening` was not evidence this point was safe on
  a different seed set. This is the documented failure mode itself (docs/DESIGN.md
  §2.4/§2.5/WHI-1213), not a bug in the point chosen.

### Anchored vs. floating — what the contrast shows

The two variants land in qualitatively different places, and that gap is itself the
finding this diagnostic exists to produce (WHI-1247 step 3): `anchored`'s `target_x`,
pinned once per simulation to the pool's own starting reserves, keeps the curve's shape
stable enough to search *and* to re-evaluate cleanly on a seed set five times the size of
the one the search used — 184 of 300 candidate points during the search were invalid, but
the chosen point held up on `observation`. `floating`'s `target_x = reserve_x` on every
swap lets the curve's own trading activity feed back into its own shape parameter on every
single swap, not just at simulation start; that closer-to-the-edge dynamic degenerates
harder — fewer invalid points appeared during the `screening`-segment search (9 of 300)
than for `anchored`, yet the one point the search settled on still failed to survive
re-evaluation on a five-times-larger, different seed set. In other words: `floating` is
not merely *lower-edge* than `anchored`, as the original "isolate how much of anchored's
edge is the re-anchor" framing anticipated — it is unable to produce a trustworthy number
on this segment at all. That is exactly why `floating` was scoped from the start (see
"Two `target_x` variants" above) as "never a result on its own," and it is exactly the
Orbic family's own quantization jitter (`docs/DESIGN.md` §6.2, `002`, WHI-1206, Canceled)
showing up a second time, one level removed from the search itself. The ceiling number this
lane exists to produce is `anchored`'s 87.234885 edge/sim above; `floating` contributes no
comparable number, only the confirmation that the re-anchor — not just the concentration/
spread shape — is load-bearing for even having a well-behaved curve to measure.
