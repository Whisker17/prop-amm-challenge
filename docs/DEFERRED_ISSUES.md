# Deferred issues registry

A living log of issues that were **surfaced during review but consciously not fixed**
in the PR that found them. This is not a bug tracker for open work — it is the record of
*known, accepted debt*: things we decided to defer, so a future change touching the same
area starts from knowledge instead of rediscovery.

## How to use this file

- **Add an entry** whenever a review turns up a real issue that a PR deliberately leaves
  unfixed (scope, risk, or priority). Record it here in the same PR that defers it.
- **Reference it** before working on the affected area — check whether the thing you are
  about to "discover" is already logged, and whether a listed fix is now in scope.
- **Close an entry** by moving it to *Resolved* (bottom) with the PR/commit that fixed
  it, rather than deleting — the history is useful.
- Keep entries short. Link the originating tracker issue / PR and the code symbol so the
  entry stays findable as the code moves.

Severity is the reviewer's judgement at defer time: **High** (correctness/safety, fix
soon — anything touching a declared high-risk path defaults to at least High), **Medium**
(operational/perf, fix when convenient), **Low** (nit/consistency).

## Entry format

```markdown
- **<one-line description of the defect>** (<Severity>, <issue-id>).
  `<file>::<symbol>` — what's wrong, why it was deferred, and what the fix would be.
```

---

## Open

- **`bench`'s parity checks are bounded to 2-decimal-place agreement with `prop-amm run`, not
  the "1e-9 relative" / "exactly" the WHI-1193 acceptance criteria state** (Low, WHI-1193).
  `crates/cli/src/output.rs:31-32` only ever prints edge at 2dp — there is no higher-precision
  output to cross-check bench's own full-precision numbers against, and editing that
  upstream-owned file is out of bounds. `tools/bench/src/commands/anchor.rs::edges_agree`
  rounds both sides to 2dp before comparing, and says so in its own doc comment and in the
  committed `results/*.md` report text. Accepted because both bench's aggregate-mode and
  per-seed paths call the exact same `HyperparameterVariance::apply`/`run_batch_native`
  functions `prop-amm run` calls internally with identical seed derivation (docs/DESIGN.md
  §2.2, §4.3), so full-precision agreement is structurally guaranteed by shared code, not
  merely hoped for — but that guarantee is by code inspection, not by a runtime assertion at
  1e-9. Fix: none available without either editing `crates/cli/src/output.rs` (upstream) or
  bench importing `prop_amm_sim`/`prop_amm_shared` types to reconstruct the CLI's own
  in-process value directly instead of parsing its stdout — worth reconsidering if a future
  strategy's ranking ever turns on a margin finer than a cent.
- **Root `Cargo.toml` now carries a one-line diff against upstream** (Low, WHI-1193).
  `Cargo.toml::[workspace].members` gained `"tools/bench"`. Accepted per the issue's own
  instruction: `tools/bench`'s own dependencies (`serde`, `toml`) are pinned inside
  `tools/bench/Cargo.toml` rather than added to `[workspace.dependencies]`, so this member-list
  line is the only upstream-owned-file cost of the whole measurement layer. Fix: none needed
  unless a future upstream sync itself touches the `members` list, in which case re-add this
  line during that merge.
- **`cargo clippy -- -D warnings` fails on inherited upstream code** (Low, template
  bootstrap). `crates/shared/src/instruction.rs:8` (`empty line after doc comment`) and
  `crates/shared/src/normalizer.rs:36,41` (`manually reimplementing div_ceil`, twice) —
  3 lints, all in upstream-owned files. Deferred because fixing them inside any of our PRs
  guarantees a conflict at the next `upstream-sync`, and the lints are cosmetic. Consequence:
  the merge gate is `cargo test --workspace` plus *no new* clippy warnings in touched files,
  **not** a clean `-D warnings` run (`AGENTS.md` § Build, test, run). Fix: send them upstream,
  or clean them in a dedicated `upstream-sync`-adjacent chore once upstream stops moving.
