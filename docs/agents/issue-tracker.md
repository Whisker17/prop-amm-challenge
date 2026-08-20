# Issue tracker: local markdown (`.scratch/`)

Issues and specs (you may know a spec as a PRD) for this repo live as **markdown files
under `.scratch/`, committed to this repo**. There is no external tracker: no API, no MCP
server, nothing to be unreachable. That is the point — a solo competition repo does not
need shared state, and a tracker that can't be reached can't block the git workflow.

Because the tracker is files in the repo, **moving an issue's state is a commit**, and it
lands in the same PR as the work. "The tracker is out of sync" and "the diff is wrong" are
the same failure here.

## Layout

```text
.scratch/
├── NEXT_ID              # the next unallocated numeric id, one line, e.g. `007`
├── <feature-slug>/
│   ├── spec.md          # the feature's spec, when /to-spec produced one
│   └── issues/
│       ├── PAMM-001-sample-the-fee-grid.md
│       └── PAMM-002-add-edge-regression-harness.md
└── <another-feature>/
```

- **One feature per directory**, slug in `kebab-case`.
- **One file per issue**, named `PAMM-NNN-<slug>.md`. Never a combined tickets file — the
  git workflow needs one addressable id per branch and PR.
- **Ids are repo-global and monotonic**, not per-feature. Allocate by reading
  `.scratch/NEXT_ID`, using that number, and writing back the increment **in the same
  commit** that creates the issue file. Ids are never reused, never renumbered — a branch
  name and a PR title point at them forever.

This deviates from the upstream skill template's per-feature `01`, `02` numbering, and
deliberately: `docs/GIT_WORKFLOW.md` § Branch naming requires a tracker id that is unique
across the repo (`feat/pamm-007-topic`), and per-feature numbering collides on the second
feature.

## Issue file format

The body follows `docs/agents/issue-template.md`. What the tracker adds is a **metadata
block at the top of the file**, before the first `##` heading — these lines are what other
skills and the git workflow read:

```markdown
# [0.1.0] [Sim] Add an edge regression harness

Id: PAMM-002
Status: ready-for-agent
Release: 0.1.0
Labels: feature
Milestone: M1
Blocked by: PAMM-001
Assignee: —
Branch: feat/pamm-002-edge-regression-harness
State: Todo

## Objective
...
```

| Line | Meaning |
| --- | --- |
| `Id:` | `PAMM-NNN`. Must match the filename. |
| `State:` | Lifecycle — `Todo` / `In Progress` / `In Review` / `Done` / `Canceled`. |
| `Status:` | Triage role from `docs/agents/triage-labels.md`. Orthogonal to `State:`. |
| `Release:` | The version this ships in. **This is the release entity** — see below. Empty for governance and `upstream-sync` issues. |
| `Labels:` | Type labels — `bug` / `feature` / `research` / `chore` / `hotfix` / `upstream-sync`. `hotfix` and `upstream-sync` change git routing. |
| `Milestone:` | Capability stage per `docs/DESIGN.md` §6. Orthogonal to `Release:`, never routes git. |
| `Blocked by:` / `Blocks:` | Space/comma-separated ids, or `None (entry point)`. |
| `Branch:` | Filled in when work starts; the audit trail from issue to PR. |

Comments and discussion append at the bottom of the file under `## Comments`, newest last,
each entry prefixed with an ISO date.

## Issue lifecycle ↔ Git (mandatory)

Implementation always uses a **git worktree** off the **resolved base**
(`docs/GIT_WORKFLOW.md` — version-scoped work targets `release/v{version}`, governance and
upstream sync target `dev`, hotfix targets `main`). The `State:` line moves in lockstep:

| When | `State:` |
| --- | --- |
| Claimed / coding in the worktree | `In Progress` |
| PR opened against the resolved base | **`In Review`** |
| PR squash-merged into the resolved base | **`Done`** |
| Abandoned | `Canceled` |

Two practical consequences of the tracker being in-repo:

- The `In Progress` and `In Review` flips are commits **on the feature branch**, so they
  ride the PR. The `Done` flip cannot be — the PR is already merged. Land it on the
  resolved base as part of post-merge cleanup (`docs/GIT_WORKFLOW.md` § Post-merge
  cleanup), in the same session, before the fan-out step.
- Because issue files live under `.scratch/` and **not** in the governance carve-out, an
  issue file edit does not turn a version-scoped PR into a governance PR. Editing the
  issue you are implementing is expected and does not need splitting.

## Release ↔ version binding

`docs/GIT_WORKFLOW.md` § Version determination resolves a version-scoped issue's base
branch from two signals: the `[X.Y.Z]` title prefix (primary), cross-checked against **this
tracker's release entity**.

