# parity-007-dodo-pmm — 2026-08-21

> **Traceability note (WHI-1219 round-3 review):** generated at commit `5851484` (a
> disposable scratch worktree, hence the generated `+dirty` suffix — same situation as
> `results/2026-08-20-compare-with-regime-slices.md`'s own note). `9bce50e` and this issue's
> round-3 fixes touched `strategies/007-dodo-pmm/lib.rs` again after this report was
> generated, but only in ways that provably don't change the committed `(K_BPS=10_000,
> FEE_BPS=66)` point's output — a variable rename, a helper extraction, a defensive guard
> unreachable at `k=1`, and clamping reserves already far under `RESERVE_CLAMP` — re-verified
> by re-running the containment-demonstration fit after each change and getting the identical
> `386.052462` / `407.545843` / `403.257399` screening/train/validation numbers every time.

- Commit: `5851484+dirty`
- Segment: `observation`
- Simulations: 1000
- Steps: 10000
- Execution path: native (fast path vs reference path)

## prop-amm validate

PASS

## Per-seed parity (fast path vs reference path)

- Segment: `observation`, n=1000
- Max observed relative diff: 0e0
- Tolerance: 1e-9
- Result: all seeds agree


## Aggregate parity (fast path vs `prop-amm run`)

- Fast path: avg edge 401.28
- `prop-amm run`: avg edge 401.28, total edge 401284.20


