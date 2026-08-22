# compare-008-lagging-vwap-fee-vs-004-ewma-shock-decay-fee — 2026-08-22

- Commit: `7e539b6+dirty`
- Segment: `validation`
- Simulations: 1000
- Steps: 10000
- Execution path: native

## Paired comparison

- Candidate: `strategies/008-lagging-vwap-fee/lib.rs` (avg edge 503.91)
- Reference: `strategies/004-ewma-shock-decay-fee/lib.rs` (avg edge 446.30)
- Paired mean difference: 57.610368
- 95% CI: [53.209045, 62.011692]


## Regime slices (docs/DESIGN.md §2.3, §5)

Bins reconstructed via `HyperparameterVariance::apply(&base, seed)`, equal-width thirds of each axis's own sampling range (a bench-level reporting choice, not a docs/DESIGN.md-specified boundary). Pooling every bin below reproduces the headline paired mean: pooled=57.610368 vs headline (see "Paired comparison" above).

| regime | n | mean diff | 95% CI |
| --- | --- | --- | --- |
| fee=Low liq=Low sigma=Low | 49 | 84.542585 | [63.460473, 105.624697] |
| fee=Low liq=Low sigma=Mid | 45 | 90.155264 | [65.836402, 114.474126] |
| fee=Low liq=Low sigma=High | 30 | 127.898943 | [63.974438, 191.823448] |
| fee=Low liq=Mid sigma=Low | 43 | 46.851107 | [36.431206, 57.271009] |
| fee=Low liq=Mid sigma=Mid | 35 | 32.091848 | [21.658893, 42.524802] |
| fee=Low liq=Mid sigma=High | 38 | 42.976193 | [34.101103, 51.851282] |
| fee=Low liq=High sigma=Low | 35 | 64.351897 | [54.932466, 73.771328] |
| fee=Low liq=High sigma=Mid | 35 | 45.789604 | [37.301959, 54.277248] |
| fee=Low liq=High sigma=High | 43 | 52.243764 | [40.411720, 64.075807] |
| fee=Mid liq=Low sigma=Low | 37 | 117.193958 | [77.734422, 156.653494] |
| fee=Mid liq=Low sigma=Mid | 48 | 77.978357 | [57.087079, 98.869636] |
| fee=Mid liq=Low sigma=High | 35 | 101.534229 | [73.401019, 129.667440] |
| fee=Mid liq=Mid sigma=Low | 37 | 47.017042 | [31.236440, 62.797645] |
| fee=Mid liq=Mid sigma=Mid | 43 | 25.561824 | [16.520486, 34.603162] |
| fee=Mid liq=Mid sigma=High | 36 | 43.252299 | [29.378423, 57.126175] |
| fee=Mid liq=High sigma=Low | 31 | 68.856364 | [57.643936, 80.068792] |
| fee=Mid liq=High sigma=Mid | 34 | 51.599764 | [42.803395, 60.396133] |
| fee=Mid liq=High sigma=High | 40 | 56.042165 | [39.513934, 72.570397] |
| fee=High liq=Low sigma=Low | 29 | 97.441460 | [59.527037, 135.355882] |
| fee=High liq=Low sigma=Mid | 32 | 69.490553 | [36.291498, 102.689609] |
| fee=High liq=Low sigma=High | 23 | 96.272139 | [48.934212, 143.610067] |
| fee=High liq=Mid sigma=Low | 40 | 22.996707 | [11.147136, 34.846278] |
| fee=High liq=Mid sigma=Mid | 40 | 5.189596 | [-5.307265, 15.686456] |
| fee=High liq=Mid sigma=High | 39 | 16.678281 | [6.798931, 26.557632] |
| fee=High liq=High sigma=Low | 38 | 37.050026 | [26.065875, 48.034176] |
| fee=High liq=High sigma=Mid | 37 | 33.657947 | [22.321381, 44.994513] |
| fee=High liq=High sigma=High | 28 | 30.258646 | [21.954554, 38.562739] |


