# [0.1.0] [Bench] Add the sweep fast path and fit the CPMM fee 0-line

Id: PAMM-003
State: Todo
Status: ready-for-agent
Release: 0.1.0
Labels: feature
Milestone: M0
Priority: High
Blocked by: PAMM-002
Blocks: PAMM-004
Assignee: —
Branch: —

## Objective

Give bench a parameter search that is fast enough to be used (0.11 s per point instead of
7–10 s), then use it to fit `strategies/001-cpmm-fee` and establish the **0-line** — the
score any strategy must beat to be worth anything.

## Context

`docs/DESIGN.md` §2.4, §2.5, §2.6, §2.8. Measured: the CLI's `ensure_build_dir`
(`crates/cli/src/commands/compile.rs:47`) keys an isolated build directory by source hash, so
every parameter point rebuilds `pinocchio`, `wincode`, `darling` and `syn` — 7–10 s and
~51 MB per point. Reusing one build directory with a shared `target/` and rewriting only
`src/lib.rs` measured **0.11–0.57 s** per point.

The fast path re-implements two things `compile.rs` also does — native shim injection
(`native_shim_source`) and `unsafe` rejection (`source_contains_unsafe_keyword`). That
duplication is bought off by the parity gate, not by hoping.

## Blocked By

- `PAMM-002` — distribution mode and the paired statistic.

## Blocks

- `PAMM-004` — grid mode reports against fitted points.

## Implementation

1. **Fast compile path.** One reused build directory, shared `target/`, `src/lib.rs`
   rewritten per point. Mirror `compile.rs`'s `Cargo.toml`, shim injection and `unsafe`
   rejection; add a comment naming `compile.rs` as the authority so a future reader knows
   where to check for drift.
2. **Parameter block protocol.** A strategy's `lib.rs` marks its free parameters with a
   delimited block the sweeper rewrites:
   ```rust
   // === PARAMS BEGIN ===
   const FEE_BPS: u128 = 30;
   // === PARAMS END ===
   ```
   The authored file stays the canonical artifact; generation happens only inside a search.
   The committed `lib.rs` is the fitted point.
3. **Search.** Coarse grid, then coordinate descent. Screening seeds fixed at
   `1_000_000..=1_000_199` for every point in a run (common random numbers). Hard cap
   **300 evaluation points**, recorded in the run's output and in `NOTES.md`.
4. **Final evaluation.** Re-run the winning point on the full train segment (1,000 sims),
   then on validation (1,000 sims).
5. **Parity gate command.** `bench parity --strategy <dir>` reproduces the committed point
   through `prop-amm validate` and `prop-amm run` and diffs against the fast path. A
   mismatch exits non-zero.
6. **Fit `001-cpmm-fee`** over an integer bps space, and write the result into its
   `NOTES.md` and `strategies/README.md`.
7. **Add the `normalizer-as-submission` reference** (`crates/shared/src/normalizer.rs`'s
   curve as a candidate) so §2.8's symmetric zero is measurable.

## Out of scope

- Grid mode, regime slicing, L1 telemetry (`PAMM-004`).
- Any strategy other than `001` and the normalizer reference.

## Acceptance criteria

- [ ] Fast path compiles a parameter point in **< 1 s** on a warm build directory, and the
      whole search holds a single build directory (verify `.build/` does not grow per point).
- [ ] `bench parity --strategy strategies/001-cpmm-fee` passes: fast-path and CLI avg edge
      agree to `1e-9` relative on the same segment.
- [ ] The fee↔edge response over the searched range is **single-peaked**. The curve is
      committed to `results/` — this is the protocol self-check of `docs/DESIGN.md` §2.8, and
      a multi-modal result blocks the issue rather than being reported as a finding.
- [ ] The fitted optimum is recorded with train **and** validation numbers, plus the
      `0..=999` observation row.
- [ ] The search refuses to exceed 300 evaluation points and says so.
- [ ] `strategies/001-cpmm-fee/NOTES.md` records the frozen parameter space, the budget
      spent, the fitted point, and the parity reproduction.
- [ ] `cargo test --workspace` green; fmt/clippy per `AGENTS.md` § Build, test, run.

## Testing / Verification

```bash
cargo run -p bench -- fit --strategy strategies/001-cpmm-fee
cargo run -p bench -- parity --strategy strategies/001-cpmm-fee
```
Expected: a fitted fee, a single-peaked response curve in `results/`, and a passing parity
line. Note whether the optimum is anywhere near the starter's 500 bps — it very likely is
not, and that gap is the point of the issue.

## References

- `docs/DESIGN.md` §2.4, §2.5, §2.6, §2.8
- `crates/cli/src/commands/compile.rs:22,47,150`
