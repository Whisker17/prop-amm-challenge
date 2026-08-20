## Summary

<!-- What this PR does and why -->

## Base branch

Resolved from `docs/GIT_WORKFLOW.md` § Resolving the base branch.
**Never default to `dev`.**

- [ ] `release/vX.Y.Z` (version-scoped) → merge with **squash**
- [ ] `dev` (repo-wide governance carve-out only) → merge with **squash**
- [ ] `dev` (finished version-integration `release/v*` merge-back) → **merge commit**,
      never squash — **human gate**
- [ ] `main` (hotfix, or temporary `release/*` cut) → merge with a **merge commit**,
      never squash

<!-- Signals this base was derived from: title prefix `[X.Y.Z]` / tracker
Release / hotfix label / carve-out paths. Targeting `main`? If
`git log origin/main..origin/dev` holds anything that must not ship yet,
this has to be a hotfix off `origin/main`, not a release. -->

## Fan-out (any PR landing on `dev`)

- [ ] After this merges, `dev` fans out into every live `release/v*` integration
      branch **in this same session** (`docs/GIT_WORKFLOW.md` § Fan-out) —
      a governance rule is only in force on branches that carry it.
      N/A if this PR does not target `dev`.

## Release / hotfix only

- [ ] Project version bumped to match the tag being created
- [ ] Tag + GitHub Release planned; deploy will come **from the tag**, not a branch
- [ ] Hotfix: `main` will be merged back into `dev` **and `dev` pushed** after this
      lands, then `dev` fans out per the section above (an unpushed backmerge makes
      the fan-out ship nothing)
- [ ] Tracker Release `commitSha` will be backfilled after tagging

## Type

- [ ] feat
- [ ] fix
- [ ] chore
- [ ] docs
- [ ] hotfix
- [ ] release

## Test plan

- [ ] `cargo test --workspace` green (or the relevant `-p <crate>` subset, named here)
- [ ] `cargo fmt` applied to **the files this PR touches** — not `--all`
      (`docs/DEFERRED_ISSUES.md`: upstream code is not fmt-clean)
- [ ] No **new** `cargo clippy --workspace --all-targets` warnings in touched files
      (3 inherited lints are pre-existing and logged)
- [ ] Touches an upstream-owned path (`AGENTS.md` §Architecture)? Then this must be an
      `upstream-sync` PR, not a feature PR
- [ ] Changed anything under `crates/sim/` or `crates/shared/src/config.rs`? Then every
      edge number quoted in this PR was re-measured after the change
- [ ] If this touches a declared high-risk path (`docs/GIT_WORKFLOW.md` § High-risk
      paths — none declared today): verification approach documented (dry-run /
      staging / mocked)
- [ ] No new tunable parameters outside `docs/DESIGN.md` §2, or the deviation is
      explained in the Summary

## Notes

<!-- Risks, rollback plan, follow-up TODOs. Deferred review findings go to
docs/DEFERRED_ISSUES.md in this PR. -->
