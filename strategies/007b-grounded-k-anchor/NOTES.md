# 007b-grounded-k-anchor

## Provenance

A `docs/DESIGN.md` §2.9 variant of `strategies/007-dodo-pmm` (WHI-1272 / parent WHI-1219),
not a fresh external port — no new `docs/references/` directory; the parent's own
`docs/references/007-dodo-pmm/` remains the mechanism source, same pattern as `003b`. Same
PMM quote, same `R = ONE` collapse, same fee-on-output. The only mechanism change is the
k<1 (now: all-k) `after_swap` anchor update.

**Ranked point (WHI-1273):** `(K_BPS=10_000, FEE_BPS=66)` — the 300-point §2.5 joint
search's own winner. `K_BPS` is a frozen-range upper-bound hit; `FEE_BPS=66` is the
search's fitted fee, not a copy of `001@66` (it happens to equal that value). Step-4
reading 2 applied: the search collapsed to `k=1`, so concentration is a jointly-fitted
negative. v1's `007` snapshots are untouched.

## Objective (WHI-1272's own framing)

The parent's committed point is `K_BPS=10_000` (`k=1`). At that point `after_swap` already
writes `raw_ratio_price(rx_post, ry_post)` and is self-correcting. Every other value in
the frozen range `25..=9_999` still ran `post_trade_mid_divide` / `post_trade_mid_multiply`
(`i_new = i_old * R_f` or `/ R_f`). Parent NOTES.md § A runaway anchor, found and fixed
records that this form has no mechanism to self-correct: at `k=1` it drifted the anchor to
<1% of the true ratio on seed `1_000_231` and produced a −12,402 edge against `001`'s
−381. The same NOTES.md explicitly warns that a `007b` revisiting interior `K_BPS` “would
need to fix the recursive anchor generally, not just re-read this file's k=1 special case
as already handled.”

Without this plumbing, WHI-1273's joint fit of interior `K_BPS` would measure a known-buggy
recursive `after_swap`, not the PMM.

## Pinned mechanism (one family, not two)

`after_swap` recomputes the post-trade mid from **current reserves only**, for **all**
`K_BPS`: write `raw_ratio_price(rx_post, ry_post)` unconditionally. The recursive
`i_old * R_f` helpers (`post_trade_mid_divide` / `post_trade_mid_multiply` /
`rf_denominator_scaled`) are **deleted, not conditionalised**. Periodic (every-N-trades) or
EWMA re-grounding is forbidden — `N` / the weight would be an invented searched parameter;
the parent already froze the anchor rule as “every executed trade.”

At `K_BPS >= K_DEN` this is the same `wr16` of clamped `raw_ratio_price` the parent
special-cased, so the k=1 storage write is bit-identical. Confirmed empirically: seed
`1_000_231` at `(10000, 66)` is **−378.69** for both `007` and `007b`.

## Fidelity self-assessment (docs/DESIGN.md §2.9)

**Minimum changes only**, relative to the parent, except the one pinned mechanism change
and the interior-k numerical guards it forced:

- Quote path at `k=1` (`fair*V/(V+fair)`, fee-on-output, `R = ONE`, overflow clamps,
  `scaled_isqrt`) is unchanged. Seed `1_000_231` matches the parent bit-for-bit on edge.
- Storage layout, MAGIC, cold-start / garbage-state fallback (`anchor_price` → live
  reserve ratio when MAGIC misses) are unchanged.
- **Mechanism change (itemised, not hidden):** k<1 `after_swap` no longer writes the PMM's
  own post-trade marginal (`i_old * R_f`). It writes the raw reserve ratio for every
  `K_BPS`. Relative to “PMM mid ≠ reserve ratio at `k<1`”: **k becomes a pure
  depth-profile knob around a CPMM-like mid.** This is a deliberate one-family pin so
  WHI-1273 searches one space, not two. It is not a byte-for-byte preservation of the
  source's `getMidPrice` at `k<1`; the parent already treated `after_swap` as new code
  (no external oracle in this harness), and this variant generalises the k=1 numerical
  fix the parent itself recorded as required for any interior-k revisit.
- **Numerical guards, k<1 only (not a second family):**
  1. *Dust fallback.* At `v < 1_000_000` nano (0.001 tokens) the k<1 quadratic's `b_sig`
     flip 0-holes then dips below even the k=1 fill (reproduced: seed 9008, `rx=8291`,
     `K_BPS=25`, `35220000 → 0` after `35210000 → 8235`). Fall back to the k=1 closed
     form, which is monotone for any `v`. Unreachable at the committed k=1 point.
  2. *Multiply-first `disc_term`* when `v^2 * 4k(1-k)` fits u128, so the k-term does not
     floor to 0 at dust `v` (`v^2 / K_DEN^2 == 0`). Divide-first remains the overflow
     fallback at `RESERVE_CLAMP`.
  3. *Empty-pool instead of `V2>V1 → 0`.* Returning 0 after a near-full fill is not
     monotone-safe; return `min(fair, v-1)` **only on that out-of-range branch**. The
     in-range return is still `v - v_out` (parent). No k=0-style `min(fair, v)` cap on
     the normal path (that flattening is what makes `K_BPS=0` fail `validate`).

