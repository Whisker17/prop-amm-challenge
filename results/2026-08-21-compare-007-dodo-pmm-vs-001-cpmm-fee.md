# compare-007-dodo-pmm-vs-001-cpmm-fee — 2026-08-21

- Commit: `5851484+dirty`
- Segment: `train`
- Simulations: 1000
- Steps: 10000
- Execution path: native

## Paired comparison

- Candidate: `strategies/007-dodo-pmm/lib.rs` (avg edge 407.55)
- Reference: `strategies/001-cpmm-fee/lib.rs` (avg edge 406.14)
- Paired mean difference: 1.401555
- 95% CI: [1.246297, 1.556812]


## Regime slices (docs/DESIGN.md §2.3, §5)

Bins reconstructed via `HyperparameterVariance::apply(&base, seed)`, equal-width thirds of each axis's own sampling range (a bench-level reporting choice, not a docs/DESIGN.md-specified boundary). Pooling every bin below reproduces the headline paired mean: pooled=1.401555 vs headline (see "Paired comparison" above).

| regime | n | mean diff | 95% CI |
| --- | --- | --- | --- |
| fee=Low liq=Low sigma=Low | 41 | 2.583765 | [1.355832, 3.811699] |
| fee=Low liq=Low sigma=Mid | 26 | 2.538156 | [1.063811, 4.012502] |
| fee=Low liq=Low sigma=High | 46 | 3.247665 | [2.480727, 4.014604] |
| fee=Low liq=Mid sigma=Low | 47 | 0.737264 | [0.373402, 1.101126] |
| fee=Low liq=Mid sigma=Mid | 35 | 1.207221 | [0.820935, 1.593507] |
| fee=Low liq=Mid sigma=High | 34 | 1.254510 | [0.867999, 1.641022] |
| fee=Low liq=High sigma=Low | 32 | 0.626685 | [0.202828, 1.050541] |
| fee=Low liq=High sigma=Mid | 47 | 0.631750 | [0.383239, 0.880260] |
| fee=Low liq=High sigma=High | 33 | 1.220935 | [0.728424, 1.713447] |
| fee=Mid liq=Low sigma=Low | 48 | 1.675052 | [0.969428, 2.380676] |
| fee=Mid liq=Low sigma=Mid | 38 | 3.293941 | [2.184233, 4.403649] |
| fee=Mid liq=Low sigma=High | 32 | 3.270178 | [2.280969, 4.259387] |
| fee=Mid liq=Mid sigma=Low | 28 | 1.375240 | [0.873021, 1.877459] |
| fee=Mid liq=Mid sigma=Mid | 37 | 1.240136 | [0.708817, 1.771455] |
| fee=Mid liq=Mid sigma=High | 32 | 1.041937 | [0.423509, 1.660366] |
| fee=Mid liq=High sigma=Low | 45 | 0.572372 | [0.241821, 0.902923] |
| fee=Mid liq=High sigma=Mid | 31 | 0.890351 | [0.578637, 1.202065] |
| fee=Mid liq=High sigma=High | 27 | 0.737395 | [0.321466, 1.153324] |
| fee=High liq=Low sigma=Low | 34 | 1.996604 | [0.963339, 3.029869] |
| fee=High liq=Low sigma=Mid | 37 | 2.350605 | [1.045675, 3.655534] |
| fee=High liq=Low sigma=High | 40 | 3.436644 | [2.481933, 4.391354] |
| fee=High liq=Mid sigma=Low | 40 | 0.337051 | [-0.241027, 0.915128] |
| fee=High liq=Mid sigma=Mid | 35 | 0.413948 | [-0.341280, 1.169176] |
| fee=High liq=Mid sigma=High | 39 | 0.587200 | [-0.581875, 1.756274] |
| fee=High liq=High sigma=Low | 33 | 0.047990 | [-0.208565, 0.304545] |
| fee=High liq=High sigma=Mid | 41 | 0.198335 | [-0.085886, 0.482555] |
| fee=High liq=High sigma=High | 42 | 0.393167 | [0.069829, 0.716505] |


