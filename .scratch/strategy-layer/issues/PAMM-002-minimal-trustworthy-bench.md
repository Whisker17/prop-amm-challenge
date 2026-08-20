# [0.1.0] [Bench] Stand up the minimal trustworthy bench and the strategies layout

Id: PAMM-002
State: Todo
Status: ready-for-agent
Release: 0.1.0
Labels: feature
Milestone: M0
Priority: Urgent
Blocked by: PAMM-001
Blocks: PAMM-003 PAMM-004
Assignee: —
Branch: —

## Objective

Create the `strategies/` layout and a `tools/bench` that runs one candidate over a seed
segment and reports the paired-by-seed statistic — and **prove it agrees with the upstream
CLI**. No search, no grid, no telemetry: this issue exists so every later number rests on a
measurement layer that was cross-checked against upstream code before anything was built on
it (`docs/DESIGN.md` §6.1).

## Context

`docs/DESIGN.md` §2.1–§2.2, §2.6, §4.2, §4.3. The only anchor produced by an upstream code
path is the starter program's **avg edge 210.50** over seeds `0..=999` (native, 10,000 steps).
`SimResult` carries `(seed, submission_edge)` only, so bench must call
`runner::run_batch_native` itself to obtain per-seed values — `prop-amm run` prints
aggregates only.

Compilation in this issue goes through the **reference path only**: shell out to
`prop-amm build <file>` and parse its printed `  Native: <path>` line, then load that dylib
with `libloading`. This is deliberately the slow path (7–10 s + BPF per file); the fast path
is `PAMM-003`, and it will be validated against the numbers this issue establishes.

## Blocked By

- `PAMM-001` — the spec this implements.

## Blocks

- `PAMM-003` — needs bench's distribution mode and paired statistic to exist.
- `PAMM-004` — same.

## Implementation

1. **`strategies/` layout.** Create `strategies/README.md` (the registry table: id, name,
   source form, status, current numbers) and `strategies/001-cpmm-fee/` holding a
   `lib.rs` copied from `programs/starter/src/lib.rs` with the fee expressed as the single
   free parameter, plus a `NOTES.md` following `docs/DESIGN.md` §2.4 (provenance, mechanism,
   frozen parameter space, measured numbers). **Do not fit it here** — that is `PAMM-003`.
2. **`tools/bench` crate.** Add `tools/bench` to `[workspace] members` in the root
   `Cargo.toml`. That file is upstream-owned; log the one-line sync cost in
   `docs/DEFERRED_ISSUES.md` as accepted debt in this PR.
3. **`config/bench.toml`** — seed segments per `docs/DESIGN.md` §2.2, parsed into a typed
   struct with fail-fast validation (`config/README.md` convention). Segments must be
   validated disjoint at load time.
4. **Distribution mode.** Build `Vec<SimulationConfig>` via
   `HyperparameterVariance::apply(&base, seed)` over a named segment and run
   `runner::run_batch_native`.
5. **Paired statistic.** Given two `BatchResult`s over the same seed list: per-seed
   difference, mean, standard error, t-interval. Implement inline (~30 lines) — no
   statistics crate, per `docs/DESIGN.md` §4.1.
6. **CLI surface.** `bench compare --candidate <path> --reference <path> --segment
   <train|validation|test|observation> [--sims N] [--steps N]`. Refuse `--segment test`
   unless an explicit `--i-am-spending-the-test-segment` flag is passed (`docs/DESIGN.md`
   §2.2: single use).
7. **Report writer.** Emit `results/<date>-<stage>.md` carrying commit sha, segment, sim and
   step counts, and execution path (`docs/DESIGN.md` §3.3).

## Out of scope

- The fast compile path, parameter search (`PAMM-003`).
- Grid mode, regime slicing, L1 telemetry (`PAMM-004`).
- Fitting the CPMM fee family (`PAMM-003`).

## Acceptance criteria

- [ ] `bench` run on `programs/starter/src/lib.rs` over segment `observation`
      (seeds `0..=999`, 10,000 steps, native) reports **avg edge 210.50**, matching
      `prop-amm run` to a relative tolerance of `1e-9` on `total_edge`.
- [ ] **Per-seed agreement:** for 20 seeds sampled from `0..=999`, bench's per-seed edge
      equals `prop-amm run <file> --simulations 1 --seed-start <seed>`'s reported total,
      exactly (same code path, single simulation).
- [ ] `bench compare` on two identical inputs reports a paired mean difference of exactly
      `0.0` with a zero-width interval.
- [ ] `config/bench.toml` with overlapping segments fails at load with a named error.
- [ ] `--segment test` without the explicit spend flag exits non-zero and changes nothing.
- [ ] `cargo test --workspace` green; `cargo fmt` applied to touched files only; no new
      clippy warnings in touched files (`AGENTS.md` § Build, test, run).
- [ ] A `results/` snapshot for the starter anchor is committed.

## Testing / Verification

```bash
cargo run -p bench -- compare --candidate programs/starter/src/lib.rs \
  --reference programs/starter/src/lib.rs --segment observation
prop-amm run programs/starter/src/lib.rs                      # -> Avg edge: 210.50
prop-amm run programs/starter/src/lib.rs --simulations 1 --seed-start 137   # per-seed spot check
```

## References

- `docs/DESIGN.md` §2.1, §2.2, §2.6, §3.3, §4.2, §4.3, §6.1
- `crates/shared/src/result.rs:2`, `crates/sim/src/runner.rs`, `crates/shared/src/config.rs:91`
