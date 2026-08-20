# 002-normalizer-as-submission

## Provenance

Source form: **ported, faithful reimplementation.** The mechanism is
`crates/shared/src/normalizer.rs::compute_swap` — the challenge's own opponent CPMM — run as
a candidate submission instead of as the fixed opponent. It cannot literally call
`normalizer::compute_swap`: a submission's `Cargo.toml` is hardcoded to
(`pinocchio`, `wincode`, `prop-amm-submission-sdk`) only (`crates/cli/src/commands/compile.rs:22`,
docs/DESIGN.md §4.2), so it can't depend on the upstream-owned `crates/shared` crate, and the
normalizer's own ABI is a raw little-endian byte layout specific to the simulator's internal
opponent call path — not the `wincode`-decoded `ComputeSwapInstruction` every submission
actually receives. `lib.rs` therefore reimplements the identical arithmetic (fixed 30bps fee,
ceiling-division constant product) against the standard submission entrypoint, matching
`001-cpmm-fee`'s boilerplate.

`MODEL_USED` is `Claude Sonnet 5`: unlike `001` (an unmodified copy of the starter), this
file's mechanism was authored fresh for this issue by transcribing `normalizer.rs`'s math
into the submission ABI, so the model attribution reflects that authorship rather than being
inherited from a prior source.

## Mechanism

Constant-product swap with a **fixed** 30bps fee — no free parameter (docs/DESIGN.md §2.8:
"the same curve as the opponent"). No `// === PARAMS BEGIN/END ===` block: there is nothing
for `bench fit` to search here, only `bench anchor`/`bench compare` to measure it.

## Frozen parameter space

None. §2.8 is explicit that this baseline has no free parameter — it exists to make the
*symmetric* case measurable (candidate and opponent run the identical mechanism), not to be
tuned.

## Fitted point

Not applicable — see above.

## Measured numbers

| Segment | n | Avg edge |
| --- | --- | --- |
| observation `0..=999` (reporting only, never a decision input) | 1,000 | 200.08 |

This is the **symmetric zero** (§2.8): running the opponent's own 30bps mechanism as the
candidate still nets a positive edge (less than `001-cpmm-fee`'s fitted 399.97, and well below
the starter's 210.50 anchor at its unfit 500bps) — showing how the order router splits flow
even with no pricing advantage on either side.

Measured via `cargo run -p prop-amm --release -- run strategies/002-normalizer-as-submission/lib.rs`
(the CLI's own default segment/step count). `bench anchor`/`bench compare` were not used for
this baseline: both write a fixed, non-slug-scoped `results/<date>-{anchor,compare}.md`
(`tools/bench/src/commands/{anchor,compare}.rs`), which WHI-1193 had already claimed for
today's date — a pre-existing scope decision of those commands, not something WHI-1194
reworks. `bench fit` is not applicable — no free parameter to search.

## Parity gate (docs/DESIGN.md §2.6)

Reproduced via `cargo run -p prop-amm-bench -- parity --strategy strategies/002-normalizer-as-submission`:
`prop-amm validate` passes; the fast path and the reference path agree on **all 1,000
`observation`-segment seeds to `0` relative difference**; the fast-path aggregate (avg edge
200.08) matches `prop-amm run`'s own 2-decimal output exactly. See
`results/2026-08-20-parity-002-normalizer-as-submission.md` for the committed snapshot.
