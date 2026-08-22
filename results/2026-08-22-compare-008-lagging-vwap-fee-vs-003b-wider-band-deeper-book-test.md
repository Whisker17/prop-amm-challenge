# compare-008-lagging-vwap-fee-vs-003b-wider-band-deeper-book — 2026-08-22

- Commit: `77bef05`
- Segment: `test`
- Simulations: 1000
- Steps: 10000
- Execution path: native

## Paired comparison

- Candidate: `strategies/008-lagging-vwap-fee/lib.rs` (avg edge 530.83)
- Reference: `strategies/003b-wider-band-deeper-book/lib.rs` (avg edge 469.09)
- Paired mean difference: 61.743094
- 95% CI: [56.869632, 66.616555]


## Regime slices (docs/DESIGN.md §2.3, §5)

Bins reconstructed via `HyperparameterVariance::apply(&base, seed)`, equal-width thirds of each axis's own sampling range (a bench-level reporting choice, not a docs/DESIGN.md-specified boundary). Pooling every bin below reproduces the headline paired mean: pooled=61.743094 vs headline (see "Paired comparison" above).

| regime | n | mean diff | 95% CI |
| --- | --- | --- | --- |
| fee=Low liq=Low sigma=Low | 36 | 102.578525 | [66.022102, 139.134947] |
| fee=Low liq=Low sigma=Mid | 42 | 108.486739 | [86.959830, 130.013649] |
| fee=Low liq=Low sigma=High | 38 | 166.639652 | [140.972920, 192.306385] |
| fee=Low liq=Mid sigma=Low | 49 | 24.093147 | [18.832778, 29.353515] |
| fee=Low liq=Mid sigma=Mid | 37 | 23.714359 | [17.849594, 29.579123] |
| fee=Low liq=Mid sigma=High | 45 | 78.010784 | [66.520511, 89.501057] |
| fee=Low liq=High sigma=Low | 30 | 43.542555 | [34.515436, 52.569674] |
| fee=Low liq=High sigma=Mid | 29 | 27.601329 | [22.031053, 33.171605] |
| fee=Low liq=High sigma=High | 34 | 64.592754 | [42.363177, 86.822330] |
| fee=Mid liq=Low sigma=Low | 48 | 105.712574 | [71.076292, 140.348855] |
| fee=Mid liq=Low sigma=Mid | 42 | 87.292730 | [57.536367, 117.049094] |
| fee=Mid liq=Low sigma=High | 35 | 145.397160 | [113.006627, 177.787693] |
| fee=Mid liq=Mid sigma=Low | 37 | 31.332249 | [20.815349, 41.849150] |
| fee=Mid liq=Mid sigma=Mid | 33 | 37.151758 | [24.658255, 49.645260] |
| fee=Mid liq=Mid sigma=High | 33 | 59.333101 | [30.625536, 88.040666] |
| fee=Mid liq=High sigma=Low | 36 | 26.817804 | [16.834222, 36.801386] |
| fee=Mid liq=High sigma=Mid | 32 | 23.787080 | [12.572224, 35.001936] |
| fee=Mid liq=High sigma=High | 33 | 33.359552 | [18.679241, 48.039864] |
| fee=High liq=Low sigma=Low | 29 | 93.542250 | [56.900717, 130.183783] |
| fee=High liq=Low sigma=Mid | 38 | 75.629482 | [55.027940, 96.231024] |
| fee=High liq=Low sigma=High | 38 | 129.798596 | [91.624943, 167.972249] |
| fee=High liq=Mid sigma=Low | 42 | 45.737497 | [29.852481, 61.622514] |
| fee=High liq=Mid sigma=Mid | 41 | 42.778497 | [28.171953, 57.385041] |
| fee=High liq=Mid sigma=High | 28 | 49.882526 | [31.805808, 67.959245] |
| fee=High liq=High sigma=Low | 39 | 3.686192 | [-2.268370, 9.640754] |
| fee=High liq=High sigma=Mid | 31 | 7.198921 | [3.729233, 10.668608] |
| fee=High liq=High sigma=High | 45 | 7.387956 | [-2.916040, 17.691953] |


