# compare-008-lagging-vwap-fee-vs-003b-wider-band-deeper-book — 2026-08-22

- Commit: `77bef05`
- Segment: `observation`
- Simulations: 1000
- Steps: 10000
- Execution path: native

## Paired comparison

- Candidate: `strategies/008-lagging-vwap-fee/lib.rs` (avg edge 501.57)
- Reference: `strategies/003b-wider-band-deeper-book/lib.rs` (avg edge 446.60)
- Paired mean difference: 54.963301
- 95% CI: [50.877495, 59.049108]


## Regime slices (docs/DESIGN.md §2.3, §5)

Bins reconstructed via `HyperparameterVariance::apply(&base, seed)`, equal-width thirds of each axis's own sampling range (a bench-level reporting choice, not a docs/DESIGN.md-specified boundary). Pooling every bin below reproduces the headline paired mean: pooled=54.963301 vs headline (see "Paired comparison" above).

| regime | n | mean diff | 95% CI |
| --- | --- | --- | --- |
| fee=Low liq=Low sigma=Low | 38 | 88.570392 | [54.210082, 122.930703] |
| fee=Low liq=Low sigma=Mid | 34 | 65.486463 | [46.129444, 84.843481] |
| fee=Low liq=Low sigma=High | 36 | 188.234546 | [155.865731, 220.603361] |
| fee=Low liq=Mid sigma=Low | 37 | 22.295025 | [15.386297, 29.203752] |
| fee=Low liq=Mid sigma=Mid | 33 | 33.266382 | [21.124774, 45.407990] |
| fee=Low liq=Mid sigma=High | 37 | 70.600030 | [56.504822, 84.695237] |
| fee=Low liq=High sigma=Low | 40 | 37.749626 | [31.628769, 43.870484] |
| fee=Low liq=High sigma=Mid | 35 | 26.164059 | [20.359862, 31.968256] |
| fee=Low liq=High sigma=High | 33 | 69.425641 | [53.906446, 84.944836] |
| fee=Mid liq=Low sigma=Low | 46 | 93.155380 | [75.336464, 110.974296] |
| fee=Mid liq=Low sigma=Mid | 32 | 79.630474 | [58.810579, 100.450370] |
| fee=Mid liq=Low sigma=High | 44 | 115.674090 | [92.767285, 138.580895] |
| fee=Mid liq=Mid sigma=Low | 42 | 39.248399 | [27.096132, 51.400667] |
| fee=Mid liq=Mid sigma=Mid | 35 | 38.711471 | [27.845320, 49.577623] |
| fee=Mid liq=Mid sigma=High | 28 | 47.176129 | [34.519674, 59.832583] |
| fee=Mid liq=High sigma=Low | 37 | 31.023658 | [22.120278, 39.927038] |
| fee=Mid liq=High sigma=Mid | 46 | 19.156801 | [12.342442, 25.971160] |
| fee=Mid liq=High sigma=High | 46 | 27.007012 | [16.267095, 37.746930] |
| fee=High liq=Low sigma=Low | 35 | 50.646186 | [29.286812, 72.005560] |
| fee=High liq=Low sigma=Mid | 30 | 69.487330 | [47.972915, 91.001746] |
| fee=High liq=Low sigma=High | 31 | 125.522865 | [100.722332, 150.323398] |
| fee=High liq=Mid sigma=Low | 44 | 49.210726 | [32.792951, 65.628501] |
| fee=High liq=Mid sigma=Mid | 45 | 32.360464 | [20.375014, 44.345914] |
| fee=High liq=Mid sigma=High | 33 | 58.248001 | [33.311297, 83.184705] |
| fee=High liq=High sigma=Low | 31 | 0.070449 | [-4.262055, 4.402954] |
| fee=High liq=High sigma=Mid | 37 | 8.932866 | [3.902426, 13.963306] |
| fee=High liq=High sigma=High | 35 | 5.699455 | [-6.494552, 17.893462] |


