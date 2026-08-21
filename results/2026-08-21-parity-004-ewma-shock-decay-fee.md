# parity-004-ewma-shock-decay-fee — 2026-08-21

- Commit: `34b2406`
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

- Fast path: avg edge 443.75
- `prop-amm run`: avg edge 443.75, total edge 443746.27


