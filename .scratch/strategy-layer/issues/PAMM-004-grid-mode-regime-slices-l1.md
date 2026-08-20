# [0.1.0] [Bench] Add grid mode, regime slices and L1 observability

Id: PAMM-004
State: Todo
Status: ready-for-agent
Release: 0.1.0
Labels: feature
Milestone: M0
Priority: High
Blocked by: PAMM-003
Blocks: None
Assignee: —
Branch: —

## Objective

Complete the measurement layer: the fragility matrix that makes `docs/DESIGN.md` §2.3's veto
rule enforceable, and the two derived metrics that let a porter tell "did not attract flow"
apart from "attracted it and priced it badly".

## Context

`docs/DESIGN.md` §2.3, §2.7, §5. Two facts make this possible without touching upstream:
every field of `SimulationConfig` is `pub` and `runner::run_batch_native` accepts an
arbitrary config vector; and `run_simulation_native` takes the submission's and the
normalizer's `after_swap` slots as **separate** arguments, while upstream calls `after_swap`
only on real trades — never during router or arbitrageur quoting (README § afterSwap).

## Blocked By

- `PAMM-003` — fitted points to report on.

## Blocks

None. M0 is complete when this merges.

## Implementation

1. **Grid mode.** Explicit factorial over `norm_fee_bps ∈ {30,55,80}`,
   `norm_liquidity_mult ∈ {0.4,1.0,2.0}`, `gbm_sigma ∈ {1e-4,1e-3,7e-3}`; 27 cells × 40
   seeds; cell `c` draws seeds `4_000_000 + c*1_000 + i`. `retail_arrival_rate` and
   `retail_mean_size` stay at `SimulationConfig::default()` (0.8, 20.0).
2. **Regime slices in distribution mode.** Reconstruct each result's regime via
   `HyperparameterVariance::apply(&base, seed)` and report per-bin paired differences
   alongside the headline number.
3. **L1 telemetry.** Wrap **both** AMMs' `after_swap` in pass-through recorders. The
   wrappers must delegate unchanged — a behaviour change here corrupts every number. Because
   `AfterSwapFn` is a plain `fn` pointer, accumulate through thread-local state and drive
   `engine::run_simulation_native` per config in bench's own rayon loop so counters can be
   reset at a known simulation boundary.
4. **Derived metrics.** Flow share (our executed volume ÷ total executed volume) and edge per
   unit volume, per simulation and aggregated.
5. **Report.** Extend `results/` output with the 27-cell matrix, the slice table, and the two
   derived metrics.

## Out of scope

- L2 (full retail/arb edge decomposition via price-path replay). Deferred — `docs/DESIGN.md`
  §2.7 and §8 open question 6.
- Widening the grid to the retail-flow axes — §8 open question 4.

## Acceptance criteria

- [ ] Grid mode reports 27 cells × 40 seeds with a paired difference and interval per cell.
- [ ] Grid output is labelled as **not** comparable to a distribution-mode headline edge
      (`docs/DESIGN.md` §2.3).
- [ ] Regime slicing reproduces the headline paired mean when all bins are pooled.
- [ ] **Telemetry is provably non-invasive:** the same candidate over the same segment yields
      a bit-identical `total_edge` with recorders installed and with them absent.
- [ ] For the starter over `0..=999`, flow share and edge per unit volume are reported, and
      flow share lies in `[0,1]`.
- [ ] `normalizer-as-submission` reports a flow share near 0.5, confirming the router splits
      symmetrically under identical curves — if it does not, that is a finding worth a
      `## Comments` entry, not a silent pass.
- [ ] `cargo test --workspace` green; fmt/clippy per `AGENTS.md` § Build, test, run.
- [ ] A `results/` snapshot covering `001-cpmm-fee`, starter and the normalizer reference is
      committed, completing M0's success criterion (`docs/DESIGN.md` §6.1).

## Testing / Verification

```bash
cargo run -p bench -- grid --strategy strategies/001-cpmm-fee
cargo run -p bench -- compare --candidate strategies/001-cpmm-fee/lib.rs \
  --reference programs/starter/src/lib.rs --segment validation --slices
```

## References

- `docs/DESIGN.md` §2.3, §2.7, §5, §6.1
- `crates/sim/src/engine.rs:97`, `crates/executor/src/native.rs:7`, `crates/shared/src/config.rs:91`
