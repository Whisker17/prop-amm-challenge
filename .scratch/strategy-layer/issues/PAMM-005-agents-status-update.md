# [Docs] Update AGENTS.md §Status now that the v1 spec has landed

Id: PAMM-005
State: Todo
Status: ready-for-agent
Release:
Labels: chore
Milestone: M0
Priority: Medium
Blocked by: PAMM-001
Blocks: None
Assignee: —
Branch: —

## Objective

`AGENTS.md` §Status still says `docs/DESIGN.md` §1–§3 and §5–§8 are unwritten and that no
strategy issue may be picked up. `PAMM-001` invalidated both statements. Correct them.

## Context

Repo-wide governance: `AGENTS.md` is in the carve-out list (`docs/GIT_WORKFLOW.md`
§ Repo-wide governance carve-out), so this cannot ride `PAMM-001`'s version-scoped PR — a
mixed PR must be split. **No version prefix, empty `Release:`, base `dev`.**

## Blocked By

- `PAMM-001` — the spec whose landing this records.

## Blocks

None.

## Implementation

1. `AGENTS.md` § Status: replace the "Not written yet" paragraph — the spec exists; what
   remains blocked is **M1**, on the frozen strategy list (`docs/DESIGN.md` §6.2).
2. `AGENTS.md` § Architecture: add `strategies/`, `tools/bench/`, `config/bench.toml` and
   `results/` so it mirrors `docs/DESIGN.md` §4.2, and replace the "our submission source has
   no home yet" sentence — it does now.
3. Leave the verification-baseline paragraph alone; it is still accurate.

## Out of scope

Any change to `docs/DESIGN.md` or to code. This PR touches carve-out files only — if it
touches anything else it must be split.

## Acceptance criteria

- [ ] `AGENTS.md` § Status no longer claims the spec is empty, and names the frozen strategy
      list as M1's blocker.
- [ ] `AGENTS.md` § Architecture lists our own paths and matches `docs/DESIGN.md` §4.2.
- [ ] `git diff --name-only origin/dev...HEAD` returns **only** carve-out paths.
- [ ] PR base is `dev`, and the PR body states the resolved base and the carve-out file list
      it was derived from.

## References

- `docs/GIT_WORKFLOW.md` § Repo-wide governance carve-out
- `docs/DESIGN.md` §4.2, §6.2
