# 006-hedged-pnl

## Provenance

Source form: **prose (HackMD document)** — the lowest-fidelity, highest-provenance-risk entry
in the frozen list (`docs/DESIGN.md` §6.2, §2.9). `docs/references/006-hedged-pnl/README.md`
carries the full provenance flag: the source document titles itself as describing *this*
challenge and its stated volatility range (`U[0.01%, 0.70%]`) matches
`crates/shared/src/config.rs` exactly, but its own claimed scoring metric ("Hedged PnL", a
terminal-inventory formula) does not match this repo's actual per-trade average-edge metric
(`docs/DESIGN.md` §2.1). Per the issue's own scoping note, this port does **not** attempt to
reproduce "Hedged PnL" scoring — there is nothing to port there, since this repo's bench/sim
already scores everything on average edge. What is ported is the doc's "Linear Price Impact
Model" mechanism section only.

No numeric anchor exists in the source at all (unlike `001`/`004`/`005`, which each ship at
least one concrete constant set) — the four cross-impact coefficients (`k++`, `k+-`, `k-+`,
`k--`) are given only as a functional form.

## Fidelity self-assessment (docs/DESIGN.md §2.9)

**This is not a faithful line-for-line port of the doc's raw formula — it cannot be one.**
The issue's own review amendments (added 2026-08-21, superseding the original implementation
steps) establish that the doc's literal 4-coefficient, unpatched quadratic formula fails
`prop-amm validate`'s strict monotonicity check for **any** value of its own impact slope: a
large slope hits the vertex before size 200 (a real, non-plateau failure since `validate.rs`
requires *strict* increase), and a small slope pushes the raw quadratic's output past the live
reserve before size 200, which the harness zeroes — and a zero re-entering the monotonicity
sample set panics. There is no surviving choice of the doc's own constants, so "minimum
changes to make it run at all" (the fidelity contract's own bar) requires substantially more
structure than a literal transcription. What follows is that minimum, declared explicitly per
the contract's "provenance is mandatory" clause:

1. **4D collapsed to 2D.** The doc's four independent coefficients are dynamically unstable
   under this harness's own trade volume (see § Frozen parameter space below) — the only
   stationary manifold is `k-+ = k++` and `k+- = k--`, which further collapses under GBM's
   zero drift and 50/50 retail symmetry to a **single shared impact slope `k`**. This means
   the implemented mechanism is "one resting mid plus a fixed half-spread, both sides sharing
   one impact slope" — a real reduction of the doc's own model, not an approximation of it by
   coincidence: the reduction *is* the doc's own stability argument, applied.
2. **A saturating tail beyond a switch point** (`saturating_tail` in `lib.rs`), value- and
   slope-matched to the raw quadratic, replacing the doc's own unbounded/vertex-bearing tail on
   both sides. This is invented structure the doc does not contain — necessary per point 1
   above, not a style choice.
