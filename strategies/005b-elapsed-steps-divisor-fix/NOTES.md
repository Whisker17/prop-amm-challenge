# 005b-elapsed-steps-divisor-fix

## Provenance

A `docs/DESIGN.md` §2.9 variant of `strategies/005-vol-adaptive-cpmm-fee` (WHI-1225), opened
explicitly as a **probe-gated, single-mechanism ablation, not a contender** — the issue's own
first paragraph states that `446.30` (the M1 winner) is out of reach for this family, and that
the success criterion is the measured ablation delta against `005` on common seeds, not the
ranking. The proposed change: divide the cumulative-variance estimator's `var_sum` by
**elapsed steps** (`sum(step - last_step)`) instead of **sample count**, since `after_swap`
only samples the first executed trade of a new step and the arbitrageur does not necessarily
trade every step — undercounting elapsed time and inflating the variance estimate.

**No `lib.rs` is committed for this issue.** Both of the issue's own pre-registered stop
rules ran; Probe A survived (on its literal thresholds) but Probe B's single measured point
lost by far more than the issue's own "broken plumbing" threshold, and diagnosis (below)
confirms the loss is a genuine, well-corroborated mechanism effect rather than an
implementation bug. There is no parameter point under the corrected estimator that reproduces
even a tie with the parent's fitted point (unlike `004b`'s `FLOOR_BPS=0` or `007`'s `K_BPS=
10_000`, each of which collapses the new mechanism back to an *identity* with its own parent
or reference) — the divisor change is not itself parametrized, so there is no PARAMS setting
that neutralizes it. Committing a known-losing point as "005b" would misrepresent it as a
viable registry entry. Per the issue's own protocol, the finding is recorded here and no
budget is spent chasing a refit the pre-registered evidence already argues against.

## Objective (issue's own framing)

Two questions, both information-value only:

1. Is the elapsed-steps divisor fix, applied to the parent's unchanged frozen space, worth
   enough to matter for the volatility-adaptive class?
2. (Added scope, folded into the same probe run) Does the sibling winner `004`'s own
   `ewma_vol` signal spend a meaningful share of time at or below the floor values
   `WHI-1223`/`004b` used, substantiating — not just hypothesizing — that issue's own closing
   explanation?

## Probe A — estimator replication (`bench estimator-probe`, WHI-1225)

New tooling (`tools/bench/src/estimator_probe.rs`, `bench estimator-probe` subcommand,
committed as part of this issue) runs the **committed** `005` over the 200 `screening` seeds
with a thread-local shadow accumulator that replicates both normalizations
(`var_sum/count` and `var_sum/elapsed_sum`) from the identical `after_swap` payload, without
touching real storage or spending search budget — full report:
`results/2026-08-22-estimator-probe-005b.md`.

| Kill condition | Measured | Threshold | Triggered? |
| --- | --- | --- | --- |
| (i) median `count/n_steps`, low-sigma tercile | 0.417650 | > 0.9 | No |
| (ii) dynamic-range decompression (old→new) | 8.72% | < ~15% AND no rank-correlation improvement | No — decompression is under the bar, but Spearman(sigma_hat, true_sigma) improved 0.507963 → 0.665412, so the "no improvement" half of the AND fails |

**Neither pre-registered kill condition triggers on its literal wording.** But the same run's
raw per-seed data (committed in the report) shows something the issue's own arithmetic did
not anticipate: across **all 200 screening seeds** (not just the low-sigma tercile), the mean
`sigma_hat` drops from **53.805 (old) to 34.645 (new) — a 35.6% reduction** — and at the
parent's fitted map (`FEE_LO=5, A_NUM=13, B_DEN=1265`) that inflates to a **mean fee drop from
76.42 bps to 50.09 bps (−26.33 bps)**. The issue's own § "The mechanism change" section
predicted "a calm fee ~8–15 bps lower, busy fee ~2–3 bps lower" — a reduction concentrated at
the low-sigma end. The measured reduction is both larger and far more broadly distributed
across the sigma range than that estimate.

## Added scope — 004's `ewma_vol` vs. floor sweep (answers WHI-1223's open question)

Same run, second pass: replicates `004`'s `ewma_vol` EWMA from its own `after_swap` payload
and records the fraction of sampled steps (and of executed Y-volume) at or below each
`FLOOR_BPS` the `004b` issue froze, bucketed by this run's own `true_sigma` tercile. Full
table: `results/2026-08-22-estimator-probe-005b.md`.

| sigma tercile | floor (bps) | fraction of steps ≤ floor | fraction of volume ≤ floor |
| --- | --- | --- | --- |
| Low | 25 | 0.5624 | 0.3072 |
| Low | 30 | 0.7028 | 0.4408 |
| Mid | 25 | 0.3584 | 0.1730 |
| Mid | 30 | 0.5347 | 0.3093 |
| High | 25 | 0.1545 | 0.0574 |
| High | 30 | 0.2822 | 0.1280 |

**This substantiates, not merely stays consistent with, `004b`'s own closing hypothesis**
(`strategies/004b-floor-subtracted-ewma-fee/NOTES.md` § Negative result): even in the **Low**
sigma tercile — the calmest third of the sampled range, not an extreme tail — `ewma_vol` sits
at or below 25 bps on 56.2% of sampled steps and at or below 30 bps on 70.3% of steps (30.7%
and 44.1% of executed volume respectively). `004b`'s two probe points used exactly these two
floors (P2 = 25 bps, P1 = 30 bps) and both lost; this measurement is the first direct evidence
of *why*: a floor at that level zeroes the residual on a majority of calm-regime steps, pinning
the fee at `BASE_BPS` in exactly the cells (21/22/24/25) where `004` already loses to
`001@66`. `004b`'s own NOTES.md is updated with this addendum (§ Negative result), and
WHI-1223 (already `Done`) receives a comment linking here — evidence-only, not a reopen.

## Probe B — the single-point degenerate-range evaluation

Scratch degenerate-range method (`004b`/`007`'s own precedent: a throwaway copy with every
`PARAMS` range collapsed to `MIN==MAX`, `bench fit --max-points 1 --no-report`, never
committed). Run from a detached scratch worktree (`git worktree add --detach`), the same
environmental workaround `004b`/`007` document for the reference compile path
(`docs/DESIGN.md` §4.5, `nested-worktree-breaks-cargo-isolated-builds` in this project's own
agent memory), against the repo at commit `3092150` (the commit that adds this issue's Probe
A tooling, `origin/dev` tip plus that one commit at measurement time).

**Storage-layout containment check first** (divisor left at `count`, only the new
`elapsed_sum` field and `STATE_END: 56 -> 64` added): reproduces the parent's own committed
numbers **bit-exactly** —

| Segment | n | Containment check (new layout, old divisor) | Parent's committed number |
| --- | --- | --- | --- |
| screening | 200 | 405.376621 | 405.376621 |
| train | 1,000 | 430.785183 | 430.785183 |
| validation | 1,000 | 425.946116 | 425.946116 |

This rules out a storage-layout bug before trusting any number below.

**0-line, `[66, 0, 2000]`, under the fix:**

| Segment | n | Measured | Predicted |
| --- | --- | --- | --- |
| screening | 200 | 385.631774 | 385.97 ± 1 |
| train | 1,000 | 407.000902 | — |
| validation | 1,000 | 402.634485 | — |

Screening deviation from the prediction: **−0.34**, well inside the issue's own "deviation >
~3 means broken plumbing" tolerance. Plumbing confirmed at this boundary too.

**Parent's own fitted point, `[5, 13, 1265]`, under the fix:**

| Segment | n | Under the fix | Parent's committed number (old divisor) | Delta |
| --- | --- | --- | --- | --- |
| screening | 200 | 386.082675 | 405.376621 | **−19.29** |
| train | 1,000 | 407.721177 | 430.785183 | **−23.06** |
| validation | 1,000 | 402.103157 | 425.946116 | **−23.84** |

The issue's own act table: `delta < −15` reads as "broken plumbing, stop and diagnose." The
storage-layout containment check above already rules out a plumbing bug at the mechanism
level; the deviation instead has a direct, measured explanation — the Probe A finding above
(a 35.6%, broadly-distributed `sigma_hat` reduction, not the assumed calm-only 8–15 bps
shift). The issue's own arithmetic underestimated the fix's magnitude; the *sign* of the
effect (fee can only fall) was correctly one-directional, but the resulting *edge* effect on
the parent's unrefit map is a loss roughly 6–10x the issue's own noise-floor estimate for
`005b` vs `005` (±1.5 to 3), not sampling noise.

Consistent across all three independently-sampled segments (−19 to −24), which rules out a
screening-only artifact.

## Negative result — why the lane closes here

The issue's own act table for Probe B: `delta in [−15, 0]` → "proceed only if Probe A showed
≥15% decompression, else close as a measured negative." Probe A's measured decompression was
**8.72%**, short of that bar. The measured delta here is more negative still (−19.29), and its
magnitude is now independently explained (not a bug, not noise) — so the same "close as a
measured negative" conclusion applies, a fortiori.

Because the divisor fix is not itself a searchable parameter (the issue's own § "The
mechanism change" explicitly rejects a `GAP_AMORT` toggle for exactly this reason — a 4th
search dimension the frozen budget cannot afford), there is no PARAMS point under the
corrected estimator known, without spending the 300-point search budget, to be competitive
with the parent's 405.38/425.95. The issue's own prediction — "the refit's compensating
direction is upward in `FEE_LO`/`A_NUM`" — is plausible, but confirming it requires exactly
the search this probe-gated protocol exists to avoid running when the pre-registered evidence
already points to a likely loss. **0 of the 300-point search budget is spent.**

Unlike `004b` (`FLOOR_BPS=0` is bit-exactly identical to the parent) and `007`
(`K_BPS=10_000` collapses to the CPMM closed form), no parameter setting here reproduces a
tie with the parent — the two measured points bracket the family's honest range under the fix:
385.63 (0-line-equivalent) to 386.08 (parent's own unrefit point), both **worse than the
parent's own 405.38** and statistically indistinguishable from each other. Committing either
as "005b" would misrepresent a measured loss as a viable registry entry, so **no `lib.rs` is
committed.**

## Acceptance criteria (per the issue)

- [x] The issue's own framing is honest: success is the ablation delta, not the ranking —
      this file states that framing in its own Provenance section.
- [x] Probe A is run first, with both kill conditions evaluated and the per-seed data
      committed (`results/2026-08-22-estimator-probe-005b.md`).
- [x] Probe B reproduces at the 0-line within the predicted band; the parent-point deviation
      is diagnosed rather than assumed — found to be a genuine mechanism effect, not broken
      plumbing, via an independent storage-layout containment check.
- [x] The 0-line point `[66,0,2000]` reproduces 385.97 ± 1 (measured 385.631774, deviation
      −0.34).
- [ ] `bench fuzz` re-run — **not applicable**: no `lib.rs` is committed, so there is no
      candidate storage footprint to fuzz.
- [x] The `FEE_LO` extension is not included (the issue's own § "The two candidate changes
      are substitutes, not complements" rejects it; not re-litigated here).
- [x] `FEE_HI` is left frozen (never touched by this issue's own frozen space).
- [ ] The inherited "MLE" comment softening — **not applicable**: no `lib.rs` is committed to
      carry that comment. Recorded here as the issue's own prior art for a future variant
      that does commit source based on this estimator.
- [x] `bench compare 005b vs 005` — **not applicable in its literal form** (no `lib.rs` to
      compare); the equivalent paired evidence is the Probe B table above (both segments'
      deltas against the parent's own committed numbers).
- [x] The refit-returns-to-a-near-parent-map case does not apply; the measured outcome is an
      unambiguous loss, reported as such, not as a tie or a win.
- [x] `cargo test --workspace` green; fmt/clippy per `AGENTS.md` (the new `bench
      estimator-probe` tooling's own tests, plus the full pre-existing suite).

## Verification

- `cargo test --workspace`: 235 passed, 0 failed, 1 ignored (baseline 214 + 21 new: 4
  `estimator_probe.rs` module tests, 6 `commands/estimator_probe.rs` tests, 11 `stats.rs`
  helper tests for `median`/`spearman_rank_correlation`).
- `rustfmt` applied to every touched `tools/bench/**` file; `cargo fmt --all` deliberately not
  run, per the repo's own documented caveat that it reformats inherited upstream files
  outside this issue's scope.
- No new `clippy`-worthy issues in the touched files (`cargo clippy -p prop-amm-bench
  --all-targets`, checked against the diff — the pre-existing `-D warnings` failures in
  upstream-owned `crates/shared` are the repo's own documented baseline, not from this issue).
- `bench estimator-probe` and the scratch degenerate-range `bench fit` runs above were
  executed from a detached scratch worktree outside the primary clone's directory tree (the
  same `nested-worktree-breaks-cargo-isolated-builds` workaround `004b`/`007` document), since
  both shell out to the reference compile path. The committed `results/*.md` report was
  copied back from that scratch run; no scratch strategy source is committed anywhere.
