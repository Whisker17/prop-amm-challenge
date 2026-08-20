# compare (with regime slices) — 2026-08-20

> **Filename note (WHI-1195):** this is a real `bench compare` run at commit `cdf54fe`
> (clean tree — the `bin compare` command's own commit-sha helper reported `+dirty` only
> because this report was generated from a disposable scratch copy with an unrelated file
> moved aside; the code itself was unmodified). It is *not* named `2026-08-20-compare.md`
> because that slot was already taken by WHI-1193's own compare run, committed before
> regime slicing existed — `report.rs`'s one-report-per-`(day, stage)` rule (docs/DESIGN.md
> §3.3: never silently overwrite committed evidence) means a second same-day `compare`
> report needs a distinct filename, not a second code path. This file exists specifically to
> satisfy WHI-1195's own "Report — extend `results/` with … the slice table" deliverable;
> `2026-08-20-compare.md` remains WHI-1193's original, untouched.

- Commit: `cdf54fe`
- Segment: `observation`
- Simulations: 1000
- Steps: 10000
- Execution path: native

## Paired comparison

- Candidate: `programs/starter/src/lib.rs` (avg edge 210.50)
- Reference: `strategies/000-normalizer/lib.rs` (avg edge 200.08)
- Paired mean difference: 10.413824
- 95% CI: [-7.909101, 28.736748]


## Regime slices (docs/DESIGN.md §2.3, §5)

Bins reconstructed via `HyperparameterVariance::apply(&base, seed)`, equal-width thirds of each axis's own sampling range (a bench-level reporting choice, not a docs/DESIGN.md-specified boundary). Pooling every bin below reproduces the headline paired mean: pooled=10.413824 vs headline (see "Paired comparison" above).

| regime | n | mean diff | 95% CI |
| --- | --- | --- | --- |
| fee=Low liq=Low sigma=Low | 38 | 49.090883 | [-46.648072, 144.829838] |
| fee=Low liq=Low sigma=Mid | 34 | 74.845180 | [26.898428, 122.791933] |
| fee=Low liq=Low sigma=High | 36 | 544.996828 | [458.650903, 631.342752] |
| fee=Low liq=Mid sigma=Low | 37 | -218.314092 | [-248.038499, -188.589686] |
| fee=Low liq=Mid sigma=Mid | 33 | -35.528785 | [-74.524811, 3.467242] |
| fee=Low liq=Mid sigma=High | 37 | 263.283111 | [222.025609, 304.540613] |
| fee=Low liq=High sigma=Low | 40 | -232.069722 | [-271.004038, -193.135406] |
| fee=Low liq=High sigma=Mid | 35 | -64.380544 | [-99.675725, -29.085363] |
| fee=Low liq=High sigma=High | 33 | 254.798694 | [202.779945, 306.817443] |
| fee=Mid liq=Low sigma=Low | 46 | 1.094311 | [-71.241181, 73.429803] |
| fee=Mid liq=Low sigma=Mid | 32 | 61.071850 | [-11.264124, 133.407824] |
| fee=Mid liq=Low sigma=High | 44 | 404.957312 | [338.104310, 471.810313] |
| fee=Mid liq=Mid sigma=Low | 42 | -292.861099 | [-323.628814, -262.093383] |
| fee=Mid liq=Mid sigma=Mid | 35 | -111.344061 | [-147.210021, -75.478102] |
| fee=Mid liq=Mid sigma=High | 28 | 218.977830 | [175.713043, 262.242617] |
| fee=Mid liq=High sigma=Low | 37 | -295.747218 | [-331.551444, -259.942992] |
| fee=Mid liq=High sigma=Mid | 46 | -146.022807 | [-186.668825, -105.376789] |
| fee=Mid liq=High sigma=High | 46 | 189.544433 | [148.254491, 230.834375] |
| fee=High liq=Low sigma=Low | 35 | -110.571972 | [-171.943335, -49.200609] |
| fee=High liq=Low sigma=Mid | 30 | 132.557443 | [65.502812, 199.612073] |
| fee=High liq=Low sigma=High | 31 | 487.109176 | [402.945532, 571.272820] |
| fee=High liq=Mid sigma=Low | 44 | -328.936149 | [-365.311693, -292.560604] |
| fee=High liq=Mid sigma=Mid | 45 | -133.403533 | [-163.230608, -103.576458] |
| fee=High liq=Mid sigma=High | 33 | 247.132581 | [192.528261, 301.736900] |
| fee=High liq=High sigma=Low | 31 | -367.310846 | [-409.028567, -325.593125] |
| fee=High liq=High sigma=Mid | 37 | -195.171968 | [-239.801607, -150.542329] |
| fee=High liq=High sigma=High | 35 | 112.162925 | [56.705207, 167.620642] |