- **`cargo fmt --all -- --check` fails on inherited upstream code** (Low, template
  bootstrap). `crates/shared/src/{config,normalizer}.rs`,
  `crates/sim/src/{arbitrageur,curve_checks,engine}.rs`, `crates/sim/tests/integration.rs` —
  6 files drift from rustfmt. Same reasoning as above: a repo-wide `cargo fmt --all` would
  touch upstream files in every diff and make every future upstream merge conflict on
  formatting. Consequence: run `cargo fmt` on **the files you touched**, never `--all`.
- **A temporary `release/v*` cut is only distinguishable from a live integration branch by
  its open PR into `main`** (Medium, inherited from the template). `docs/GIT_WORKFLOW.md`
  § Fan-out (the live-branch query) — between pushing a fresh cut and opening its PR, the
  query classifies the cut as "live", so a concurrent fan-out merges unreleased `dev` into a
  branch queued for production. Deferred because the only real fix changes the branch-naming
  contract (a distinct prefix for temporary cuts, or pushing only after the PR exists); the
  workflow mitigates it with prose only ("open the PR immediately after the push",
  § Releasing to `main`). Low likelihood in a single-operator repo.
- **`docs/agents/issue-tracker.md` still describes `.scratch/` and the WHI-1192/"`Done`
  state flip" `docs/DEFERRED_ISSUES.md` entries as present-tense open problems**
  (Low, WHI-1199). `docs/agents/issue-tracker.md:9-13` (top-of-file forward-reference),
  `:63-71` (§ Decisions, Decision 1 — "It is left in place, stale text and all"), and
  `:101-138` (§ "What happened to `.scratch/`") — this PR (WHI-1199) deleted `.scratch/`
  and moved both `docs/DEFERRED_ISSUES.md` entries those sections cite to *Resolved*, but
  all three sites still read as if none of that happened. Left unfixed here because
  `docs/agents/issue-tracker.md` is a governance carve-out path
  (`docs/GIT_WORKFLOW.md` § Repo-wide governance carve-out) and this PR's diff is
  non-carve-out only — touching it here would mix scopes, which WHI-1199's own acceptance
  criteria forbid. Fix: WHI-1200 (governance-scoped, carve-out paths only) updates all
  three sites to past tense.
