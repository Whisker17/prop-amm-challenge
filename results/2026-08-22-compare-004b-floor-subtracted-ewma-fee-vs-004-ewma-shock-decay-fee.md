# compare-004b-floor-subtracted-ewma-fee-vs-004-ewma-shock-decay-fee — 2026-08-22

- Commit: `edf9b62`
- Segment: `observation`
- Simulations: 1000
- Steps: 10000
- Execution path: native

## Paired comparison

- Candidate: `strategies/004b-floor-subtracted-ewma-fee/lib.rs` (avg edge 443.75)
- Reference: `strategies/004-ewma-shock-decay-fee/lib.rs` (avg edge 443.75)
- Paired mean difference: 0.000000
- 95% CI: [0.000000, 0.000000]


## Regime slices (docs/DESIGN.md §2.3, §5)

Bins reconstructed via `HyperparameterVariance::apply(&base, seed)`, equal-width thirds of each axis's own sampling range (a bench-level reporting choice, not a docs/DESIGN.md-specified boundary). Pooling every bin below reproduces the headline paired mean: pooled=0.000000 vs headline (see "Paired comparison" above).

| regime | n | mean diff | 95% CI |
| --- | --- | --- | --- |
| fee=Low liq=Low sigma=Low | 38 | 0.000000 | [0.000000, 0.000000] |
| fee=Low liq=Low sigma=Mid | 34 | 0.000000 | [0.000000, 0.000000] |
| fee=Low liq=Low sigma=High | 36 | 0.000000 | [0.000000, 0.000000] |
| fee=Low liq=Mid sigma=Low | 37 | 0.000000 | [0.000000, 0.000000] |
| fee=Low liq=Mid sigma=Mid | 33 | 0.000000 | [0.000000, 0.000000] |
| fee=Low liq=Mid sigma=High | 37 | 0.000000 | [0.000000, 0.000000] |
| fee=Low liq=High sigma=Low | 40 | 0.000000 | [0.000000, 0.000000] |
| fee=Low liq=High sigma=Mid | 35 | 0.000000 | [0.000000, 0.000000] |
| fee=Low liq=High sigma=High | 33 | 0.000000 | [0.000000, 0.000000] |
| fee=Mid liq=Low sigma=Low | 46 | 0.000000 | [0.000000, 0.000000] |
| fee=Mid liq=Low sigma=Mid | 32 | 0.000000 | [0.000000, 0.000000] |
| fee=Mid liq=Low sigma=High | 44 | 0.000000 | [0.000000, 0.000000] |
| fee=Mid liq=Mid sigma=Low | 42 | 0.000000 | [0.000000, 0.000000] |
| fee=Mid liq=Mid sigma=Mid | 35 | 0.000000 | [0.000000, 0.000000] |
| fee=Mid liq=Mid sigma=High | 28 | 0.000000 | [0.000000, 0.000000] |
| fee=Mid liq=High sigma=Low | 37 | 0.000000 | [0.000000, 0.000000] |
| fee=Mid liq=High sigma=Mid | 46 | 0.000000 | [0.000000, 0.000000] |
| fee=Mid liq=High sigma=High | 46 | 0.000000 | [0.000000, 0.000000] |
| fee=High liq=Low sigma=Low | 35 | 0.000000 | [0.000000, 0.000000] |
| fee=High liq=Low sigma=Mid | 30 | 0.000000 | [0.000000, 0.000000] |
| fee=High liq=Low sigma=High | 31 | 0.000000 | [0.000000, 0.000000] |
| fee=High liq=Mid sigma=Low | 44 | 0.000000 | [0.000000, 0.000000] |
| fee=High liq=Mid sigma=Mid | 45 | 0.000000 | [0.000000, 0.000000] |
| fee=High liq=Mid sigma=High | 33 | 0.000000 | [0.000000, 0.000000] |
| fee=High liq=High sigma=Low | 31 | 0.000000 | [0.000000, 0.000000] |
| fee=High liq=High sigma=Mid | 37 | 0.000000 | [0.000000, 0.000000] |
| fee=High liq=High sigma=High | 35 | 0.000000 | [0.000000, 0.000000] |


