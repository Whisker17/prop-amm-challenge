# Triage Labels

The skills speak in terms of five canonical triage roles. This file maps those roles to the
actual strings used in this repo's issue tracker — which is
[Linear](./issue-tracker.md) (team `Whisker-Personal`), so a "label" is a real Linear
label object attached to the issue, not a value on a text line.

| Label in mattpocock/skills | Linear label        | Meaning                                  |
| -------------------------- | ------------------- | ---------------------------------------- |
| `needs-triage`             | `needs-triage`      | Maintainer needs to evaluate this issue  |
| `needs-info`                | `needs-info`        | Waiting on reporter for more information |
| `ready-for-agent`          | `ready-for-agent`   | Fully specified, ready for an AFK agent  |
| `ready-for-human`          | `ready-for-human`   | Requires human implementation            |
| `wontfix`                   | `wontfix`           | Will not be actioned                     |

All five already exist as labels on the `Whisker-Personal` team. When a skill mentions a
role ("apply the AFK-ready triage label"), attach the corresponding label via
`linear.save_issue({ id, labels: [...] })` — the triage role is one value in that array,
not the whole set; keep any type label (below) alongside it, since `labels` replaces the
full set on every call.

Lifecycle is tracked **separately** on the issue's `state` field (`Todo` → `In Progress` →
`In Review` → `Done`). The triage label says *who* should do the work; `state` says *how
far along* it is. Neither implies the other: a `ready-for-human` issue can sit in
`In Progress`, and a `ready-for-agent` issue can sit in `Todo` for weeks.

Type labels are a third, orthogonal axis, also Linear labels: `bug`, `feature`,
`research`, `chore`, `hotfix`, `upstream-sync`. `research`, `chore`, and `hotfix` already
exist lowercase on this team, matching this convention exactly. `Bug` and `Feature` also
exist, but **capitalized** — generic workspace defaults, not created for this lowercase
convention — so don't reuse them; create lowercase `bug` and `feature` (and
`upstream-sync`, which doesn't exist in any casing yet) with
`linear.create_issue_label({ name, team: "Whisker-Personal" })` the first time one is
needed.

## The two type labels that change git behaviour

Routing labels, not triage roles — but they are the reason a label can be load-bearing
here (`docs/GIT_WORKFLOW.md` § Resolving the base branch):

- **`hotfix`** — branch off `origin/main`, PR into `main`. Apply it only when production is
  actually broken *and* `dev` holds work that must not ship yet. **In this repo that is
  currently hypothetical:** there is no deployment, so "production" means at most a
  submitted `lib.rs`. If everything on `dev` is shippable, the fix rides a normal release
  instead.
- **`upstream-sync`** — branch off `origin/dev`, PR into `dev`, then fan out
  (`docs/GIT_WORKFLOW.md` § Upstream sync). The diff must be a merge of `upstream/main`
  touching upstream-owned paths only; mixing our own changes into it makes the base
  resolution ambiguous, and the rule is to refuse rather than guess.

Agents **must resolve the base before creating the worktree** — never assume `dev`.

## High-risk-path extra caution (this repo)

`docs/GIT_WORKFLOW.md` § High-risk paths declares **none** for this repo, so there is no
path that is automatically barred from `ready-for-agent` here.

Two judgement calls survive that anyway, and they are about *measurement integrity*, not
safety:

- An issue that would edit an **upstream-owned path** (`AGENTS.md` § Architecture) outside
  the `upstream-sync` lane should be `ready-for-human` — the right answer is usually that
  the issue is misfiled, and an agent will happily implement it instead of noticing.
- An issue that changes **how edge is measured** rather than how the strategy prices —
  `crates/sim/`, `crates/shared/src/config.rs` — should be `ready-for-human` for the same
  reason: it silently invalidates every number recorded before it.

If a genuine high-risk path is ever declared, add it to that section, and route issues
touching it to `ready-for-human` by default from then on.
