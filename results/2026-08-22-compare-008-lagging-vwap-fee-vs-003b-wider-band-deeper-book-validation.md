# compare-008-lagging-vwap-fee-vs-003b-wider-band-deeper-book — 2026-08-22

- Commit: `77bef05`
- Segment: `validation`
- Simulations: 1000
- Steps: 10000
- Execution path: native

## Paired comparison

- Candidate: `strategies/008-lagging-vwap-fee/lib.rs` (avg edge 503.91)
- Reference: `strategies/003b-wider-band-deeper-book/lib.rs` (avg edge 447.22)
- Paired mean difference: 56.682838
- 95% CI: [52.176061, 61.189614]


## Regime slices (docs/DESIGN.md §2.3, §5)

Bins reconstructed via `HyperparameterVariance::apply(&base, seed)`, equal-width thirds of each axis's own sampling range (a bench-level reporting choice, not a docs/DESIGN.md-specified boundary). Pooling every bin below reproduces the headline paired mean: pooled=56.682838 vs headline (see "Paired comparison" above).

| regime | n | mean diff | 95% CI |
| --- | --- | --- | --- |
| fee=Low liq=Low sigma=Low | 49 | 71.443561 | [55.594692, 87.292429] |
| fee=Low liq=Low sigma=Mid | 45 | 96.595125 | [72.349469, 120.840780] |
| fee=Low liq=Low sigma=High | 30 | 171.054146 | [96.464257, 245.644035] |
| fee=Low liq=Mid sigma=Low | 43 | 25.059351 | [16.175036, 33.943666] |
| fee=Low liq=Mid sigma=Mid | 35 | 27.308693 | [18.249298, 36.368087] |
| fee=Low liq=Mid sigma=High | 38 | 76.272558 | [63.927628, 88.617488] |
| fee=Low liq=High sigma=Low | 35 | 45.590837 | [39.600101, 51.581572] |
| fee=Low liq=High sigma=Mid | 35 | 31.306558 | [25.834399, 36.778717] |
| fee=Low liq=High sigma=High | 43 | 76.128792 | [60.142499, 92.115085] |
| fee=Mid liq=Low sigma=Low | 37 | 90.533352 | [61.265859, 119.800845] |
| fee=Mid liq=Low sigma=Mid | 48 | 88.631649 | [70.488353, 106.774944] |
| fee=Mid liq=Low sigma=High | 35 | 139.325512 | [113.766171, 164.884853] |
| fee=Mid liq=Mid sigma=Low | 37 | 38.622484 | [27.533469, 49.711498] |
| fee=Mid liq=Mid sigma=Mid | 43 | 31.961160 | [23.112235, 40.810085] |
| fee=Mid liq=Mid sigma=High | 36 | 59.706052 | [38.903534, 80.508571] |
| fee=Mid liq=High sigma=Low | 31 | 24.152361 | [19.094338, 29.210384] |
| fee=Mid liq=High sigma=Mid | 34 | 29.222705 | [21.985237, 36.460174] |
| fee=Mid liq=High sigma=High | 40 | 45.795909 | [23.766672, 67.825147] |
| fee=High liq=Low sigma=Low | 29 | 69.742601 | [41.988071, 97.497131] |
| fee=High liq=Low sigma=Mid | 32 | 69.131283 | [39.978872, 98.283694] |
| fee=High liq=Low sigma=High | 23 | 120.977274 | [77.691519, 164.263029] |
| fee=High liq=Mid sigma=Low | 40 | 37.313820 | [25.771614, 48.856026] |
| fee=High liq=Mid sigma=Mid | 40 | 25.503860 | [16.601092, 34.406628] |
| fee=High liq=Mid sigma=High | 39 | 52.219801 | [39.075212, 65.364389] |
| fee=High liq=High sigma=Low | 38 | -1.780557 | [-5.805274, 2.244160] |
| fee=High liq=High sigma=Mid | 37 | 6.158515 | [2.046349, 10.270681] |
| fee=High liq=High sigma=High | 28 | 2.606551 | [-7.608796, 12.821898] |