3. **State initialisation from `reserve_y/reserve_x`** when storage is zeroed or unrecognised
   (`reserve_mid`), since `compute_swap` cannot write storage and the router/arbitrageur quote
   before the first `after_swap` ever fires (finding #4).
4. **A sane-band clamp** (`clamp_mid`, `[MIN_MID_FP, MAX_MID_FP]`) on every loaded or derived
   mid price, sanitising `validate.rs`'s random-byte storage probe and bounding drift over a
   full 10,000-step simulation (finding #5).
5. **Input clamped before squaring, on both sides**, and the buy side uses the stable
   inversion `x = 2*SCALE*y / (sqrt(buy_fp^2 + 2*K_SCALED*y) + buy_fp)`, never the
   cancellation-prone `(sqrt(...) - buy_fp)/K_SCALED` form (finding #7).

None of this is disguised as faithful — it is the issue's own prescribed minimal-adaptation
set (its "declare it up front" section), implemented as specified. What *is* preserved
faithfully: the core economic idea (a two-sided market maker with a linear-in-flow impact
slope, no separate "fee" term — the impact itself captures spread) and the doc's own named
failure mode being taken seriously (a fixed slope is not scale-invariant the way a CPMM's fee
is, which is exactly what the grid mode results below confirm).

## Shape-safety rule (docs/DESIGN.md §2.9, cross-cutting finding #3)

Inside `compute_swap`, both `buy_output` and `sell_output` read only `rx`/`ry` and `mid_fp`
(itself derived from storage bytes and reserves) — never `input_amount`. `mid_fp` is computed
once per call from state, then the entire quoted curve (raw quadratic or tail) is a
fixed-parameter function of the trade size alone. `step` is not read anywhere in
`compute_swap` (it is only available in the `after_swap` payload, offset 34, and `after_swap`
does not need it).

## Cold start and garbage-state handling (findings #4, #5)

- **Cold start.** Storage is zero-initialised at the start of every simulation. A
  zero-initialised `storage[OFF_MAGIC..OFF_MAGIC+8]` reads as `0`, which never equals `MAGIC`
  (`0x4845_4447_4550_4E4C`), so `mid_from_state` falls back to `reserve_mid(rx, ry)` — the
  reserve ratio, the only fair-price proxy available before any trade has fired `after_swap`.
- **Garbage state.** `crates/cli/src/commands/validate.rs`'s randomized probe fills
  `storage[0..32]` (including the 8-byte magic) with pseudo-random bytes. The magic check is
  the sanitiser: an 8-byte random value has a `2^-64` chance of colliding with `MAGIC`, so
  every randomized-probe iteration falls back to `reserve_mid` without ever reading the
  `mid_fp` field as an arbitrary `u128` — confirmed by `prop-amm validate`'s randomized
  reserve/storage check passing (§ Parity gate below). Unlike a strategy that also reads
  `var_sum`/`count`-style fields from storage, this design does not even attempt to read
  anything beyond the magic check when it fails — there is nothing else to sanitise.
- **Sane-band clamp.** Every value used as `mid_fp` — freshly derived or loaded from
  (recognised) storage — passes through `clamp_mid`, bounding it to `[1_000, 1e15]` (a real
  price floor of ~1e-6 and ceiling of 1e6 Y per X). This both defends against a hypothetical
  future corruption of a magic-tagged state and bounds how far `mid` can drift over 10,000
  steps of `after_swap` updates (see § Frozen parameter space's stability argument).

## Every error path saturates, never returns a spurious 0 (finding #6)

Both `buy_output` and `sell_output` end in `raw.min(reserve_cap_x/y) as u64` — the output is
**structurally bounded by the reserve cap** regardless of which branch (raw or tail) produced
`raw`, so it can never exceed the live reserve and trip `crates/sim/src/amm.rs`'s
"quote > reserve -> 0.0" guard. For a huge input (the arbitrageur brackets toward
`MAX_INPUT_AMOUNT ~= 1.8e19` nano), both sides are already in `saturating_tail`'s branch by
construction (see § Saturating tail below for why the switch point is always far below any
input that large), which asymptotes toward — but never reaches — `reserve_cap`, so the output
saturates toward the reserve, never collapses to `0`.

## Saturating tail (findings #1, #6, #7)

Both `cost_buy`'s inversion and the direct sell-side quadratic are only used up to a switch
point chosen from the *current* reserves and mid price (not a fixed constant), because a fixed
switch point could be arbitrarily wrong relative to whatever reserve state a given call
happens to see:

- **Buy side.** The inverted cost function has no vertex (it's monotone increasing for all
  `y >= 0` — a cost function, not a payoff), so the only reason to leave it is the reserve
  cap. Switch value `v0 = reserve_cap_x / 2`; the corresponding input threshold `y0` is
  computed **forward** (`cost_buy(v0, ...)`, no inversion needed just to find the switch).
- **Sell side.** The raw quadratic `sell_fp*x/SCALE - k*x^2/(2*SCALE^2)` has a real vertex at
  `x = sell_fp*SCALE/k` (this is exactly the failure the review amendment identifies — see
  § Fidelity self-assessment above). Switch input
  `x0 = min(vertex/2, (reserve_cap_y*SCALE)/(2*sell_fp))`: the first term keeps `x0` on the
  strictly-increasing half of the parabola; the second guarantees
  `cost_sell(x0) <= reserve_cap_y/2` **regardless of `k`**, using the bound
  `sell_fp*x/SCALE - k*x^2/(2*SCALE^2) <= sell_fp*x/SCALE` (the quadratic term is never
  negative-of-negative). Neither bound depends on the other holding, so `x0` is always safe.

Past the switch, `saturating_tail` fits `reserve_cap - D/((v - switch) + C)`, choosing `C`/`D`
so the tail is **both value- and slope-continuous** with the raw curve at the switch point (not
just value-matched — matching the review's own weaker "kinks downward" bar with a strictly
stronger guarantee costs one extra, cheap division). The tail is strictly increasing and
concave for every `v` past the switch and asymptotes to `reserve_cap` without ever reaching it,
by construction (`C, D > 0` always, given `reserve_cap > value_at_switch`, which both switch
constructions above guarantee).

Verified empirically, not just algebraically: `prop-amm validate`'s strict monotonicity check
(sizes 0.1..200 real tokens, single fixed state) and `bench fuzz`'s 324-state x 2-side sweep
(dense linear/geometric/clustered grids plus golden-section fair-price sample sets, every
`[grid]` regime corner, zeroed- and random-byte-storage variants, states reached only after a
full-length GBM drift) both **pass with zero violations** on the first attempt — no iteration
was needed once this construction was implemented.

## Clamping before squaring (finding #7)

- `cost_buy`/`invert_buy` only ever square `x <= v0` (the buy switch value) or evaluate
  `buy_fp^2 + 2*K_SCALED*y` for `y <= y0` — both bounded by the current reserve, never by an
  arbitrary caller-supplied input. `buy_fp` itself is bounded by the sane-band clamp
  (`<= 1.02e15` after the spread multiplier), so `buy_fp^2` never approaches `u128::MAX`.
- The sell side never squares `x` beyond `x0`, which is bounded by construction (see above) to
  keep `k*x0^2` well inside `u128`.
- The buy-side inversion uses the stable rationalised form
  `x = 2*SCALE*y / (sqrt(buy_fp^2 + 2*K_SCALED*y) + buy_fp)` throughout — the naive
  `(sqrt(...) - buy_fp)/K_SCALED` form is never used anywhere in this file.
- Every multiplication that could plausibly overflow for an out-of-range reserve or input is
  `saturating_mul`, and every subtraction that could plausibly underflow is `saturating_sub` —
  belt-and-suspenders given the switch-point constructions above already keep every squared
  quantity inside a safe range for any realistic reserve.

## Pre-search shape-fuzz gate (docs/DESIGN.md §2.9, WHI-1212)

`bench fuzz --strategy strategies/006-hedged-pnl`: **PASS — zero shape violations** across 324
states x 2 sides (dense sweeps + golden-section fair-price sample sets, every `[grid]` regime
corner in both a zeroed- and random-byte-storage variant, plus states reached only after a
full-length GBM drift). No report committed (a PASS writes none, by design). Re-run after
committing the fitted point below — same result.

## Frozen parameter space (declared before any search runs — docs/DESIGN.md §2.4)

**Search 2 dimensions**, per the issue's own review amendment (which supersedes the issue's
original four-coefficient implementation steps) — the 4D form is **not searched at all**,
recorded here as deliberately excluded rather than merely out of budget:

### Why 4D is excluded, not just unsearched

Per executed sell of `x`, the spread changes by `(k++ - k-+) * x`; per executed buy, by
`(k-- - k+-) * x`. Both terms are signed by the **coefficient difference**, not by the trade
direction, so with any asymmetric choice the spread drifts monotonically with total *unsigned*
volume across a 10,000-step simulation carrying thousands of trades — either widening until no
flow routes to this pool, or crossing the quotes and being round-tripped by the arbitrageur
every step. The only stationary manifold is `k-+ = k++` and `k+- = k--` (both prices move in
lockstep per trade side); GBM's zero drift plus 50/50 retail symmetry then forces
`k_sell = k_buy = k`, collapsing the state to one mid plus a fixed half-spread. This is not a
budget-driven simplification — a 4D point outside this manifold is not a valid design, so
there is nothing 4D left to search.

### Searched

| Param | Range | Reason |
| --- | --- | --- |
| `K_SCALED` (impact slope `k`, Y/X per X, at `SCALE=1e9` fixed point) | `250000000..=32000000000` (k = 0.25..=32) | Brackets the CPMM's own marginal-price slope at the default state (`2p/x = 2.0` at `(100, 10000)`) by 8x each way; below 0.25 the book is deeper than 8x reserves and the tail dominates every quote; above 32 the vertex/switch sits inside routine retail sizes (confirmed below: `k=8` already shows the curve degrading sharply). |
| `DELTA_BPS` (half-spread, bps **of mid**, not absolute) | `5..=200` | 5 undercuts the normalizer's minimum fee (30bps) to probe pure flow-stealing; 200 is roughly 3x `001`'s fitted 66bps with headroom. bps-of-mid rather than an absolute spread because GBM wanders roughly 2x over 10,000 steps at `sigma=0.007` — an absolute spread would be broken by construction at that scale. |

### Frozen, recorded as deliberately un-searched

`MIN_MID_FP = 1_000` / `MAX_MID_FP = 1e15` (the sane-band clamp — a safety bound, not a curve
parameter; searching it would not be measuring the mechanism); `SWITCH_FRACTION_NUM/DEN = 1/2`
and `RESERVE_CAP_NUM/DEN = 999/1000` (the saturating-tail switch point and reserve headroom —
structural constants of the safety patch in § Saturating tail, not part of the doc's own
mechanism, so not something a "how faithful is the port" search should tune).

## Step 0 — pre-registered bounded probe (docs/DESIGN.md §2.5/§2.9, issue's own review
amendment)

All three sub-steps run on the 200 screening seeds with common random numbers, frozen in
advance of any search, per the issue's own "Step 0" amendment:

1. **Shape gate.** PASS — see § Pre-search shape-fuzz gate and `prop-amm validate` (§ Parity
   gate below) above. *Predicted: passes. Confirmed.*
2. **Anchor point** `(k=2.0, delta_bps=66)` — screening avg edge **350.658135**. The issue's
   stop rule was "must reproduce `001`'s screening number of ~384.82; if it lands far off (say
   < 340), diagnose before any search runs." `350.66` is **not** far off by that threshold
   (`> 340`), but it is not a close reproduction either — expected and explained, not a bug:
   unlike `005`'s nested-CPMM-clone demonstration, this family does **not** algebraically
   contain `001`'s curve at any parameter setting (a quadratic-impact market maker is a
   structurally different mechanism from a fee-discounted constant product, not a
   generalisation of it), so `(k=2.0, delta=66)` is only an economically comparable point, not
   an exact clone. `350.66` at a plausible middle-of-range `k` being in the same
   hundred-edge-unit neighbourhood as `001`'s optimum is the actual signal here — it says the
   state handling and sign mapping produce sane numbers, which is what this step exists to
   check, not that the two mechanisms are identical.
