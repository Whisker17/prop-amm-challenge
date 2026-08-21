# 001-cpmm-fee

## Provenance

Source form: **inherited, not ported.** This is a copy of `programs/starter/src/lib.rs` — the
challenge's own shipped starter, a constant-product AMM (`x*y=k`) with a flat fee. No external
material, no fidelity risk: the mechanism is already exactly what upstream ships and simulates.

What actually differs from the starter: `NAME` (identifies this strategy in the registry),
the `after_swap` no-op arm's comment, and an added doc comment on `compute_swap`. The
`compute_swap` mechanism itself — and `MODEL_USED`, describing who produced that mechanism —
is untouched.

## Mechanism

Constant-product swap with a single free parameter, the fee, expressed in basis points out of
10,000 as `FEE_BPS` (WHI-1194 rewrote the original `FEE_NUMERATOR / FEE_DENOMINATOR` = 950/1000
into this convention — bit-for-bit equivalent for every input, since scaling a truncating
integer division's numerator and denominator by the same factor never changes its floor;
verified via `prop-amm run` reproducing the unchanged 210.50 starter anchor at `FEE_BPS = 500`
before any search ran). `FEE_BPS` lives in the `// === PARAMS BEGIN/END ===` block
(`tools/bench/src/params.rs`) so `bench fit` can rewrite it during a search. This is the
**0-line** (`docs/DESIGN.md` §2.8): every other strategy is measured against the best
fixed-fee CPMM, and a strategy that doesn't beat it is a negative result, not a mid-table
entry. `001` also doubles as bench's own self-check — its fee-edge response must be
single-peaked, or the bug is in bench, not in a discovery.

## Frozen parameter space (declared before any search runs — docs/DESIGN.md §2.4)

**fee_bps ∈ [1, 500], integer.**

Rationale, so WHI-1194 can search inside it or challenge it before spending the budget:

- **Upper bound 500bps** — the starter's own shipped value (`FEE_NUMERATOR/FEE_DENOMINATOR` =
  950/1000 = 5% = 500bps). It's the parity anchor (§2.1) and DESIGN.md §2.8 explicitly flags it
  as "almost certainly not the family optimum," so the space must include it rather than start
  below it.
