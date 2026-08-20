# Issue tracker: Linear

Issues and specs (you may know a spec as a PRD) for this repo live in **Linear** — team
`Whisker-Personal`, project **"Prop AMM Challenge — strategy layer"**. Ids are
`WHI-NNNN`, allocated by Linear itself when an issue is created; nothing here allocates
them by hand.

This replaces an earlier design where the tracker was markdown files committed under
`.scratch/`. That design's one structural advantage — "moving an issue's state is a
commit, so it lands in the same PR as the work" — does **not** carry over: Linear and git
are now two independently-updatable systems that can disagree. § Decisions below records
that explicitly, with the mitigation. `.scratch/` itself is untouched by this move — see
§ What happened to `.scratch/`.

## Reading and writing issues

Reached through the `linear.*` tools exposed via the slim-tools MCP gateway
(`discover_tools` then `execute_code`), not a markdown file read.

- **Fetch an issue:** `linear.get_issue({id: "WHI-1196", includeReleases: true})`. Always
  pass `includeReleases: true` when the version cross-check matters (§ Release ↔ version
  binding) — it is not in the default response.
- **Create or update an issue:** `linear.save_issue({...})`. Omit `id` to create; pass it
  to update. One call sets title, description, state, assignee, labels, priority,
  `parentId`, `blocks`/`blockedBy`/`relatedTo`, and `addReleases`/`setReleases` — see the
  field mapping below for which of these replaces which piece of the old metadata block.
- **List / search issues:** `linear.list_issues({project: "...", query: "...", ...})`.
- **Comment:** `linear.save_comment({issueId, body})`.

## Field mapping (old markdown line → Linear field)

The old tracker's metadata block was plain text because the tracker *was* a text file.
Every one of those lines is now a real, typed field on the issue object — set via
`save_issue`, not written as a line of markdown. There is no metadata block to author by
hand anymore; the table below exists to translate old habits and old references in other
docs, not to describe a format you produce.

| Old `Key:` line | Linear field | Notes |
| --- | --- | --- |
| `Id:` | the issue identifier (`WHI-NNNN`) | Assigned by Linear on create. Never hand-allocated. |
| `State:` | `state` (workflow status) | Team's states are `Backlog` / `Todo` / `In Progress` / `In Review` / `Done` / `Canceled` / `Duplicate` — a 1:1 name match with the old lifecycle vocabulary below. |
| `Status:` (triage role) | a **label** | `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix` already exist as labels for this team. An issue can carry other labels too — the triage role is just one of them, same as before it was "one value on a line." |
| `Release:` | the linked **release entity** | `save_issue({addReleases: ["<release id or slug>"]})` or `setReleases`. See § Release ↔ version binding — this is *not* the same thing as the project or a milestone. |
| `Labels:` (type) | Linear labels | `research`, `chore`, `hotfix` already exist (lowercase, this team). `bug`, `feature`, `upstream-sync` don't exist yet as lowercase labels — create with `linear.create_issue_label({name, team})` the first time one is needed, matching the existing casing. |
| `Milestone:` | **not a Linear field** | `docs/DESIGN.md` §6 owns the M0–M3 milestone concept as documentation. Linear's own per-project milestone object is unused here (`list_milestones` returns empty) — standing it up is out of scope for this rebind. Reference the milestone in the issue body's `## Context` instead. |
| `Blocked by:` / `Blocks:` | native `blockedBy` / `blocks` relations | Set via `save_issue({blockedBy: [...]})` / `({blocks: [...]})`. Strictly better than the old convention: `get_issue({includeRelations: true})` returns them as real edges, not text to parse. |
| `Assignee:` | native `assignee` | User id, name, email, or `"me"`. |
| `Branch:` | native `gitBranchName` | Linear computes this per issue (e.g. `demiwhisker/whi-1196-governance-rebind-...`) — prefer it over hand-rolling a branch name (`docs/GIT_WORKFLOW.md` § Branch naming). |
| `Priority:` | native `priority` | `0=None` / `1=Urgent` / `2=High` / `3=Medium` / `4=Low`. |

`docs/agents/issue-template.md` covers the title convention and the body sections
(`## Objective`, `## Implementation`, `## Acceptance criteria`, …) — those are unchanged;
they're markdown inside the issue's `description` field instead of markdown inside a
committed file.

