out_of_competition: true

# ceiling-anchored-trade-triggered-observation-orbic-oracle-vs-001-cpmm-fee — 2026-08-23

This is the ceiling lane (WHI-1247, `ceilings/README.md`): an out-of-competition measurement of an oracle re-anchor the arbitrageur cannot front-run. Nothing on this page is submittable or ranked.

**Three honesty constraints bound what this number means** (WHI-1247 § Context): (1) it is a one-sided **lower bound** on what perfect price knowledge is worth — Orbic-with-a-spread is one member of the perfect-information class, not its maximum, so this number does not bound the remaining headroom above a stronger submission from above; (2) once the quote is accurate and spread, the arbitrageur mostly stops trading against it, so most of the number is `retail volume x captured spread x flow share(spread)` — the only genuinely non-closed-form content is the flow-share-vs-spread curve the router grants against the normalizer's own sampled fee/liquidity; (3) that content generalizes to *any* oracle-centered quoter and carries little content specific to the Orbic curve itself.

- Commit: `831284c`
- Segment: `observation`
- Simulations: 1000
- Steps: 10000
- Execution path: native (host-side, never BPF-compiled)
- Variant: anchored
- Cursor mode: trade-triggered
- Reference (0-line): `001-cpmm-fee`

## Fitted point

- concentration: 2.3300
- spread_bps: 103.0000
- Search budget spent: 184 (exhausted: false)
- Best avg edge on `screening`: 473.787933
- Invalid points during search: 110

## Edge vs the 0-line

| field | value |
|---|---|
| n | 1000 |
| mean edge diff (oracle - reference) | 87.234885 |
| std error | 3.172875 |
| 95% interval | [81.016163, 93.453606] |

## Trade-triggered cursor staleness (steps since last executed trade)

| aggregate | value |
|---|---|
| mean of per-sim means | 3.886 |
| median of per-sim means | 3.144 |
| p95 of per-sim means | 8.437 |
| max of per-sim p95 | 109.000 |
| max of per-sim max | 205.000 |

