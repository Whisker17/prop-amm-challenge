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
actually receives. `lib.rs` therefore reimplements the identical arithmetic against the
standard submission entrypoint, matching `001-cpmm-fee`'s boilerplate.

**Fidelity self-assessment (docs/DESIGN.md §2.9), line-by-line against
`crates/shared/src/normalizer.rs::compute_swap`:**

| `normalizer.rs` | `002`'s `compute_swap` | Match? |
| --- | --- | --- |
| `let net = input_amount * (10000 - fee_bps) / 10000;` | `let net_y/net_x = input_amount * (10_000 - FEE_BPS) / 10_000;` | Identical formula |
| `reserve_x.saturating_sub((k + new_ry - 1) / new_ry) as u64` | same, `saturating_sub((k + new_ry - 1) / new_ry) as u64` | Identical (ceiling division via `+d-1`, not `.div_ceil` — matching what's actually shipped, not what clippy would prefer) |
| `fee_bps`: read from `data[25..27]` (raw byte layout), defaults to 30 if absent/zero | `FEE_BPS`: fixed `const = 30` | **Not identical** — see below |
| `k = reserve_x * reserve_y` (`u128`) | same | Identical |

The one real divergence: `normalizer.rs`'s `fee_bps` is a *runtime* value the simulator
supplies per simulation (`crates/sim/src/engine.rs::amm_norm.set_initial_storage(&config.norm_fee_bps.to_le_bytes())`),
sampled as `norm_fee_bps ~ U[30, 80]` (`crates/shared/src/config.rs::HyperparameterVariance`),
whereas `002`'s `FEE_BPS` is a compile-time constant fixed at 30. This is not a fidelity gap
in the *arithmetic* — it is a structural limit of the submission ABI: a candidate's
`compute_swap` only ever receives its own reserves and its own storage, never the opponent's
sampled fee, so no submission could mirror that per-simulation draw even if it wanted to.
`30` was chosen because it is both `SimulationConfig::default()`'s own `norm_fee_bps` and the
floor of the sampled range — the single fixed point closest to "the opponent's mechanism,
unparameterized."

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
opponent's own mechanism measurable as a candidate, not to be tuned.

## Fitted point

Not applicable — see above.

## Measured numbers

| Segment | n | Avg edge |
| --- | --- | --- |
| observation `0..=999` (reporting only, never a decision input) | 1,000 | 200.08 |

This is a **meaningful zero** (§2.8), not a per-simulation-symmetric one (see the fidelity
self-assessment above: the live opponent's fee is resampled `~U[30, 80]` every simulation, so
this only actually matches the opponent's fee in the roughly 1-in-51 sims where the draw
lands on 30 — the rest of the time it is a fixed-30bps CPMM against a competitor priced
somewhere in `[30, 80]`). Running the opponent's own formula, at its own default fee, as the
candidate still nets a positive edge (less than `001-cpmm-fee`'s fitted 399.97, and well below
the starter's 210.50 anchor at its unfit 500bps) — showing how the order router splits flow
between two AMMs of the identical *kind*, without either side having a curve-shape advantage.

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
