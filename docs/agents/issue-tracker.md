# Issue tracker: Linear

Issues and specs (you may know a spec as a PRD) for this repo live in **Linear** — team
`Whisker-Personal`, project **"Prop AMM Challenge — strategy layer"**. Ids are
`WHI-NNNN`, allocated by Linear itself when an issue is created; nothing here allocates
them by hand.

This replaces an earlier design where the tracker was markdown files committed under
`.scratch/`. That design's one structural advantage — "moving an issue's state is a
commit, so it lands in the same PR as the work" — does **not** carry over: Linear and git
are now two independently-updatable systems that can disagree. § Decisions below records
that explicitly, with the mitigation. `.scratch/` itself was deleted by WHI-1199 — see
§ What happened to `.scratch/` (and other non-carve-out stale references).

## Reading and writing issues

Reached through the `linear.*` tools exposed via the slim-tools MCP gateway
(`discover_tools` then `execute_code`), not a markdown file read.

- **Fetch an issue:** `linear.get_issue({id: "WHI-1196", includeReleases: true})`. Always
  pass `includeReleases: true` when the version cross-check matters (§ Release ↔ version
  binding) — it is not in the default response.
- **Create or update an issue:** `linear.save_issue({...})`. Omit `id` to create; pass it
  to update. One call sets title, description (or a targeted `patch` instead of the full
  text), state, assignee, labels, priority, `parentId`,
  `blocks`/`blockedBy`/`relatedTo`, and `addReleases`/`setReleases` — see the field mapping
  below for which of these replaces which piece of the old metadata block.
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
| `State:` | `state` (workflow status) | Team's states are `Backlog` / `Todo` / `In Progress` / `In Review` / `Done` / `Canceled` / `Duplicate`. The resolved-base git workflow below only ever moves an issue through `Todo` → `In Progress` → `In Review` → `Done` (or `Canceled`) — a 1:1 name match with the old lifecycle vocabulary. `Backlog` precedes that flow (`docs/GIT_WORKFLOW.md`'s own state-mapping table pairs it with `Todo` as "not started, no branch"; an issue leaves `Backlog` for `Todo` once linked to a release, § Version axis) and `Duplicate` is a terminal state outside it, same footing as `Canceled`. |
| `Status:` (triage role) | a **label** | `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix` already exist as labels for this team. An issue can carry other labels too — the triage role is just one of them, same as before it was "one value on a line." |
| `Release:` | the linked **release entity** | `save_issue({addReleases: ["<release id or slug>"]})` or `setReleases`. See § Release ↔ version binding — this is *not* the same thing as the project or a milestone. |
| `Labels:` (type) | Linear labels | See `docs/agents/triage-labels.md` (type labels: `bug`, `feature`, `research`, `chore`, `hotfix`, `upstream-sync`) for which already exist and how to create the rest. |
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
Deleted, eventually — but not by this PR (WHI-1196). `.scratch/` was **never actually
populated** — `NEXT_ID` was still `001`, no issue file was ever created, `releases/` held
only `.gitkeep`. There was no real *issue data* to lose by deleting it or to preserve by
archiving it. That did **not** mean `.scratch/` was harmless as-is: `.scratch/README.md`
still asserted "This directory **is** the issue tracker for this repo," which was false
and was exactly the kind of second authority Decision 1 was framed to avoid — it just
wasn't a *data* authority, since there was no issue content behind the claim. This PR left
it in place, stale text and all, because deleting or correcting it was outside its
carve-out-only scope; WHI-1199 later deleted `.scratch/` outright — see § What happened to
`.scratch/` (and other non-carve-out stale references).

**2. What replaces "state change is a commit"?**
Nothing does — this is accepted explicitly, not papered over. The old tracker's structural
guarantee was that an issue's `State:` line and the diff that changed it landed in the same
commit, so "the tracker is out of sync" and "the diff is wrong" were literally the same
failure. Linear and git are now two separate systems with no shared commit boundary; they
can drift. The mitigation is operational discipline, not a technical fix:
- Flip the Linear `state` as the **literal next action** at each git milestone — worktree
  created → `In Progress` *before* the first commit; PR opened → `In Review` immediately;
  PR merged → `Done` as soon as post-merge cleanup reaches that step (`AGENTS.md` §
  Post-merge cleanup / `docs/GIT_WORKFLOW.md` — the last step, after fan-out, not the
  first) — rather than batching every update to the end of a session. This minimizes the
  window where the two can disagree, it does not close it.
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

## What happened to `.scratch/` (and other non-carve-out stale references)

`.scratch/` was untouched by this PR (WHI-1196) — it touched only the governance carve-out
file list (`docs/GIT_WORKFLOW.md` § Repo-wide governance carve-out), and `.scratch/` was
not on that list, so deleting or correcting it in this PR would have violated this same
issue's own scope constraint ("the diff contains carve-out paths only"). Since it held no
real issue data (Decision 1), this didn't block anything at the time, but its `README.md`
was factually wrong until WHI-1199 — the follow-up chore issue this section anticipated —
deleted `.scratch/` outright (`b459d61`).

