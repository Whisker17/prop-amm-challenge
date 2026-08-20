# prop-amm-challenge — Design Document / PRD

> This file is the **spec of record** for this project. Every issue, architectural
> decision, and parameter traces back to a section here. It is produced by grilling the
> idea into shape (`/grill-me`) and formalizing the result (`/to-spec`) — do not skip
> straight to code with this document empty.
>
> Sections marked _(fill in)_ contain guidance for what belongs there. Replace the
> guidance as you write; delete sections that genuinely don't apply, but keep §7 and §8
> even when short — they are what stops an agent from re-inventing a rejected design or
> ignoring a known risk.
>
> **State of this document:** §4.1 and §4.2 are written, because they describe code that
> already exists (inherited from `upstream/main`). Everything else is still the template
> stub — **the strategy this repo exists to build has no spec yet.** Produce it with
> `/grill-me` then `/to-spec`; `AGENTS.md` §Status must be updated when it lands.

## 1. Background & Goals

### 1.1 Vision

_(fill in: the long-term product vision, one paragraph. What does this exist to do?)_

### 1.2 v1 Scope

_(fill in: exactly what v1 delivers. Cite prior research / validated data where
parameters are inherited rather than invented — agents are instructed not to re-derive
anything sourced here.)_

### 1.3 Non-goals (explicitly out of scope for v1)

_(fill in: what v1 deliberately does not do. Be as concrete as §1.2 — this list is what
prevents scope creep in tickets.)_

### 1.4 Success criteria

_(fill in: objectively checkable conditions under which v1 is "done and working".)_

## 2. Requirements / Specification

_(fill in: the functional spec. For a product, the feature-level requirements; for a
system, the behavioral rules. Number the subsections — issues will cite them as
`docs/DESIGN.md §2.x`. Any tunable parameter defined here must state where its value
comes from.)_

## 3. Cross-cutting Policies

_(fill in: policies that constrain every feature rather than belonging to one — e.g.
risk limits, security/privacy rules, compliance constraints, performance budgets. Delete
if the project truly has none.)_

## 4. System Architecture

### 4.1 Tech stack

Inherited from the challenge, not chosen by us — the grader runs upstream's harness, so
every entry here is a constraint rather than a decision.

| Piece | What | Why it is not ours to change |
| --- | --- | --- |
| Language | Rust 2021, Cargo workspace, `resolver = "2"` | The submission is a Rust source file the grader compiles. |
| Submission artifact | one `lib.rs` implementing `compute_swap`, built against `crates/submission-sdk` (`pinocchio`) | Fixed by the challenge's submission interface. |
| Execution | `solana_rbpf` (BPF) in `crates/executor`, or a natively compiled dylib via `libloading` for local speed | Both paths must agree; the native path exists only so local iteration is fast. |
| Simulation | `crates/sim`, parallelised with `rayon`; RNG is `rand` + `rand_pcg` (`Pcg64`) + `rand_distr` | Seeded and reproducible — the draw *order* in `crates/shared/src/config.rs` is load-bearing for seed reproducibility. |
| CLI | `crates/cli` (`prop-amm`), `clap` derive | `validate` / `run` / `build` are the local contract with the web submission. |
| Release profile | `lto = true`, `codegen-units = 1`, `opt-level = 3` | Benchmark numbers are only comparable under the same profile. |

Not present and deliberately so: no storage, no service, no deploy target. The "deploy" of
this project is pasting a `lib.rs` into the web UI.

_(fill in when the strategy layer lands: any library we add for our own optimisation /
search work, with a one-line reason each.)_

### 4.2 Module layout

`AGENTS.md` §Architecture mirrors this section — keep them in sync. **(u)** marks
upstream-owned code: it changes only through `docs/GIT_WORKFLOW.md` § Upstream sync, and
editing it in a feature PR gets silently reverted by the next sync.

```text
crates/
├── shared/            (u) sim config + per-sim parameter sampling, normalizer CPMM
│                          reference, swap instruction ABI
├── executor/          (u) run a submission behind one interface: BPF (solana_rbpf) or
│                          native dylib
├── sim/               (u) the simulation itself — price process, arbitrageur, retail
│                          flow, order router, curve shape checks, engine loop.
│                          This is what produces the edge number.
├── cli/               (u) the `prop-amm` binary: validate / run / build / compile
└── submission-sdk/    (u) the pinocchio surface a submission's lib.rs links against
programs/
├── starter/           (u) BPF, outside the workspace — what a submission is copied from
└── normalizer/        (u) BPF, outside the workspace — the benchmark CPMM
```

**Our own code has no home yet.** That is an open §4.2 decision, not an oversight:

_(fill in: where the candidate `compute_swap` source lives, whether the strategy is a
crate in this workspace or a standalone file the CLI compiles, and where sweep/search
tooling lives. Until this is decided, keep candidates out of `crates/` so nothing is
mistaken for upstream code — `AGENTS.md` §Architecture states the same rule.)_

### 4.3 Key interfaces

_(fill in: the seams that decouple modules — protocols, traits, API contracts. Note
which are load-bearing ("strategy code depends only on this interface") and which are
deliberately NOT abstracted yet — see §7.)_

### 4.4 Core flows

_(fill in: the main runtime sequences, end to end. A numbered walk-through per flow.)_

### 4.5 State & recovery

_(fill in: what state persists, where, and how the system behaves across restarts and
crashes. Delete if stateless.)_

## 5. Data & Observability

_(fill in: what gets logged/journaled, alerting channels and tiers, and how the data
supports post-hoc review. Delete subsections that don't apply.)_

## 6. Milestones

_(fill in: numbered milestones M1..Mn, each with a one-line success criterion. Issue
titles carry the `[X.Y.Z]` version prefix, not a milestone tag — a milestone is tracker
metadata (which capability stage), and never routes git. See
`docs/agents/issue-template.md` § Title convention and `docs/GIT_WORKFLOW.md`
§ Version axis.)_

## 7. Rejected Alternatives

_(fill in: every significant design option that was considered and rejected, with the
reason. This section exists so reviewers and agents don't re-propose them. An agent whose
output contradicts an entry here must flag it explicitly rather than silently override —
see `docs/agents/domain.md`.)_

## 8. Known Risks & Open Questions

_(fill in: what could sink this design, and what remains undecided. Reviewers are
invited to attack this list. Move items out as they get resolved — resolved decisions
made after v1 ships go to `docs/adr/`.)_
