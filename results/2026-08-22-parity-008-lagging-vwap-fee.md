# parity-008-lagging-vwap-fee — 2026-08-22

- Commit: `7e539b6`
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

- Fast path: avg edge 501.57
- `prop-amm run`: avg edge 501.57, total edge 501565.63