**The release entity is the `Release:` line in the issue file.** It is a real
cross-check, not a vacuous one — the two signals are written independently (title heading
vs metadata line) and can disagree, which is exactly what the check exists to catch:

```bash
# Read both signals for an issue and compare them by hand before creating the worktree.
f=.scratch/<feature>/issues/PAMM-002-*.md
head -1 "$f"                      # -> # [0.1.0] [Sim] ...
grep -m1 '^Release:' "$f"         # -> Release: 0.1.0
```

Nothing enforces a non-empty `Release:` — a plain file has no validation. **Enforcement is
the agent's refusal**: a missing prefix, a missing `Release:`, or a disagreement between
them means refuse to start and surface it. Never fall back to `dev`.

Governance issues and `upstream-sync` issues carry no version prefix and an empty
`Release:` — for them the version-scoped row does not apply at all.

## Release records

The version axis needs a home too (`docs/GIT_WORKFLOW.md` § Version axis). One file per
version, `.scratch/releases/X.Y.Z.md`:

```markdown
# 0.1.0

State: Planned
Tag: —
CommitSha: —
Submitted: —
Score: —

## Issues
- PAMM-001
- PAMM-002
```

- `State:` — `Planned` → `Released`.
- `Tag:` / `CommitSha:` — backfilled after tagging. `CommitSha:` is the **commit** the tag
  points at, so dereference it: `git rev-parse 'v0.1.0^{commit}'` (a bare
  `git rev-parse v0.1.0` returns the tag object and will never match a branch head).
- `Submitted:` / `Score:` — the date the `lib.rs` from that tag was submitted to the
  challenge UI, and what it scored. This is the whole reason the axis exists here: without
  it, a leaderboard number cannot be tied to a tree.

A version's issue list here and the `Release:` lines on the issues are the same fact
written twice. They must agree; when they don't, the issue file wins (it is what git
routing reads).

## When a skill says "publish to the issue tracker"

Create `.scratch/<feature-slug>/issues/PAMM-NNN-<slug>.md` (creating the directories as
needed), body per `docs/agents/issue-template.md`, metadata block per the table above.
Allocate `NNN` from `.scratch/NEXT_ID` and write back the increment in the same commit.
All issue content is written in English.

## When a skill says "fetch the relevant ticket"

Read the file. `grep -rl 'Id: PAMM-002' .scratch/` finds it from the id alone; the user
will usually just pass the id or the path.

## Publishing ticket sets (e.g. from `/to-tickets`)

1. **Order:** allocate ids in dependency order — blockers first — so later files can
   reference real ids.
2. **Blocking edges:** the `Blocked by:` / `Blocks:` metadata lines *are* the edges; there
   is no native relation to mirror. Both directions must be written — a one-sided edge is a
   bug, since the frontier query below reads `Blocked by:` only.
3. **Scoping:** every issue in a version batch gets the `[X.Y.Z]` title prefix **and** a
   matching `Release:`. Milestone is orthogonal — set it when the set is a capability
   stage, and keep it out of the title.
4. **State + labels:** `State: Todo`, `Status: ready-for-agent` unless the user says
   otherwise.
5. **One commit for the set**, including the `NEXT_ID` bump, so the ids are never
   half-allocated.

## Wayfinding operations

Used by `/wayfinder`. The **map** is a file with one **child** file per ticket.

- **Map:** `.scratch/<effort>/map.md` — the Notes / Decisions-so-far / Fog body.
- **Child ticket:** `.scratch/<effort>/issues/PAMM-NNN-<slug>.md` with the question in the
  body, plus `Type:` (`research` / `prototype` / `grilling` / `task`) and
  `Status: claimed|resolved` lines.
- **Blocking:** the `Blocked by:` line. A ticket is unblocked when every id it lists is
  `resolved`.
- **Frontier:** scan the effort's `issues/` for files that are open, unblocked and
  unclaimed (`Assignee: —`); lowest id wins.
- **Claim:** set `Assignee:` and `Status: claimed`, and **commit before any work** — an
  uncommitted claim is invisible to a parallel session.
- **Resolve:** append the answer under `## Answer`, set `Status: resolved`, then append a
  context pointer to the map's Decisions-so-far.

Wayfinder tickets are **decision** tickets, not build slices: no version prefix, empty
`Release:` (`docs/GIT_WORKFLOW.md` § Version axis) — nothing ships from resolving one.

## Upstream PRs as a triage surface

**No.** `upstream` (`benedictbrady/prop-amm-challenge`) is read-only and its PR queue is
not ours to triage. `/triage` reads `.scratch/` only. What upstream *merges* reaches us
through `docs/GIT_WORKFLOW.md` § Upstream sync, as a tracked issue.