The same reasoning covered `docs/DEFERRED_ISSUES.md`, also outside the carve-out, which at
the time carried **two** affected entries: the `WHI-1192` entry described the tracker as
in-repo and said the naming inconsistency "resolves when WHI-1196 lands" (this PR is
WHI-1196); the "`Done` state flip cannot ride its own PR" entry's whole deferral reason —
"the alternative (an external tracker) is what we deliberately traded away" — was inverted
the moment this PR adopted an external tracker. Resolving either meant editing
`docs/DEFERRED_ISSUES.md`, which was not a carve-out path — WHI-1199 moved both entries to
*Resolved*.

**Why this wasn't recorded in `docs/DEFERRED_ISSUES.md` per the usual rule.** `AGENTS.md` §
Git workflow says a review finding left unfixed goes there *in this PR*. That rule
presumed the PR could touch that file; this one couldn't without breaking its own
carve-out-only constraint. Recording it here instead — the tracker-specific home for
exactly this situation — was the compensating move, not a skipped step.

This repo's live governance path (`AGENTS.md`, `docs/GIT_WORKFLOW.md`, this file,
`docs/agents/issue-template.md`, `docs/agents/triage-labels.md`, and
`.claude/skills/implement/SKILL.md`) no longer asserts the tracker is in-repo markdown, as
of this PR (WHI-1196). `.scratch/README.md` and `docs/DEFERRED_ISSUES.md` were known,
out-of-scope exceptions at the time (above), not overlooked ones — both were since closed
by WHI-1199. `.claude/skills/setup-matt-pocock-skills/`
was, and remains, a different case: its `issue-tracker-{local,github,gitlab}.md` files are
option templates copied into this very file when `/setup-matt-pocock-skills` runs, not live
guidance any skill reads — each says so explicitly rather than reading as a live claim
about this repo. Its `SKILL.md` (e.g. "Local markdown — issues live as files under
`.scratch/<feature>/` in this repo") is the same kind of template prose describing what
picking that option means, not a claim about which option is picked today. Separately,
`.claude/skills/{to-tickets,ask-matt,code-review}/SKILL.md` mention `.scratch/` only as
one branch of tracker-conditional guidance ("if local files… if a real tracker…"), never
asserting it as this repo's tracker — both left as-is.

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
Linear being unreachable (MCP/network failure) is a *different* failure from a signal
that was read and found missing — see `docs/GIT_WORKFLOW.md` § Version determination for
which one gets a fallback and which one refuses.

Governance issues and `upstream-sync` issues carry no version prefix and no linked
release — for them the version-scoped row does not apply at all. (WHI-1196 itself is an
example: no `[X.Y.Z]` prefix, no release entity.)

## Release records

The version axis needs a home too (`docs/GIT_WORKFLOW.md` § Version axis). One **release**
per version, on the "Prop-AMM-Challenge" **release pipeline** — a separate Linear object
from the "Prop AMM Challenge — strategy layer" **project** referenced everywhere else in
this doc; a pipeline holds releases, a project holds issues, and this repo happens to have
one of each with similar names:

- **Create/update:** `linear.save_release({ name: "0.1.0", version: "0.1.0", pipeline: "Prop-AMM-Challenge", commitSha: "..." })` — omit `id` to create, pass it to update (e.g. to backfill `commitSha` after tagging).
- **Read:** `linear.get_release({ id: "<release id or slug>", includeReleaseNotes: true })` or `linear.list_releases({ pipeline: "Prop-AMM-Challenge" })`.
- **Attach issues:** `linear.save_issue({ id: "WHI-NNNN", addReleases: ["<release id or slug>"] })`.

Fields that matter, same intent as the old per-file record:

- `stage` — the release pipeline stage (`Planned` → `Released`, this team's stages).
- `commitSha` — backfilled after tagging. This is the **commit** the tag points at, not
  the tag object — dereference explicitly: `git rev-parse 'v0.1.0^{commit}'` (a bare
  `git rev-parse v0.1.0` returns the tag object and will never match a branch head).
- `startDate` / `targetDate` / `completedAt` — the release's own dates.
- **Submitted / Score** — no dedicated Linear field for either. Record both as free text
  in the release's `description` (`save_release({id, description: "Submitted: 2026-08-20\nScore: 210.5"})`)
  the moment a `lib.rs` from that tag is submitted to the challenge UI and a score comes
  back — this is the whole reason the version axis exists (`docs/GIT_WORKFLOW.md` §
  Version axis): without it, a leaderboard number can't be tied back to a tree.

A version's linked issues (via `releases[]`) and each issue's title prefix are the same
fact written twice, same as before — now in two separately-editable fields rather than two
lines in one file. They must agree; when they don't, this is the same failure as § Release
↔ version binding disagreeing — **refuse and surface it**, don't pick a side. There is no
"linked release wins" rule: § Decisions #3 says the linked release is the authoritative
*signal to read*, not that it silently overrides a title prefix that disagrees with it.

## Wayfinding operations

Used by `/wayfinder`. Linear's native primitives replace what the old scheme hand-rolled
with map files, child-ticket files, and directory scans — the map/ticket/blocking/frontier
*concepts* from `wayfinder/SKILL.md` are unchanged, only where they physically live:

- **Map:** a Linear issue labelled `wayfinder:map`. Its body is the map body from
  `wayfinder/SKILL.md` (`## Destination` / `## Notes` / `## Decisions so far` / etc.).
  None of the `wayfinder:*` labels exist yet on this team (unlike the five triage labels,
  all of which already exist — `docs/agents/triage-labels.md`) — create them with
  `linear.create_issue_label({name, team: "Whisker-Personal"})` the first time a map
  is charted.
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