- **`telemetry.rs`'s recorder-dispatch mechanism duplicates `compile.rs`'s `Slot`/
  `LOADED_AFTER_SWAP` shape** (Low, WHI-1195). `tools/bench/src/telemetry.rs::AmmSlot` /
  `REAL_AFTER_SWAP` / `record_and_delegate` reproduce `compile.rs`'s `Slot` /
  `LOADED_AFTER_SWAP` / `call_after_swap` mechanism (static `AtomicPtr` array + a
  `#[repr(usize)]` enum + transmute-and-call) nearly line for line. Deferred because the two
  serve genuinely different jobs despite the shared shape: `compile.rs`'s dispatches between
  two *candidate identities* (`compare`'s two build slots), while `telemetry.rs`'s dispatches
  submission-vs-normalizer and additionally has to fall back to a no-op when no real
  `after_swap` was installed — a case `compile.rs` doesn't have. A shared generic wrapper
  around "install a `fn`-pointer behind an `AtomicPtr`, dispatch through a transmuting
  trampoline" would need its own abstraction over that difference, for a net gain of maybe a
  dozen lines. Fix: revisit if a third call site needs the same shape — two duplicates is a
  pattern worth naming, three is worth extracting.
- **A committed `compare` report with a regime-slice table needed a non-standard filename**
  (Low, WHI-1195). `results/2026-08-20-compare-with-regime-slices.md` — `report.rs`'s
  one-report-per-`(day, stage)` rule means today's `2026-08-20-compare.md` slot was already
  spent by WHI-1193's own compare run, committed before regime slicing existed; regenerating
  it would either silently clobber that evidence (`report.rs` itself refuses this) or require
  deleting it first (destroying committed evidence, also against §3.3). Deferred: no code
  change, since this is `report.rs`'s existing, intentional protection working as designed —
  just an unusual filename for one report. Fix: none needed; `2026-08-20-compare.md` stays
  WHI-1193's, and any future same-day rerun of `compare` needs its own distinctly-named file
  the same way.
- **`commands/l1.rs` bakes `strategies/000-normalizer/lib.rs` into generic measurement
  infrastructure's `--file` default** (Low, WHI-1195). `tools/bench/src/commands/l1.rs::
  DEFAULT_NORMALIZER_AS_SUBMISSION` — a specific strategy id is now a default in a command
  that should otherwise work on any submission file. Deferred (not really disputed, just not
  changed): `commands/anchor.rs::DEFAULT_FILE` already hardcodes `programs/starter/src/lib.rs`
  the same way, and the report-collision constraint (`report.rs` allows one report per
  `(day, stage)`; the ticket wants "a `results/` snapshot covering the starter and the
  normalizer reference" as one artifact) is what forces `l1` to default to a *list* rather
  than a single required path. Fix: if a third baseline ever needs the same treatment,
  consider moving the default file list into `config/bench.toml` instead of a second hardcoded
  constant.

---

## Resolved

- **`docs/DESIGN.md` cites `WHI-` issue ids while `AGENTS.md`/`docs/agents/**` still name
  `.scratch/` as the tracker of record** (Low, WHI-1192). `docs/DESIGN.md` §6.1, §8 —
  inconsistent tracker naming across the repo, owned by the separate governance issue
  WHI-1196. Resolved by WHI-1196 (`dc973d1`), which rebound the live governance path
  (`AGENTS.md`, `docs/GIT_WORKFLOW.md`, `docs/agents/issue-tracker.md`, and friends) to
  Linear naming. Entry moved here by WHI-1199.
- **The `Done` state flip cannot ride its own PR** (Low, template bootstrap).
  `docs/agents/issue-tracker.md` § Issue lifecycle ↔ Git — this was deferred on the
  premise that the tracker is in-repo, so `State: Done` has to be committed onto the
  resolved base *after* the merge, a direct commit to a protected branch by construction.
  Resolved by WHI-1196 (`dc973d1`): the tracker is now Linear, so `State: Done` is a
  `linear.save_issue` call, not a git commit — the protected-branch-commit defect this
  entry named is gone. This closes only that narrow defect, not the broader trade-off it
  sat next to: `docs/agents/issue-tracker.md` § Decisions #2 records, as its own
  already-accepted debt, that Linear and git are now separate systems with no shared
  commit boundary and can drift — real and ongoing, mitigated by operational discipline
  only, not eliminated by this entry's closure. Entry moved here by WHI-1199.
- **`tools/bench`'s fast-path timing (`0.11–0.57 s`/point) has no measurement to cite**
  (Low, WHI-1192). `docs/DESIGN.md` §2.6 — the number described `tools/bench`'s search
  fast path, which didn't exist in the repo yet. Resolved by WHI-1194, which added the
  fast path and re-measured it (`strategies/001-cpmm-fee/NOTES.md`,
  `results/2026-08-20-fit-001-cpmm-fee.md`) — the original estimate was **not**
  reproduced: 160 warm compiles measured min=0.472s, mean=1.000s, max=1.311s in the
  measuring session's environment, re-confirmed under lower system load with no
  improvement, and attributed to that environment rather than the fast path's design
  (which never rebuilds `pinocchio`/`wincode`/`prop-amm-submission-sdk` after the
  directory's first use). §2.6 now cites the real figures instead of the estimate. This
  closes the "no measurement to cite" defect; it does not claim the `< 1s` figure a
  future WHI-1194 acceptance check named — that's a live, disclosed gap in
  `strategies/001-cpmm-fee/NOTES.md`, not a re-opening of this entry.