3. **Axis liveness (2 evals).** `(k=0.5, 66)` -> screening avg edge **230.074985**; `(k=8, 66)`
   -> screening avg edge **186.098836**. Both deviate from the anchor by more than 100 edge
   units — an order of magnitude beyond any paired-CI noise at 200 seeds. The stop rule
   ("negative result iff neither deviates from the anchor beyond noise") does not fire: the
   depth axis clearly and substantially moves the number in both directions. **Proceed to the
   full search.**

## Search (docs/DESIGN.md §2.5)

Run via `cargo run -p prop-amm-bench --release -- fit --strategy strategies/006-hedged-pnl`
(`config/bench.toml`'s `[search]` budget, 300 points; screening segment
`1_000_000..=1_000_199`, common random numbers). Coarse grid (2-dimensional, 12 points/axis,
144 points) then coordinate descent. Full curve committed at
`results/2026-08-21-fit-006-hedged-pnl.md`.

**Converged after 226 of 300 points** (budget never exhausted); **zero invalid points** —
every evaluated parameter vector produced a valid edge, consistent with the pre-search fuzz
gate having already cleared this family across every regime corner.

**Winning point: `K_SCALED = 1639729121` (k ≈ 1.64), `DELTA_BPS = 67`.**

| Segment | n | Avg edge |
| --- | --- | --- |
| screening (search inner loop) | 200 | 354.858203 |
| train (final evaluation) | 1,000 | 384.781858 |
| validation (final evaluation) | 1,000 | 379.135831 |

