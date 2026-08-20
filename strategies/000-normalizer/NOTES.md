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

Mechanism copied (constant-product with a fee) from `crates/shared/src/normalizer.rs` /
`programs/normalizer/src/lib.rs`, fixed at fee `997/1000` = 30 bps
(`SimulationConfig::default().norm_fee_bps`'s own value). The submission plumbing (pinocchio
entrypoint, `wincode::SchemaRead` decode struct, `NAME`/`MODEL_USED`/`get_model_used`) is
copied from `programs/starter/src/lib.rs`'s shape, since the normalizer program itself
doesn't implement the submission interface (it uses raw pinocchio syscalls directly and has
no `NAME`/`MODEL_USED`, so it can't be built through `prop-amm build`).

Fidelity: exact **at this fixed 30 bps parameterization** — the integer division and
rounding are bit-for-bit `crates/shared/src/normalizer.rs::compute_swap`'s own formula
evaluated at `fee_bps = 30`. It is *not* a dynamic mirror of the opponent's actual per-
simulation mechanism: `crates/sim/src/engine.rs` sets the opponent AMM's storage to the
*sampled* `config.norm_fee_bps` (`U[30, 80]`) every simulation, and a submission has no way
to read that opponent-side value — a `lib.rs` only ever sees its own instruction data. So
this fixture is "the same curve as the opponent" only in the sense of sharing the opponent's
formula and *default* constant, not in the sense of tracking the opponent's actual sampled
fee simulation-by-simulation. That distinction is exactly what the Finding below turns on.

## Numbers

See `results/2026-08-20-l1.md` (flow share, edge per unit volume, seeds `0..=999`) and
`results/2026-08-20-grid.md` (as the `--reference` in the 27-cell fragility matrix against
the starter).

**Finding — the observation-segment flow share is 0.606, not near 0.5.** WHI-1195's
acceptance criteria (Linear) anticipate this outcome explicitly: "`normalizer-as-submission`
reports a flow share near 0.5, confirming the router splits symmetrically under identical
curves. If it does not, that is a finding worth a comment, not a silent pass." It doesn't,
on the `observation` segment, and here's why: this fixture's fee is *fixed* at 30 bps, while
the opponent's fee is *sampled* per simulation from `U[30, 80]` bps (mean ~55) — so on
average this fixture is meaningfully cheaper than its opponent, and retail/arb flow correctly
routes to the cheaper venue more often than not. That fee asymmetry dominates the
opposite-signed liquidity asymmetry (this fixture's reserves are fixed at
`SimulationConfig::default()`'s `initial_x`/`initial_y`, while the opponent's are scaled by
`norm_liquidity_mult ~ U[0.4, 2.0]`, averaging *deeper* than this fixture's — which on its own
would pull flow share *below* 0.5).

This is not merely a plausible story: `tools/bench/src/telemetry.rs`'s
`matched_curve_and_reserves_produce_flow_share_near_half` test pins `norm_fee_bps = 30` and
`norm_liquidity_mult = 1.0` (matching this fixture's own fixed curve and reserves) and
verifies flow share *does* land within 0.01 of 0.5 there — docs/DESIGN.md §2.8's "perfect
symmetry" claim holds exactly where its own preconditions (matched curve, matched reserves)
hold. The `observation` segment's 0.606 is not evidence against that claim; it measures a
different, harder comparison (fixed 30 bps vs. a sampled `U[30,80]` opponent) that §2.8 never
promised would be symmetric.
