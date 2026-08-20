# `.scratch/` — the issue tracker

This directory **is** the issue tracker for this repo. It is committed, not scratch space
in the throwaway sense: the name comes from the upstream skills convention.

- Format, naming, id allocation, lifecycle, and the wayfinder layout:
  **`docs/agents/issue-tracker.md`**.
- Issue body structure: **`docs/agents/issue-template.md`**.
- Triage vocabulary: **`docs/agents/triage-labels.md`**.

## The two rules that are easy to get wrong

1. **`NEXT_ID` is the id allocator.** Read it, use that number, write back the increment —
   in the **same commit** that creates the issue file. Ids are repo-global, monotonic, never
   reused, never renumbered: branch names and PR titles point at them forever.
2. **State changes are commits.** `State: In Progress` and `State: In Review` ride the
   feature branch. `State: Done` cannot — the PR is already merged by then — so it lands on
   the resolved base during post-merge cleanup, in the same session.

Nothing here is validated by tooling. Every guarantee in the workflow rests on an agent
refusing to start when a signal is missing, rather than on a tracker rejecting bad input.