Against `001-cpmm-fee`'s own committed numbers (screening 384.82, train 406.14, validation
401.80): **-29.96 screening, -21.36 train, -22.66 validation** — a consistent shortfall across
all three independently-sampled segments. Per `docs/DESIGN.md` §2.8, **this is a negative
result, not a mid-table entry**: `006` does not beat the 0-line. This matches the issue's own
review-amendment prediction almost exactly ("385-410 — most likely at or slightly below
401.80. I predict it does not beat the 0-line"), landing just below the bottom of that
predicted range on validation (379.14 vs. the predicted floor of 385).

The winning `k=1.64` sits comfortably inside the interior of its `0.25..=32` range (not at
either boundary), and `delta=67` sits almost exactly on `001`'s own fitted `66` — the search
converged to "roughly `001`'s own spread, with a modest impact slope layered on top", which is
consistent with the grid-mode finding below (the impact mechanism helps at low volatility and
actively hurts at high volatility, so the fitted point is a compromise between those regimes,
not a clear win in either).

**Compile timing:** 228 warm compiles, min=0.229s, mean=0.752s, max=3.489s during this run —
exceeds `docs/DESIGN.md` §2.6's `<1s` target on the mean and max samples.
`strategies/001-cpmm-fee/NOTES.md` (WHI-1205) already ruled out every structural cause of a
fast-path compile-time gap on this machine; this run's elevated variance is consistent with
genuine background load rather than a regression — a second bench session (`WHI-1207`) was
running concurrently against a different strategy in a separate worktree throughout this
search (confirmed via `ps aux` at the time), which would directly explain both the mean sitting
well above WHI-1205's own re-measurement and the 3.489s outlier. Not re-investigated further
here for the same reason `005`'s NOTES.md gives: a single run under known contention cannot
establish a regression independent of that load.

## Grid mode: 27-cell fragility matrix (docs/DESIGN.md §2.3)

`cargo run -p prop-amm-bench --release -- grid --candidate strategies/006-hedged-pnl/lib.rs
--reference strategies/001-cpmm-fee/lib.rs` — reference is `001-cpmm-fee` (the 0-line every
M1 candidate is measured against, `docs/DESIGN.md` §2.8), not `000-normalizer`. Full table
committed at `results/2026-08-21-grid-006-hedged-pnl.md`.

| cell | fee (bps) | liq mult | sigma | candidate | reference | mean diff |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | 30 | 0.4 | 0.0001 | 724.50 | 710.96 | +13.54 |
| 1 | 30 | 0.4 | 0.0010 | 708.57 | 695.34 | +13.23 |
| 2 | 30 | 0.4 | 0.0070 | 72.95 | 197.26 | **-124.31** |
| 3 | 30 | 1.0 | 0.0001 | 408.93 | 389.11 | +19.82 |
| 4 | 30 | 1.0 | 0.0010 | 403.10 | 382.63 | +20.47 |
| 5 | 30 | 1.0 | 0.0070 | -280.34 | -128.96 | **-151.38** |
| 6 | 30 | 2.0 | 0.0001 | 216.95 | 205.57 | +11.38 |
| 7 | 30 | 2.0 | 0.0010 | 202.66 | 192.54 | +10.11 |
| 8 | 30 | 2.0 | 0.0070 | -459.60 | -288.26 | **-171.34** |
| 9 | 55 | 0.4 | 0.0001 | 859.67 | 842.83 | +16.84 |
| 10 | 55 | 0.4 | 0.0010 | 851.83 | 836.89 | +14.94 |
| 11 | 55 | 0.4 | 0.0070 | 66.38 | 278.70 | **-212.32** |
| 12 | 55 | 1.0 | 0.0001 | 569.01 | 547.15 | +21.86 |
| 13 | 55 | 1.0 | 0.0010 | 578.22 | 546.96 | +31.26 |
| 14 | 55 | 1.0 | 0.0070 | -171.23 | 32.09 | **-203.32** |
| 15 | 55 | 2.0 | 0.0001 | 356.25 | 348.19 | +8.06 |
| 16 | 55 | 2.0 | 0.0010 | 348.26 | 341.02 | +7.24 |
| 17 | 55 | 2.0 | 0.0070 | -368.41 | -159.57 | **-208.83** |
| 18 | 80 | 0.4 | 0.0001 | 1081.38 | 1030.48 | +50.89 |
| 19 | 80 | 0.4 | 0.0010 | 1074.60 | 1018.67 | +55.94 |
| 20 | 80 | 0.4 | 0.0070 | 372.14 | 483.73 | **-111.59** |
| 21 | 80 | 1.0 | 0.0001 | 895.01 | 838.09 | +56.91 |
| 22 | 80 | 1.0 | 0.0010 | 898.71 | 838.72 | +59.99 |
| 23 | 80 | 1.0 | 0.0070 | 121.91 | 315.55 | **-193.65** |
| 24 | 80 | 2.0 | 0.0001 | 709.30 | 668.09 | +41.22 |
| 25 | 80 | 2.0 | 0.0010 | 713.51 | 665.90 | +47.61 |
| 26 | 80 | 2.0 | 0.0070 | -38.66 | 152.43 | **-191.09** |

18 of 27 cells favor `006`, all 9 negative cells (bold) favor `001`. Full 95% CIs in
`results/2026-08-21-grid-006-hedged-pnl.md` — every listed cell's CI excludes 0 (the narrowest
margins are cells 6/7/15/16, all still comfortably significant), so every sign above is a real
effect, not noise.

