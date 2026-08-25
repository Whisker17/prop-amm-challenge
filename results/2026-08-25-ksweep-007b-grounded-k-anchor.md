# ksweep-007b-grounded-k-anchor — 2026-08-25

- Commit: `231dd58`
- Segment: `screening`
- Simulations: 200 per cell
- Steps: 10000
- Execution path: native (fast path)
- Fitted fee held fixed: `FEE_BPS* = 66` (the 300-point search's own winner, not 66-by-default)

Colleague-facing k-sweep (the issue's exact 12-point set, denser at small `k`; not a
strict geometric sequence). Reporting only — not a decision input (docs/DESIGN.md §2.2).
Each interior cell is a genuine degenerate-range `bench fit` (ranges
collapsed to `MIN==MAX` at that `(K_BPS, 66)`, report on, no `--max-points` / `--no-report`).
The `k = 1` cell is the search winner, reproduced by the same method.

This table **did not** change the committed point. Search-winner screening avg edge:
**386.052462**. No sweep cell beat that, so §8 lesson 7 did not fire.

## k-sweep at `FEE_BPS = 66`

| `K_BPS` | `k` | screening avg edge | vs search winner |
| --- | --- | ---: | ---: |
| 25 | 0.0025 | -20011.897038 | −20397.95 |
| 50 | 0.005 | -20013.225738 | −20399.28 |
| 100 | 0.01 | -20012.685729 | −20398.74 |
| 200 | 0.02 | -20011.699839 | −20397.75 |
| 400 | 0.04 | -20012.466025 | −20398.52 |
| 800 | 0.08 | -20012.695464 | −20398.75 |
| 1500 | 0.15 | -20015.828874 | −20401.88 |
| 2500 | 0.25 | -20028.534315 | −20414.59 |
| 4000 | 0.40 | -19998.477352 | −20384.53 |
| 6000 | 0.60 | 133.449465 | −252.60 |
| 8000 | 0.80 | 323.408980 | −62.64 |
| 10000 | 1.00 | 386.052462 | 0 (search winner) |

## `k = 0` (`K_BPS = 0`) — not searched

`prop-amm validate` FAIL (shape-fatal; source's own `k=0` branch is a hard flat cap
`output = min(i*delta, V1)`):

```
Error: FAIL: Monotonicity violation (sell side). size=200 output=9934000000000 <= prev_output=9934000000000
```

Scored `Invalid`, not an edge number. Reproduced on both `007b` and unmodified parent `007`.

## Lesson 7

No sweep cell's screening edge beat the search winner. Committed point remains the search's
own re-eval winner `(K_BPS=10_000, FEE_BPS=66)`. No named-probe follow-up.