- **Lower bound 1bps** — the practical non-zero floor. 0bps removes the fee mechanism's whole
  differentiation (identical to `normalizer-as-submission`, §2.8's other baseline), so it isn't a
  meaningful point in *this* family's space.
- The competitor (normalizer) samples `norm_fee_bps ∈ U[30, 80]` (`crates/shared/src/config.rs`);
  500bps brackets that range with headroom on both sides for a coarse-grid-then-descent search
  (§2.5) to actually locate an interior optimum rather than hit a boundary.

This is a WHI-1193 (M0) decision, not a §6.2 owner-input item — §6.2 is the *frozen strategy
list* for M1's ported strategies, and `001` is the M0 baseline that measures all of them.

## Fitted point (WHI-1194)

Run via `cargo run -p prop-amm-bench -- fit --strategy strategies/001-cpmm-fee`
(`config/bench.toml`'s `[search]` budget, 300 points; screening segment
`1_000_000..=1_000_199`). Coarse grid (150 evenly-spaced points across `1..=500`) then
coordinate descent converged after **159 of 300** *distinct* evaluation points — the search
memoizes a revisit of an already-measured point (`tools/bench/src/search.rs`), so this counts
points actually compiled and simulated, not evaluation attempts — and never approached its
budget cap. Full curve (param → screening avg edge) is committed at
`results/2026-08-20-fit-001-cpmm-fee.md`; it is **strictly increasing from `FEE_BPS = 1` to
66, then strictly decreasing to 500** — a clean single peak, confirming the protocol
self-check (§2.8) before this or any other strategy's numbers are trusted.

**On the "&lt; 1s on a warm build directory" acceptance criterion — WHI-1205 settled this.**
WHI-1194's committed report (`results/2026-08-20-fit-001-cpmm-fee.md`) showed 160 warm
compiles at min=0.472s, mean=1.000s, max=1.311s — roughly 9x the 0.11s bare-`cargo build`
control measured on this same machine during WHI-1193 — and attributed the gap to "this
session's execution environment" without a control in hand. WHI-1205 checked all four
candidate structural causes named in that follow-up issue:

- **Cargo.toml rewritten per point?** No — `ensure_fast_build_dir_at`
  (`tools/bench/src/fast_compile.rs`) only writes it when content differs from the fixed
  template, and the template never changes across points. Ruled out.
- **`--features no-entrypoint` passed?** Yes, already passed on every build. Ruled out.
- **Does `.build/fast` resolve against the root `[profile.release]` (`lto=true`,
  `codegen-units=1`)?** No, when invoked from the workspace root — `cargo build -v`
  showed `-C embed-bitcode=no` on `user_program`'s compile (LTO would require
  `embed-bitcode=yes`) and no explicit `-C lto=…`/`-C codegen-units=…` override. Ruled
  out as an explanation of the gap. **But** this surfaced two real, separate bugs fixed as
  part of this issue:
  1. `FAST_BUILD_DIR` was a bare relative path (`.build/fast`), resolved against the
     runtime working directory rather than the workspace root — invoked via `cargo test`
     (whose cwd is the crate directory), the build dir would land at
     `tools/bench/.build/fast` instead of the repo root. Fixed by anchoring at
     `CARGO_MANIFEST_DIR` (resolved at compile time), independent of runtime cwd.
  2. Independently of cwd: nested one level under a git worktree
     (`.claude/worktrees/<name>/`, this repo's own mandated git-workflow layout —
     confirmed by actually reproducing it there, not just reasoned about), `.build/fast`
     sits somewhere the *outer* primary clone's `exclude = [".build"]` (a literal
     top-level-only pattern) doesn't cover, and cargo hard-errors: `current package
     believes it's in a workspace when it's not`. Fixed the way cargo's own error message
     suggests: an empty `[workspace]` table in the fast-path `Cargo.toml` template makes
     the package its own workspace root unconditionally, regardless of nesting depth or
     any ancestor's `exclude` list. Verified with a regression test that reproduces the
     exact failure against a synthetic nested-workspace fixture, then confirms the empty
     table resolves it (`tools/bench/src/fast_compile.rs`'s
     `a_bare_relative_build_dir_gets_folded_into_an_outer_workspace_when_nested`).
- **Is the timed window wider than the build itself?** Partially, yes — `compile_timed`
  (`tools/bench/src/commands/fit.rs`) times `cargo build` **plus** locating and
  `dlopen`-ing the resulting dylib (a fresh tempfile copy each time). Instrumented
  separately: build ≈0.11-0.13s, load (copy + `dlopen`) ≈0.14-0.22s — real and previously
  unaccounted-for, but nowhere near the missing ~0.75s needed to explain WHI-1194's mean.
  On the narrower sub-question — cargo's own process overhead (spawn, lock acquisition,
  manifest re-parse) rather than the actual compile — that overhead is inherent to any
  `cargo build` invocation, including WHI-1193's own bare-cargo-build control (also
  measured as a full process invocation, not an in-process compile). Since the control and
  the fast path pay the same per-invocation overhead, it cannot be the source of a gap
  *between* them.

None of the four explains the 9x magnitude — the fast path's own mechanism is not at
fault. (The "9x" itself is measured against the 0.11s raw-build-only control; against the
more honest build+load control below, ~0.25-0.35s, WHI-1194's 1.000s mean is closer to
3x — still unexplained by any of the four, just a smaller gap to explain.) A fresh,
bounded re-measurement on this same machine — `bench fit --strategy
strategies/001-cpmm-fee --max-points 8 --no-report` (WHI-1205's new flags; see "Cheap
verification" below) — gives **7 warm compiles: min=0.320s, mean=0.327s, max=0.343s —
MEETS the &lt;1s target**, close to `docs/DESIGN.md` §2.6's original 0.11–0.57s estimate
once build+load are counted together honestly.

**What this does and does not establish.** The gap does not reproduce on this machine
today, and that is consistent with WHI-1194's environment attribution — but
non-reproduction is not confirmation of it: this session ruling out the four named
structural causes and getting a fast number is not proof that WHI-1194's specific
session-environment explanation (rather than some other, still-unidentified transient
factor) was the correct one. What *is* settled: the fast path's code is not the culprit,
the `<1s` criterion is achievable and now demonstrated on this machine, and there's a
real, cited control to compare against for any future recurrence — which is what WHI-1194
lacked. The exact cause of that one session's numbers remains, honestly, unknown.

What *is* demonstrated, regardless of session-to-session variance: the fast path never
rebuilds `pinocchio`/`wincode`/`prop-amm-submission-sdk` after the first point
(`.build/fast/` stays a single directory — verified never to grow, see the Acceptance
criteria checklist), so every subsequent point pays only for relinking `user_program`
itself — at minimum a 5-15x improvement over the reference path's unconditional
7-10s/point (`crates/cli/src/commands/compile.rs`'s per-source-hash isolated build), and
now (WHI-1205) confirmed to meet the sub-1s target outright on a normal run.

**Cheap verification (WHI-1205):** `bench fit` previously had no way to bound a run —
`--strategy` was its only flag, so even a compile-timing smoke test cost a full run at up
to 300 search points (~7 minutes) and wrote a dated `results/` snapshot. `bench fit` now
accepts `--max-points <N>` (bounds the search budget, 1..=300) and `--no-report` (skips
writing to `results/`), so the fast path's compile timing can be checked in well under a
minute — the bounded run above took 43s wall-clock total.

**Winning point: `FEE_BPS = 66`** — well inside the competitor's own `norm_fee_bps ∈ U[30, 80]`
sampling range (`crates/shared/src/config.rs`), and far from the starter's 500bps, matching
this issue's own prediction.

| Segment | n | Avg edge |
| --- | --- | --- |
| screening (search inner loop) | 200 | 384.82 |
| train (final evaluation) | 1,000 | 406.14 |
| validation (final evaluation) | 1,000 | 401.80 |
| observation `0..=999` (reporting only, never a decision input) | 1,000 | 399.97 |

The observation-segment number is what's leaderboard-comparable against the **210.50**
starter anchor (§2.1) — `FEE_BPS = 66` very nearly doubles it.

## Parity gate (docs/DESIGN.md §2.6)

Reproduced via `cargo run -p prop-amm-bench -- parity --strategy strategies/001-cpmm-fee`
against the committed `FEE_BPS = 66` point: `prop-amm validate` passes; the fast path and the
reference path agree on **all 1,000 `observation`-segment seeds to `0` relative difference**
(well inside the `1e-9` gate); the fast-path aggregate (avg edge 399.97) matches
`prop-amm run`'s own 2-decimal output exactly. See
`results/2026-08-20-parity-001-cpmm-fee.md` for the committed snapshot.