### Explaining the negative cells — a perfectly clean split

**Every one of the 9 negative cells has `sigma = 0.0070` (the grid's high-volatility level),
and every one of the 18 positive cells has `sigma <= 0.0010`.** This is not a fuzzy tendency —
it is an exact partition of all 27 cells by one axis alone, independent of `norm_fee_bps` or
`norm_liquidity_mult`. This is exactly the mechanism's own designed tradeoff, and exactly what
the issue's own prediction called out before this grid ran: `k` is an **absolute** slope, while
the fair price moves multiplicatively (GBM). At low/mid sigma, `mid` (updated only by
`k * x_executed` per executed trade) tracks a slowly-moving fair price closely, so the impact
slope earns a genuine edge on top of the ~67bps half-spread (the low/mid-sigma wins are
directly proportional to how much extra the impact term captures over a flat spread — compare
cell 21/22 at `fee=80` where the wins are largest, +56.91/+59.99, against `fee=30` at cells
0/1, the smallest wins). At `sigma=0.007`, the fair price moves far faster than the
trade-driven `mid` update can track, so the arbitrageur repeatedly catches this pool
mispriced and extracts LVR on top of what `001`'s flat fee already concedes at that volatility
— and the size of the loss scales with how deep the opponent's own liquidity is (cell 8's
`liq=2.0` loss of -171.34 versus cell 2's `liq=0.4` loss of -124.31 at the same fee/sigma: a
deeper opponent both takes more flow away at the moment this pool is mispriced *and* gives the
arbitrageur a bigger corrective trade to extract from this pool once it does route here).