## Decisions (recorded 2026-08-20, WHI-1196)

These three were explicitly called out as decisions to make and record, not invent —
here they are, with the reasoning.

**1. Is the in-repo tracker deleted, or kept as a read-only archive?**
Neither, exactly. `.scratch/` was **never actually populated** — `NEXT_ID` was still `001`,
no issue file was ever created, `releases/` held only `.gitkeep`. There is no real history
to lose by deleting it and no real history to preserve by archiving it, so the "two
authorities" risk the original framing worried about doesn't exist in practice: there is
nothing in `.scratch/` that could disagree with Linear. It is left in place, untouched,
and dead — see § What happened to `.scratch/` for why this PR doesn't delete it either.

**2. What replaces "state change is a commit"?**
Nothing does — this is accepted explicitly, not papered over. The old tracker's structural
guarantee was that an issue's `State:` line and the diff that changed it landed in the same
commit, so "the tracker is out of sync" and "the diff is wrong" were literally the same
failure. Linear and git are now two separate systems with no shared commit boundary; they
can drift. The mitigation is operational discipline, not a technical fix:
- Flip the Linear `state` as the **literal next action** at each git milestone — worktree
  created → `In Progress` *before* the first commit; PR opened → `In Review` immediately;
  PR merged → `Done` as the *first* step of post-merge cleanup — rather than batching
  updates at the end of a session. This minimizes the window where the two can disagree,
  it does not close it.
- Every commit message and PR title carries the `WHI-NNNN` id (already required by
  `docs/GIT_WORKFLOW.md`), so a human auditing after the fact can always reconcile Linear
  state against git history by searching for the id — the two systems are separate, but
  never unlinked.

**3. Does the version cross-check use Linear's release entity or the project?**
**The release entity**, never the project and never a milestone. Verified non-vacuous
before deciding, not assumed: issue WHI-1193 carries the title prefix `[0.1.0]` as free
text *and* a separately-linked `releases[]` entry (`version: "0.1.0"`, pipeline
"Prop-AMM-Challenge", release id `ed825007-...`) fetched via
`get_issue({includeReleases: true})`. These are two genuinely different fields, set
independently — exactly what `docs/GIT_WORKFLOW.md` § Version determination requires for
the cross-check to be real rather than reading the same fact twice. The project
("Prop AMM Challenge — strategy layer") is a single fixed value for every issue in this
repo and could never serve as a version signal.

## What happened to `.scratch/`

Untouched. This PR touches only the governance carve-out file list
(`docs/GIT_WORKFLOW.md` § Repo-wide governance carve-out), and `.scratch/` is not on that
list — deleting it here would violate this same issue's own scope constraint ("the diff
contains carve-out paths only"). Since it holds no real content (see Decision 1), leaving
it costs nothing today. Physically removing it is a separate, small chore issue for
whoever picks it up next; nothing in this repo reads it anymore as of this change.

## Issue lifecycle ↔ Git (mandatory)

