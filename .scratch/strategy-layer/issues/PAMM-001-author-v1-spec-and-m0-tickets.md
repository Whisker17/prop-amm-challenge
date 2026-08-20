# [0.1.0] [Docs] Author the v1 spec and open the M0 tickets

Id: PAMM-001
State: In Progress
Status: ready-for-agent
Release: 0.1.0
Labels: research
Milestone: M0
Priority: Urgent
Blocked by: None (entry point)
Blocks: PAMM-002 PAMM-003 PAMM-004 PAMM-005
Assignee: —
Branch: feat/pamm-001-v1-spec

## Objective

Fill `docs/DESIGN.md` §1–§3 and §5–§8 (and the open decisions inside §4) with the v1 spec
for the strategy layer, and open the M0 implementation tickets. Until this lands, `AGENTS.md`
§Status forbids picking up any strategy issue.

## Context

`docs/DESIGN.md` §4.1/§4.2 described inherited upstream code only; everything else was the
template stub. The spec was derived through a design interview covering twelve decisions:
success criteria, the primary metric, evaluation modes, family-vs-point comparison, compile
paths, seed segmentation, the fidelity contract, observability depth, the search protocol,
baselines, convergence, and the M0 split. Each rejected option is recorded in §7 so it is
not re-proposed.

## Blocked By

None (entry point).

## Blocks

- `PAMM-002`, `PAMM-003`, `PAMM-004` — the M0 issues this ticket creates.
- `PAMM-005` — the governance-lane `AGENTS.md` §Status update, which cannot ride this PR
  (carve-out; `docs/GIT_WORKFLOW.md` § Repo-wide governance carve-out requires the split).

## Implementation

1. Rewrite `docs/DESIGN.md`: §1 (vision, scope, non-goals, success criteria), §2 (the
   measurement protocol, §2.1–§2.10), §3 (parameter provenance, upstream boundary,
   reproducibility, budgets), §4.1 additions, §4.2 with `strategies/` + `tools/bench` +
   `config/bench.toml` + `results/` and the rationale for file-based strategies, §4.3–§4.5,
   §5, §6 (milestones + the blocked strategy list), §7 (22 rejected alternatives), §8 (7
   risks, 6 open questions).
2. Create `.scratch/strategy-layer/issues/PAMM-00{1..5}-*.md`.
3. Allocate ids 001–005 from `.scratch/NEXT_ID`; write back `006` in this same commit.

## Out of scope

- `AGENTS.md` §Status — governance carve-out, see `PAMM-005`.
- The frozen strategy list in §6.2 — owner input, not derivable from the repo.
- Any code.

## Acceptance criteria

- [x] `docs/DESIGN.md` contains no remaining `_(fill in: …)_` placeholder.
- [x] §7 records every alternative rejected during the interview, each with its reason.
- [x] §6.2 states explicitly that M1 is blocked on the strategy list and what fields each
      entry needs.
- [x] Every quantitative claim in the spec cites either a `file:line` or a measurement
      recorded in this repo.
- [x] `.scratch/NEXT_ID` reads `006` and five issue files exist with matching `Id:` lines.

## Testing / Verification

`grep -c "fill in" docs/DESIGN.md` returns 0. Title prefix `[0.1.0]` and `Release: 0.1.0`
agree on every version-scoped issue file created here.

## References

- `docs/DESIGN.md` §1–§8
- `docs/agents/issue-template.md`, `docs/agents/issue-tracker.md`
