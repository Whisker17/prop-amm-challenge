# 007b-grounded-k-anchor

## Provenance

A `docs/DESIGN.md` §2.9 variant of `strategies/007-dodo-pmm` (WHI-1272 / parent WHI-1219),
not a fresh external port — no new `docs/references/` directory; the parent's own
`docs/references/007-dodo-pmm/` remains the mechanism source, same pattern as `003b`. Same
PMM quote, same `R = ONE` collapse, same fee-on-output. The only mechanism change is the
k<1 (now: all-k) `after_swap` anchor update.

**This directory has no terminal / ranked state.** Constants stay at the parent's
`(K_BPS=10_000, FEE_BPS=66)` so this issue's only new number is “the k<1 path no longer
runs away.” Fitting is WHI-1273, the only issue that may commit a ranked point. If
WHI-1273 is abandoned, `007b` is marked Canceled per §2.10's removal clause, not left as a
pinned `(10000, 66)`.

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
     monotone-safe; return `min(fair, v-1)` instead.

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

Committed in this directory, **not as a ranked point:** `(K_BPS=10_000, FEE_BPS=66)`.
The frozen space is unchanged; this issue does not spend search budget.

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

**`K_BPS=5000/7500` 2-nano concavity is the parent quote path, not this `after_swap`.**
Reproduced on an unmodified copy of `strategies/007-dodo-pmm/lib.rs` at the same two
points: `FAIL: Concavity violation (buy side). At size=100, step2=9836 > step1=9834`
(`K_BPS=5000`) and `At size=50, step2=9861 > step1=9859` (`K_BPS=7500`). Parent Step 0.5
never validated these two k values (its set was `{25, 400, 2500, 10_000}`). `bench fuzz`
(§2.9's 4-nano gate) PASSes both. WHI-1273 searching these k values inherits this
quote-path 1-nano miss; it is not a recursive-anchor artifact.

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
(factor >100, edge −12,402). This variant stays at factor 1 by construction: each
`after_swap` write is the current reserve ratio, never `i_old * R_f`.

## Measurement provenance

- Build profile: `target/release/` — binaries
  `/tmp/prop-amm-whi-1272-meas/target/release/prop-amm` and
  `/tmp/prop-amm-whi-1272-meas/target/release/bench`.
- Detached checkout: `/tmp/prop-amm-whi-1272-meas`, synced to this issue's `007b` sources
  (worktree `.claude/worktrees/whi-1272`).
- No `results/` ranking snapshot was written by this issue (no `bench fit` / `grid` /
  `compare`).
