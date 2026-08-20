# 000-normalizer

## What this is

Not a ranked candidate family. This is the "normalizer-as-submission" baseline from
`docs/DESIGN.md` §2.8: the competitor's own curve, run through the submission interface so
`tools/bench` can measure it exactly like any other candidate. It exists to answer one
question — "how does the order router split flow when both AMMs quote the same curve?" — and
has no frozen search space, no fitting, and no train/validation numbers.

Id `000` (before `001-cpmm-fee`) so it reads as a baseline in `strategies/README.md`, not an
entry competing for the ranking.

## Provenance

Mechanism copied verbatim (constant-product with a fee) from
`crates/shared/src/normalizer.rs` / `programs/normalizer/src/lib.rs`: fee
`997/1000` = 30 bps, matching `SimulationConfig::default().norm_fee_bps`. The submission
plumbing (pinocchio entrypoint, `wincode::SchemaRead` decode struct, `NAME`/`MODEL_USED`/
`get_model_used`) is copied from `programs/starter/src/lib.rs`'s shape, since the normalizer
program itself doesn't implement the submission interface (it uses raw pinocchio syscalls
directly and has no `NAME`/`MODEL_USED`, so it can't be built through `prop-amm build`).

Fidelity: exact. The CPMM formula is bit-for-bit the same division/rounding as
`crates/shared/src/normalizer.rs::compute_swap`.

## Numbers

See `results/2026-08-20-l1.md` (flow share, edge per unit volume, seeds `0..=999`) and
`results/2026-08-20-grid.md` (as the `--reference` in the 27-cell fragility matrix against
the starter).