Implementation always uses a **git worktree** off the **resolved base**
(`docs/GIT_WORKFLOW.md` — version-scoped work targets `release/v{version}`, governance and
upstream sync target `dev`, hotfix targets `main`). The Linear `state` moves in lockstep,
each flip made as the literal next action (§ Decisions, #2):

| When | Linear `state` |
| --- | --- |
| Claimed / coding in the worktree | `In Progress` |
| PR opened against the resolved base | **`In Review`** |
| PR squash-merged into the resolved base | **`Done`** |
| Abandoned | `Canceled` |

## Release ↔ version binding

`docs/GIT_WORKFLOW.md` § Version determination resolves a version-scoped issue's base
branch from two signals: the `[X.Y.Z]` title prefix (primary), cross-checked against
**this tracker's release entity** (§ Decisions, #3).

```
# Fetch both signals for an issue and compare them by hand before creating the worktree.
linear.get_issue({ id: "WHI-1193", includeReleases: true })
#   .title            -> "[0.1.0] [Bench] ..."          (primary signal)
#   .releases[0].version -> "0.1.0"                       (cross-check)
```

Nothing enforces these agreeing — Linear validates neither field against the other.
**Enforcement is the agent's refusal**: a missing prefix, a missing linked release, or a
disagreement between them means refuse to start and surface it. Never fall back to `dev`.

Governance issues and `upstream-sync` issues carry no version prefix and no linked
release — for them the version-scoped row does not apply at all. (WHI-1196 itself is an
example: no `[X.Y.Z]` prefix, no release entity.)

## Release records

The version axis needs a home too (`docs/GIT_WORKFLOW.md` § Version axis). One **release**
per version, on the "Prop-AMM-Challenge" pipeline:

- **Create/update:** `linear.save_release({ name: "0.1.0", version: "0.1.0", pipeline: "Prop-AMM-Challenge", commitSha: "..." })` — omit `id` to create, pass it to update (e.g. to backfill `commitSha` after tagging).
- **Read:** `linear.get_release({ id: "<release id or slug>", includeReleaseNotes: true })` or `linear.list_releases({ pipeline: "Prop-AMM-Challenge" })`.
- **Attach issues:** `linear.save_issue({ id: "WHI-NNNN", addReleases: ["<release id or slug>"] })`.

Fields that matter, same intent as the old per-file record:

- `stage` — the release pipeline stage (`Planned` → `Released`, this team's stages).
- `commitSha` — backfilled after tagging. This is the **commit** the tag points at, not
  the tag object — dereference explicitly: `git rev-parse 'v0.1.0^{commit}'` (a bare
  `git rev-parse v0.1.0` returns the tag object and will never match a branch head).
- `startDate` / `targetDate` / `completedAt` — the release's own dates.

A version's linked issues (via `releases[]`) and each issue's title prefix are the same
fact written twice, same as before — now in two separately-editable fields rather than two
lines in one file. They must agree; when they don't, the linked release entity wins (it is
what git routing reads, per § Decisions #3).

## Wayfinding operations

Used by `/wayfinder`. Linear's native primitives replace what the old scheme hand-rolled
with map files, child-ticket files, and directory scans — the map/ticket/blocking/frontier
*concepts* from `wayfinder/SKILL.md` are unchanged, only where they physically live:

- **Map:** a Linear issue labelled `wayfinder:map`. Its body is the map body from
  `wayfinder/SKILL.md` (`## Destination` / `## Notes` / `## Decisions so far` / etc.).
- **Child ticket:** a Linear issue with `parentId` set to the map's issue id
  (`save_issue({ parentId: "<map issue id>" })`), carrying a `wayfinder:<type>` label
  (`wayfinder:research` / `wayfinder:prototype` / `wayfinder:grilling` / `wayfinder:task`).
  List a map's children with `linear.list_issues({ parentId: "<map issue id>" })`.
- **Blocking:** native `blockedBy` — `save_issue({ id: "<ticket>", blockedBy: ["<blocker>"] })`.
  This is a real upgrade over the old body-convention: it renders as a blocking indicator
  in Linear's own UI, which is the whole reason `wayfinder/SKILL.md` prefers "native
  blocking" when the tracker has it.
- **Frontier:** `linear.list_issues({ parentId: "<map issue id>", state: "Todo", assignee: null })`,
  then drop any that still have an open `blockedBy` edge (check via `get_issue({includeRelations: true})`
  on each candidate, since `list_issues` doesn't filter on relation state directly).
- **Claim:** `save_issue({ id: "<ticket>", assignee: "me" })` — Linear's assignee field
  *is* the claim, same semantics as before ("an open, unassigned ticket is unclaimed"),
  now enforced by a real field instead of a text convention.
- **Resolve:** `save_comment({ issueId: "<ticket>", body: "## Answer\n\n..." })`, then
  `save_issue({ id: "<ticket>", state: "Done" })`, then append a context pointer to the
  map issue's `## Decisions so far` via `save_issue({ id: "<map>", patch: [...] })` or a
  full `description` rewrite.

Wayfinder tickets are **decision** tickets, not build slices: no version prefix, no linked
release (`docs/GIT_WORKFLOW.md` § Version axis) — nothing ships from resolving one.

## Upstream PRs as a triage surface

**No.** `upstream` (`benedictbrady/prop-amm-challenge`) is read-only and its PR queue is
not ours to triage. `/triage` reads the Linear project's open issues, not upstream's PR
list. What upstream *merges* reaches us through `docs/GIT_WORKFLOW.md` § Upstream sync, as
a tracked issue.
