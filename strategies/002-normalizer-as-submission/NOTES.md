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

Run via `cargo run -p prop-amm-bench -- anchor --file strategies/002-normalizer-as-submission/lib.rs`
and `cargo run -p prop-amm-bench -- compare --candidate strategies/002-normalizer-as-submission/lib.rs
--reference strategies/001-cpmm-fee/lib.rs`. See `results/` for the committed snapshots.

<!-- Filled in after the real bench anchor/compare runs (docs/GIT_WORKFLOW.md's
     nested-worktree caveat: run from a scratch worktree outside `.claude/worktrees/`). -->
