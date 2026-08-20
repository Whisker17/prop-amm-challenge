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
- **The `Done` state flip cannot ride its own PR** (Low, template bootstrap).
  `docs/agents/issue-tracker.md` § Issue lifecycle ↔ Git — the tracker is in-repo, so
  `State: Done` has to be committed onto the resolved base *after* the merge, outside any PR.
  That is a direct commit to a protected branch by construction. Deferred because the
  alternative (an external tracker) is what we deliberately traded away; mitigation is that
  it is part of the mandatory post-merge cleanup sequence, in the same session.
- **`docs/DESIGN.md`'s bench-timing numbers still lack a `file:line`/measurement citation**
  (Low, WHI-1192). §2.6 (`0.11–0.57 s` fast-path search, `7–10 s`/`~51 MB` reference path),
  §7 #10 (`~25 GB` for 500 points), §4.2 (`~40–60 lines` of per-strategy boilerplate) — these
  describe `tools/bench`'s fast compile path, which does not exist in the repo yet. Deferred
  because re-measuring them requires the tooling WHI-1194 builds; the starter anchor (§2.1,
  the one number that *was* independently reproducible against existing upstream code) was
  verified and cited during this PR's review. Fix: once `tools/bench` lands, re-measure and
  cite via a `results/` snapshot per §3.3, replacing the inherited estimates.
- **`docs/DESIGN.md` now cites `WHI-` issue ids while `AGENTS.md`/`docs/agents/**` still name
  `.scratch/` as the tracker of record** (Low, WHI-1192 review, tracked by WHI-1196). No fix
  here — out of scope by this issue's own description (governance carve-out). Noted so the
  inconsistency isn't silently missed before WHI-1196 lands.

---

## Resolved

_(none yet)_