This single-axis split is also the direct, mechanistic explanation for why the fitted point
loses in aggregate (§ Search above): the screening/train/validation segments sample
`gbm_sigma` uniformly across the full `[0.0001, 0.007]` range every simulation, so the fitted
point's real average edge is a blend of the 18 positive-cell-like regimes and the 9
negative-cell-like regimes, and the negative cells here are **an order of magnitude larger in
magnitude** than the positive ones (-124 to -212 versus +7 to +60) — a fixed-slope impact
mechanism's high-volatility losses dominate its low-volatility gains once averaged over the
full sampled distribution, exactly the "arb bleed ~ sigma^2/k" cost the issue's own prediction
named. None of the 9 negative cells needed a separate explanation beyond "sigma is high" — a
notably cleaner fragility pattern than `005`'s own grid (which needed two distinct explanations
for its own negative cells).

## Estimator note: `mid` is not a volatility estimate (informational)

Unlike `005`, `006` carries no volatility estimator at all — `mid` tracks the *level* of the
fair price via trade-driven updates, not its *variance*. The grid-mode split above shows this
design would very plausibly benefit from `k` (or `delta_bps`) scaling with a volatility signal
the way `005` does with its fee — but the issue's own frozen space (§ Frozen parameter space
above) fixes `k` and `delta_bps` as flat constants; adding a volatility-adaptive impact slope
would be new mechanism, not this port's minimum-adaptation set, so it is out of scope here
(§2.9: a `006b` variant, not part of this faithful-minimum port).

