# compare-005b-elapsed-steps-divisor-fix-vs-005-vol-adaptive-cpmm-fee — 2026-08-22

- Commit: `001cf40`
- Segment: `screening`
- Simulations: 200
- Steps: 10000
- Execution path: native
- Candidate source: `strategies/005-vol-adaptive-cpmm-fee/lib.rs` with only the elapsed-steps
  divisor fix applied (`elapsed_sum: u64` at `[56..64]`, `STATE_END: 56 -> 64`,
  `fee_from_state` divides by `elapsed_sum.max(1)` instead of `count`), at the parent's own
  fitted `PARAMS` point (`FEE_LO=5, A_NUM=13, B_DEN=1265`) — run from a scratch copy at path
  `strategies/_scratch-005b-compare/lib.rs`, never committed (`004b`/`007`'s own precedent for
  a probe-gated evaluation, `strategies/005b-elapsed-steps-divisor-fix/NOTES.md`).

## Paired comparison

- Candidate: `strategies/_scratch-005b-compare/lib.rs` (avg edge 386.08)
- Reference: `strategies/005-vol-adaptive-cpmm-fee/lib.rs` (avg edge 405.38)
- Paired mean difference: -19.293945
- 95% CI: [-26.685351, -11.902540]


## Regime slices (docs/DESIGN.md §2.3, §5)

Bins reconstructed via `HyperparameterVariance::apply(&base, seed)`, equal-width thirds of each axis's own sampling range (a bench-level reporting choice, not a docs/DESIGN.md-specified boundary). Pooling every bin below reproduces the headline paired mean: pooled=-19.293945 vs headline (see "Paired comparison" above).

| regime | n | mean diff | 95% CI |
| --- | --- | --- | --- |
| fee=Low liq=Low sigma=Low | 9 | -29.412023 | [-48.664809, -10.159237] |
| fee=Low liq=Low sigma=Mid | 5 | -75.013887 | [-158.751623, 8.723849] |
| fee=Low liq=Low sigma=High | 9 | -91.294295 | [-132.114610, -50.473980] |
| fee=Low liq=Mid sigma=Low | 9 | 7.508532 | [-16.061187, 31.078251] |
| fee=Low liq=Mid sigma=Mid | 8 | -27.505637 | [-49.953531, -5.057744] |
| fee=Low liq=Mid sigma=High | 8 | -84.523648 | [-115.767134, -53.280162] |
| fee=Low liq=High sigma=Low | 5 | 29.775716 | [9.317850, 50.233583] |
| fee=Low liq=High sigma=Mid | 10 | -8.003877 | [-20.201688, 4.193934] |
| fee=Low liq=High sigma=High | 10 | -31.779236 | [-58.726049, -4.832424] |
| fee=Mid liq=Low sigma=Low | 11 | -29.718504 | [-43.383685, -16.053323] |
| fee=Mid liq=Low sigma=Mid | 8 | -45.706214 | [-95.728894, 4.316466] |
| fee=Mid liq=Low sigma=High | 4 | -92.750443 | [-240.331376, 54.830489] |
| fee=Mid liq=Mid sigma=Low | 2 | 6.887636 | [-167.870068, 181.645339] |
| fee=Mid liq=Mid sigma=Mid | 7 | 31.699947 | [-2.136841, 65.536735] |
| fee=Mid liq=Mid sigma=High | 9 | -21.775442 | [-46.314098, 2.763213] |
| fee=Mid liq=High sigma=Low | 11 | 35.175365 | [-11.734455, 82.085185] |
| fee=Mid liq=High sigma=Mid | 5 | 49.355185 | [-24.733757, 123.444127] |
| fee=Mid liq=High sigma=High | 3 | 36.613323 | [-47.619080, 120.845727] |
| fee=High liq=Low sigma=Low | 7 | -39.223794 | [-77.640329, -0.807260] |
| fee=High liq=Low sigma=Mid | 8 | -24.002696 | [-53.173604, 5.168211] |
| fee=High liq=Low sigma=High | 8 | -25.604232 | [-63.354363, 12.145898] |
| fee=High liq=Mid sigma=Low | 6 | -26.225703 | [-69.868857, 17.417451] |
| fee=High liq=Mid sigma=Mid | 8 | -39.752945 | [-76.767656, -2.738234] |
| fee=High liq=Mid sigma=High | 11 | 7.855500 | [-13.068341, 28.779341] |
| fee=High liq=High sigma=Low | 6 | -25.792206 | [-40.292064, -11.292349] |
| fee=High liq=High sigma=Mid | 7 | -11.805839 | [-35.026845, 11.415167] |
| fee=High liq=High sigma=High | 6 | 43.057482 | [17.149400, 68.965564] |


