# config/

Two different things live here, and only one of them is "config" in the usual sense.

## 1. `agent-roles.conf` — the agent runtime mapping

Shell-sourced by `scripts/agent-dispatch.sh`; explained in `docs/agents/runtime.md`. It is
the **single edit point** when a model generation turns over. Checked in, no secrets.

## 2. Runtime parameters (none yet)

Non-secret parameters of *our* code — sweep ranges, search grids, strategy tunables — go
here as checked-in TOML when the first one exists.

- **Secrets** would go in `.env` (never committed). This repo has none today: the simulator
  is local and deterministic and nothing calls a network service.
- **Parameters** go here, in TOML, checked in. They are decisions — every value should
  trace to `docs/DESIGN.md` §2 or be explicitly flagged as unvalidated.
- Loading is **typed and fail-fast**: one `serde` struct per config file, parsed at startup,
  with cross-field validation (e.g. `min < max`) in a constructor that returns `Result`. A
  bad config must kill the process with a clear error before any simulation runs — a sweep
  that silently ran on a default is worse than one that never started.
- Per-machine overrides use an untracked `<name>.local.toml` copy (gitignored), so checking
  out a release tag never clobbers local settings.

No loader code ships yet — write it when the first config file lands.

## What does *not* belong here

Simulation parameters owned by the **challenge** — per-step volatility range, normalizer
fee and liquidity sampling, step count, base reserves. Those live in
`crates/shared/src/config.rs`, are upstream-owned, and change only through
`docs/GIT_WORKFLOW.md` § Upstream sync. Copying them here to "make them configurable" is
how you end up optimising against a harness the grader does not run.