## Compute units (docs/DESIGN.md §2.9, cross-cutting finding #9)

No bench tooling exposes measured CU headroom today — the same gap `005-vol-adaptive-cpmm-fee/NOTES.md`
records, applying to every M1 strategy, not uniquely this one.

Division-count estimate (finding #9 names division count, not instruction count, as the thing
to bound on SBF): the common case (raw branch, both sides) does, per `compute_swap` call:
`mid_from_state`/`reserve_mid` (0-1 divisions depending on whether the magic check hits),
`buy_output`'s setup (`reserve_cap_x`, `v0`, `buy_fp` — 3 divisions) plus `cost_buy` for the
`y0` threshold (2 divisions) plus `invert_buy` (`isqrt`'s Newton loop — 2 divisions/iteration,
converging in well under 16 iterations given the bounded input this is ever called with, per
§ Clamping before squaring — plus 1 final division), roughly 6 setup divisions plus the
`isqrt` loop; the sell side is structurally identical minus the inversion (no `isqrt` needed at
all in its raw branch, since it computes output directly). The tail branch (rare — only
engaged for inputs beyond half the reserve cap) adds 2 more divisions
(`saturating_tail`'s `c` and the final `reserve_cap - d/w`). This is the same order of
magnitude as `005-vol-adaptive-cpmm-fee`'s own already-passing ~25-division worst-case
estimate (that family also runs an `isqrt` loop), so at finding #9's own cited cost
(10^2-10^3 CU/division on SBF), this is comfortably inside the 100,000 CU limit.
Corroborated empirically: `prop-amm validate`'s native/BPF parity check and `bench parity`
below both execute the real BPF program repeatedly with no compute-budget failure.

## Parity gate (docs/DESIGN.md §2.6)

Reproduced via `cargo run -p prop-amm-bench --release -- parity --strategy
strategies/006-hedged-pnl` against the committed `(K_SCALED=1639729121, DELTA_BPS=67)` point:
`prop-amm validate` passes; the fast path and the reference path agree on **all 1,000
`observation`-segment seeds to `0` relative difference** (well inside the `1e-9` gate); the
fast-path aggregate (avg edge 369.58) matches `prop-amm run`'s own 2-decimal output exactly
(369.58, total 369582.74). See `results/2026-08-21-parity-006-hedged-pnl.md` for the committed
snapshot.

**Leaderboard-comparable number: avg edge 369.58** (`observation` segment, seeds `0..=999`,
native), against `001-cpmm-fee`'s own **399.97** on the same segment — a **-30.39 (-7.6%)**
shortfall, consistent with the negative result recorded in § Search above.

## Summary

`006-hedged-pnl` is a **fitted, negative result** (`docs/DESIGN.md` §2.8) — not `wontfix`
(the shape gate passes cleanly after the review amendment's prescribed adaptations), not a
win. The mechanism's core hypothesis (a linear-in-flow impact slope, decoupled from reserves,
as a genuinely new axis versus every fee-CPMM in the frozen list) is real and measurable: grid
mode shows a clean, fully-explained split where it wins at low/mid volatility and loses badly
at high volatility, because the slope is an absolute quantity while the fair price moves
multiplicatively. Averaged over the full sampled volatility distribution, the high-volatility
losses dominate, and the fitted point ends up a compromise that trails `001-cpmm-fee`'s flat
fee by roughly 6-8% across every independently-sampled segment. This confirms, rather than
merely repeats, the issue's own prediction — the value of running it was exactly this
confirmation and the clean per-regime attribution, not the aggregate number itself.
