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

**Finding — flow share is 0.606, not near 0.5.** docs/DESIGN.md §2.8 frames
normalizer-as-submission as "the same curve as the opponent" implying a symmetric split, and
explicitly allows for this: "If it does not [report near 0.5], that is a finding worth a
comment, not a silent pass." It doesn't, and here's why: this fixture's fee is *fixed* at 30
bps, while the opponent's fee is *sampled* per simulation from `U[30, 80]` bps (mean ~55) —
so on average this fixture is meaningfully cheaper than its opponent, and retail/arb flow
correctly routes to the cheaper venue more often than not. That fee asymmetry dominates the
opposite-signed liquidity asymmetry (this fixture's reserves are fixed at
`SimulationConfig::default()`'s `initial_x`/`initial_y`, while the opponent's are scaled by
`norm_liquidity_mult ~ U[0.4, 2.0]`, averaging *deeper* than this fixture's — which on its own
would pull flow share *below* 0.5). True 30bps-vs-30bps, liquidity-mult-1.0-vs-1.0 symmetry
only holds pointwise at one particular sampled config, not in aggregate over the graded
distribution's own sampling.