## Shape-safety / cold start / error paths

Inherited from the parent (findings #3–#7, #9), plus the guards above. `compute_swap`
still never reads `input_amount` for anything but `delta`. `after_swap` now never reads
the previous anchor — each write depends only on the current (clamped) post-trade
reserves — so the runaway-anchor failure mode cannot accumulate.

## Parameter space — unchanged from the parent

| Param | Range | Reason |
| --- | --- | --- |
| `K_BPS` | **25..=10_000** | Parent's frozen range. `k=0` excluded as shape-fatal (source's own `k=0` branch is a hard flat cap that fails `validate`'s strict monotonicity). |
| `FEE_BPS` | **1..=500** | Same as `001-cpmm-fee` / the parent. |

Committed as the ranked point (WHI-1273): `(K_BPS=10_000, FEE_BPS=66)`. Frozen space
unchanged from the parent. `K_BPS=10_000` is a **boundary hit** (WHI-1209 presentation);
`FEE_BPS=66` is whatever the search fitted, not `001`'s fee held fixed.

## Pre-search shape-fuzz gate (docs/DESIGN.md §2.9)

`prop-amm validate` and `bench fuzz --strategy strategies/007b-grounded-k-anchor` at
`K_BPS ∈ {25, 100, 400, 1000, 2500, 5000, 7500, 10_000}` × `FEE_BPS=66`. A PASS writes
no `results/` report (§2.9). Release profile; detached checkout
`/tmp/prop-amm-whi-1272-meas` (Trap 4 / WHI-1247). Binaries
`/tmp/prop-amm-whi-1272-meas/target/release/prop-amm` and
`/tmp/prop-amm-whi-1272-meas/target/release/bench`.

| `K_BPS` | `prop-amm validate` | `bench fuzz` (324 states x 2 sides) |
| --- | --- | --- |
| 25 | PASS (incl. native/BPF parity) | PASS — zero shape violations |
| 100 | PASS (incl. native/BPF parity) | PASS — zero shape violations |
| 400 | PASS (incl. native/BPF parity) | PASS — zero shape violations |
| 1,000 | PASS (incl. native/BPF parity) | PASS — zero shape violations |
| 2,500 | PASS (incl. native/BPF parity) | PASS — zero shape violations |
| 5,000 | FAIL — concavity 2-nano (see below) | PASS — zero shape violations |
| 7,500 | FAIL — concavity 2-nano (see below) | PASS — zero shape violations |
| 10,000 | PASS (incl. native/BPF parity) | PASS — zero shape violations |

**`K_BPS=5000/7500` 2-nano concavity is inherited from the parent quote path.**
Reproduced on an unmodified copy of `strategies/007-dodo-pmm/lib.rs` at the same two
points: `FAIL: Concavity violation (buy side). At size=100, step2=9836 > step1=9834`
(`K_BPS=5000`) and `At size=50, step2=9861 > step1=9859` (`K_BPS=7500`). Parent Step 0.5
never validated these two k values (its set was `{25, 400, 2500, 10_000}`). `bench fuzz`
(§2.9's 4-nano gate) PASSes both. 007b's `disc_term` is multiply-first when it fits, so
the rounding is not byte-identical to the parent, but the miss is the same 2-nano
isqrt/formula class the parent already recorded at `K_BPS=2500` before `scaled_isqrt`.
Logged in `docs/DEFERRED_ISSUES.md` (WHI-1272) rather than treated as an `after_swap`
defect. WHI-1273 searching these k values inherits this 2-nano miss against
`validate`'s 1-nano step tolerance.

### Expected `K_BPS = 0` monotonicity FAIL

`prop-amm validate` at `K_BPS = 0`:

```
Error: FAIL: Monotonicity violation (sell side). size=200 output=9934000000000 <= prev_output=9934000000000
```

The source's own `k=0` branch is a hard flat cap (`output = min(i*delta, V1)`), which
fails `validate`'s strict monotonicity at its fixed probe sizes. Recorded as the expected
FAIL, not as a defect of this variant. Not a fuzz PASS requirement, not a scored edge.

## Runaway diagnostic

Throwaway instrumentation (not committed): at every `compute_swap`, compare stored
anchor vs `raw_ratio_price(rx, ry)`. 10_000-step native `prop-amm run`, release profile,
seed `1_000_231` (the parent's high-sigma / low-liq collapse seed).

| Check | Seed | `K_BPS` | Result |
| --- | --- | --- | --- |
| Interior k | 1,000,231 | 2500 | max factor **1.000** (0 mismatches / ~400k quotes). Edge −20,235.34 (economic, not a runaway). |
| Mild k | 1,000,231 | 7500 | max factor **1.000** (0 mismatches / ~410k quotes). Edge −530.80. |
| k=1 regression vs parent | 1,000,231 | 10_000 | max factor **1.000**; edge **−378.69** on both `007` and `007b` (parent post-fix was −378.69 vs `001`'s −381.55). |

The parent's pre-fix collapse on this seed at `k=1` was stored <1% of the true ratio
(factor >100, edge −12,402). Factor 1 rules that signature out: each `after_swap`
write is the current reserve ratio, never `i_old * R_f`. It does **not** by itself
prove the `K_BPS=2500` −20,235 edge is "not a runaway of some other kind" — it only
proves the stored mid tracks the reserve ratio. The economic reading (adverse
selection at low k, parent Step 0.5 #4) is unchanged and is WHI-1273's to re-measure.

## WHI-1273 — joint fit of `(K_BPS, FEE_BPS)`

Release profile; detached checkout `/tmp/prop-amm-whi-1273-meas` at `231dd58` (Trap 4 /
WHI-1247). Binaries `target/release/bench` and `target/release/prop-amm`. Reports generated
from that clean sha (no `+dirty`). `test` segment was not read. `research-out/` was not
cited.

### Step 1 — containment (not the ranking snapshot)

Degenerate-range copy pinning `K_BPS 10000..=10000`, `FEE_BPS 66..=66`. Genuine `bench fit`
(report on) plus `bench parity --segment observation`.

| | 007b `(10000, 66)` | parent 007 |
| --- | ---: | ---: |
| screening (n=200) | 386.052462 | 386.052462 |
| train (n=1,000) | 407.545843 | 407.545843 |
| validation (n=1,000) | 403.257399 | 403.257399 |
| observation (n=1,000) | 401.28 | 401.28 |

Parity: 1000/1000 observation seeds agree to `0` relative difference; fast-path avg 401.28
matches `prop-amm run` avg 401.28, total edge 401284.20.
`results/2026-08-25-fit-007b-k1-containment.md`,
`results/2026-08-25-parity-007b-k1-containment.md`.
Mismatch would have blocked the search; it did not.

`prop-amm validate` 2-nano concavity at `K_BPS=5000`/`7500` re-derived on unmodified parent
007 and on 007b (identical strings). Not a 007b regression. Search winner did not land
there, so the quote-path rewrite stays deferred (`docs/DEFERRED_ISSUES.md`).

### Step 2 — 300-point search (unconditional)

`bench fit --strategy strategies/007b-grounded-k-anchor` (no `--max-points` / `--no-report`).
Coarse-grid-then-coordinate-descent, screening seeds `1_000_000..=1_000_199`.

- Cap: 300. **Spent: 170/300.** Invalid: 0. Stopped: converged.
- Winner: `K_BPS=10000, FEE_BPS=66`. Screening 386.052462; train 407.545843; validation
  403.257399.
- Ranking snapshot: `results/2026-08-25-fit-007b-grounded-k-anchor.md`.

The search ran to completion before either step-4 reading was applied.

### Bands actually evaluated (grid or descent)

Linear `K_BPS` grid nodes (12-point linspace 25..=10000): 25, 932, 1839, 2745, 3652, 4559,
5466, 6373, 7280, 8186, 9093, 10000. Descent then walked `K_BPS` 7507, 8754, 9377, 9689,
9845, 9923, 9962, 9981, 9991, 9996, 9998, 9999 around the `k=1` basin, and `FEE_BPS` around
66.

| Band | Evaluated? | Points |
| --- | --- | --- |
| `K_BPS ∈ [25, 400]` | yes (grid) | 25 only — the linear grid undersamples small-`k` (the whole band is one node) |
| `(400, 2500]` | yes (grid) | 932, 1839 |
| `(2500, 9000]` | yes (grid + descent) | 2745, 3652, 4559, 5466, 6373, 7280, 7507, 8186, 8754 |
| `10_000` | yes (grid + descent) | 10000, plus neighbours 9999…9093 |

No band was missing. Compensatory coverage of the undersampled small-`k` band is the
k-sweep below, not invented grid points.

### Step 4 — what “best” means

**Reading 2 applied:** the search collapsed to `K_BPS=10_000`. Concentration is a
jointly-fitted negative. Commit `(10000, fitted FEE_BPS)` = `(10000, 66)`. Boundary hit
flagged. `FEE_BPS=66` is the search's own fitted fee (the k=1 fee-on-output slice peaked
at 66: screening 65→385.980622, 66→386.052462, 67→385.823268), not `001@66` copied in.

Reading 1 (interior `K_BPS` wins on screening and holds on validation) did not apply.

### Step 5 — k-sweep at `FEE_BPS* = 66` (reporting only)

`results/2026-08-25-ksweep-007b-grounded-k-anchor.md`. 12 degenerate-range genuine `bench
fit`s, plus the recorded `k=0` validate FAIL.

| `K_BPS` | `k` | screening avg edge |
| --- | --- | ---: |
| 25 | 0.0025 | −20011.90 |
| 50 | 0.005 | −20013.23 |
| 100 | 0.01 | −20012.69 |
| 200 | 0.02 | −20011.70 |
| 400 | 0.04 | −20012.47 |
| 800 | 0.08 | −20012.70 |
| 1500 | 0.15 | −20015.83 |
| 2500 | 0.25 | −20028.53 |
| 4000 | 0.40 | −19998.48 |
| 6000 | 0.60 | 133.45 |
| 8000 | 0.80 | 323.41 |
| 10000 | 1.00 | **386.05** (search winner) |

`k=0`: `FAIL: Monotonicity violation (sell side). size=200 output=9934000000000 <= prev_output=9934000000000`.

**Lesson 7 did not fire:** no sweep cell beat the search winner on screening. The table did
not silently displace `(10000, 66)`. Interior `k ≤ 0.40` is a ~−20,000 screening plateau
(the low-`k` cells wiggle by tens, they do not rise toward `k=1`); from `k=0.60` the
response then rises toward the winner. Economic, not plumbing — WHI-1272 factor 1.000 on
seed `1_000_231` at `K_BPS=2500`.

### Step 6 — grid / parity / compare on the fitted point

**Grid** vs `001-cpmm-fee` (`results/2026-08-25-grid-007b-grounded-k-anchor.md`): **24/27
cells favor `007b` with a CI excluding 0; 3 (cells 7, 16, 24) are statistical ties**; no
cell flips sign. Bit-identical to the parent's k=1 grid, as required of the grounded k=1
path.

**Parity** (`results/2026-08-25-parity-007b-grounded-k-anchor.md`): `prop-amm validate`
PASS; 1000/1000 observation seeds agree to `0` relative; fast-path avg **401.28** matches
`prop-amm run` 401.28 (total 401284.20). Leaderboard-comparable number: avg edge **401.28**
(`observation`, seeds `0..=999`) vs `001`'s 399.97.

**Compare `--segment train`** vs `001-cpmm-fee`
(`results/2026-08-25-compare-007b-grounded-k-anchor-vs-001-cpmm-fee.md`): candidate
407.55, reference 406.14, paired mean diff **1.401555**, 95% CI **[1.246297, 1.556812]**,
n=1,000 (excludes 0). Regime slices (equal-width thirds): every populated bin has a
non-negative mean diff; the three headline-CI-including-zero bins from the parent compare
(high-fee × mid/high liq × low/mid sigma) reappear with the same CIs. Pooling reproduces
the headline 1.401555.

### Consolidated segment table

| Segment | n | `007b` avg edge | `001` avg edge | diff |
| --- | --- | ---: | ---: | ---: |
| screening | 200 | 386.05 | 384.82 | +1.23 |
| train | 1,000 | 407.55 | 406.14 | +1.40 (paired 1.401555) |
| validation | 1,000 | 403.26 | 401.80 | +1.46 |
| observation | 1,000 | 401.28 | 399.97 | +1.31 |

Same numbers as v1 `007` at this point — expected, because the k=1 quote and after_swap
write are bit-identical to the parent. The new evidence is that a 170-point joint search
plus the k-sweep both failed to find a better interior `k`.

## Measurement provenance

- Build profile: `target/release/` — binaries
  `/tmp/prop-amm-whi-1273-meas/target/release/bench` and
  `/tmp/prop-amm-whi-1273-meas/target/release/prop-amm`.
- Detached checkout: `/tmp/prop-amm-whi-1273-meas` at `231dd58` (worktree
  `.claude/worktrees/whi-1273` is not where measurements ran).
- WHI-1272 plumbing measurements remain those recorded above (fuzz / runaway diagnostic)
  from `/tmp/prop-amm-whi-1272-meas`.
- Unit tests in `lib.rs` (`#[cfg(test)]`) are **not** part of `cargo test --workspace`
  (`strategies/` is not a workspace member, same as `003`/`003b`). WHI-1272: 3 passed
  against an isolated crate (`/tmp/whi-1272-tdd`).
