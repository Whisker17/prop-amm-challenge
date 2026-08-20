# Issue Template

The canonical shape for every issue in this repo. Copy the skeleton in
[§ Copy-paste skeleton](#copy-paste-skeleton), fill each section, and delete the inline
guidance (the `> _italic_` hints). **All issue content is written in English.**

It pairs with:

- `docs/agents/issue-tracker.md` — where issues live (Linear), how ids are assigned, and
  the field mapping for everything that precedes the body below.
- `docs/agents/triage-labels.md` — the five canonical triage labels.
- `docs/DESIGN.md` — the PRD these issues implement; every issue should trace back to a
  section there.

---

## Principles

A good issue is a **self-contained unit of work**: a competent engineer (or an AFK agent)
can open it cold and know *what* to build, *why*, *where* in the codebase, what it
*depends on*, and *how to know it's done* — without asking a follow-up question.

1. **One issue = one deliverable.** If it needs "and" in the objective, split it.
2. **Concrete over abstract.** Name the files, functions, config keys, and
   `docs/DESIGN.md` section. Reference `docs/DESIGN.md §2.2`, not "the entry logic".
3. **Verifiable.** Every issue ends in acceptance criteria that are objectively
   checkable (a test passes, a log shows the expected decision, a value appears in the
   data store).
4. **Dependencies explicit.** State what blocks this and what this unblocks, so the
   work can be ordered and parallelized.
5. **Scoped.** Say what is *out* of scope as clearly as what is in.
6. **No new parameters without a source.** Anything that reads as a tunable parameter
   must cite where in `docs/DESIGN.md` §2 / `references/` it comes from, or be
   explicitly flagged as a new, unvalidated parameter.

---

## Title convention

```
[X.Y.Z] [Component] <imperative, specific description>
```

- **`[X.Y.Z]`** — the **version / Release** this issue ships in (e.g. `[0.2.0]`).
  This is the **primary git-routing signal** (`docs/GIT_WORKFLOW.md` § Resolving
  the base branch). An implementing agent that cannot read a version prefix, or
  whose prefix disagrees with Linear's linked release entity, **refuses to start**.
  Omit the prefix only for repo-wide governance (the carve-out file list in
  that section) — those issues target `dev` and have no version. A `hotfix`-labelled
  issue keeps a prefix too, using the four-segment hotfix version (`[0.1.5.1]`, per
  `docs/GIT_WORKFLOW.md` § Version axis), but it routes off the **label**, not the
  prefix.
- **`[Component]`** — the module this touches, per `docs/DESIGN.md` §4.2, plus
  cross-cutting tags `[Config]`, `[Infra]`, `[Docs]`, `[CI]`.
- **Description** — short, specific, and imperative. Prefer
  `Add request dedup to the scheduler` over `Scheduler stuff`.

Do **not** put the milestone in the title. Milestone and Release can both render
as `0.2.0` and have already disagreed in practice (title `[0.2.0]`, milestone
`0.3.0`). Milestone is a `docs/DESIGN.md` §6 concept, referenced in `## Context`; the
title prefix is the version.

---

## Fields (Linear issue fields, not a markdown block)

The tracker is Linear (`docs/agents/issue-tracker.md`), so what used to be a block of
`Key: value` lines is now a set of real fields on the issue object, set via
`linear.save_issue({...})`. The table below is the same mapping as
`docs/agents/issue-tracker.md` § Field mapping, kept here for how to *set* each one when
authoring a new issue:

| Field             | How to set it                                                            |
| ------------- | ------------------------------------------------------------------------ |
| Identifier (`WHI-NNNN`) | Assigned by Linear on create. Never hand-allocated.                |
| Linked release | `save_issue({ addReleases: ["<release>"] })`. Required-by-convention for every version-scoped issue; must match the `[X.Y.Z]` title prefix. **A missing linked release blocks implementation** — the agent refuses rather than guessing `dev`. Linear validates nothing here; enforcement is the refusal in `docs/GIT_WORKFLOW.md`. Empty for repo-wide governance and `upstream-sync` (no version prefix). |
| Milestone (doc-only) | Capability stage (`docs/DESIGN.md` §6), referenced in `## Context`, not a Linear field. Orthogonal to the linked release. Do not use it to express the version or to route git. |
| `priority`   | `Urgent` / `High` / `Medium` / `Low` — see the table below.               |
| Triage `label` | One of the five roles in `triage-labels.md`, set as a Linear label.     |
| Type `label`s | `bug`, `feature`, `research`, `chore`, `hotfix`, `upstream-sync`. `hotfix` and `upstream-sync` change the git base branch — see `triage-labels.md`. |
| `state`      | Lifecycle: `Todo` / `In Progress` / `In Review` / `Done` / `Canceled`.    |
| `assignee`   | Set when claimed; unset in the backlog.                                  |
| `blockedBy` / `blocks` | Native relations. Both directions must be set — nothing derives the reverse edge for you. |

### Priority guide

| Priority   | Use when…                                                                      |
| ---------- | ------------------------------------------------------------------------------ |
| **Urgent** | Blocks a milestone, or is a safety item on a declared high-risk path (`docs/GIT_WORKFLOW.md` § High-risk paths). Do first. |
| **High**   | Core deliverable of the milestone; needed for it to be "done."                 |
| **Medium** | Valuable but not blocking; can slip a milestone without derailing it.          |
| **Low**    | Nice-to-have, polish, or opportunistic cleanup.                                |

---

## Body sections

Fill the sections below in order. Sections marked _(optional)_ may be dropped when they
add nothing; keep the rest even if brief.

### `## Objective`
One or two sentences: what this issue delivers and why it matters.

### `## Context` _(optional)_
Background a newcomer needs: the relevant `docs/DESIGN.md` section, prior research link,
or an external platform fact. Skip if the Objective is fully self-explanatory.

### `## Blocked By` / `## Blocks`
Dependency graph. List issue identifiers (e.g. `WHI-1042`) and a short reason. These
sections are the human-readable prose; the machine-readable edges are the native
`blockedBy` / `blocks` relations set via `save_issue`, and the two must agree. Use
`None (entry point)` when there are no blockers.

### `## Implementation`
The plan of record. Numbered steps, each anchored to a concrete file/module. Include:
- Exact file paths to create or modify.
- Function signatures, config keys, schema, or API payload shapes where they pin the
  design.
- Code blocks for anything the implementer must follow verbatim.
This is the section that makes an issue AFK-ready — be generous with specifics.

### `## Out of scope` _(optional)_
Explicitly list what this issue does **not** cover, to prevent scope creep and to signal
where follow-up issues pick up.

### `## Acceptance criteria`
A checklist of objectively verifiable conditions. Each item is a fact someone can
confirm: a passing test, a log line, a stored record, a delivered notification. If you
can't check it, it's not a criterion — rewrite it.

### `## Testing / Verification` _(optional)_
How to prove the criteria hold: exact commands, fixtures to replay, or manual steps and
expected output. Merge into Acceptance criteria for small issues.

### `## References` _(optional)_
Links to `docs/DESIGN.md` sections, `docs/references/`, prior issues, or external docs.
Bare URLs are fine.

---

## Copy-paste skeleton

Create with `linear.save_issue`, title and fields as shown, `description` holding the
body sections below (the `Id:`/`State:`/… lines are gone — those are now the call's
fields, not text in the body):

```
linear.save_issue({
  title: "[X.Y.Z] [Component] <imperative, specific description>",
  team: "Whisker-Personal",
  project: "Prop AMM Challenge — strategy layer",
  state: "Todo",
  labels: ["ready-for-agent", "feature"],
  priority: 3,          // Medium
  addReleases: ["X.Y.Z"],
  blockedBy: [],        // or the blocking issue ids
  description: `
## Objective
> _One or two sentences: what this delivers and why. State the success outcome._

## Context
> _(optional) Background, docs/DESIGN.md section, platform facts._

## Blocked By
> _Issue ids + one-line reason, or `None (entry point)`. Prose mirror of the `blockedBy` field set on the call above — keep them in agreement._

## Blocks
> _Issue ids this unblocks, or `None`. Prose mirror of the `blocks` field._

## Implementation
> _Numbered, file-anchored plan. Signatures, config keys, schema, code blocks._
1.
2.
3.

## Out of scope
> _(optional) What this issue deliberately does not cover._

## Acceptance criteria
- [ ]
- [ ]
- [ ]

## Testing / Verification
> _(optional) Exact commands / fixtures / manual steps + expected output._

## References
> _(optional) Links to docs/DESIGN.md sections, references/, prior issues._
`,
})
```

---

## Appendix: Milestones

Issues carry an `[X.Y.Z]` title prefix because they belong to a **Release** (the
git-routing signal, a linked Linear release entity — `docs/agents/issue-tracker.md` §
Field mapping). They may also sit under a **milestone** (capability stage), per
`docs/DESIGN.md` §6 — referenced in `## Context`, not a Linear field (§ Field mapping).
Keep the milestone list in §6; if the version set changes, that is the linked release, not
this file.
