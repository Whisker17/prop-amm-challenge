out_of_competition: true

# ceiling-anchored-fingerprint-l1-observation-orbic-oracle-vs-001-cpmm-fee — 2026-08-24

This is the ceiling lane (WHI-1247, `ceilings/README.md`): an out-of-competition measurement of an oracle re-anchor the arbitrageur cannot front-run. Nothing on this page is submittable or ranked.

**Three honesty constraints bound what this number means** (WHI-1247 § Context): (1) it is a one-sided **lower bound** on what perfect price knowledge is worth — Orbic-with-a-spread is one member of the perfect-information class, not its maximum, so this number does not bound the remaining headroom above a stronger submission from above; (2) once the quote is accurate and spread, the arbitrageur mostly stops trading against it, so most of the number is `retail volume x captured spread x flow share(spread)` — the only genuinely non-closed-form content is the flow-share-vs-spread curve the router grants against the normalizer's own sampled fee/liquidity; (3) that content generalizes to *any* oracle-centered quoter and carries little content specific to the Orbic curve itself.

- Commit: `492df79`
- Segment: `observation`
- Simulations: 1000
- Steps: 10000
- Execution path: native (host-side, never BPF-compiled)
- Variant: anchored
- Cursor mode: fingerprint-l1
- Reference (0-line): `001-cpmm-fee`

## Fitted point

- concentration: 62.4200
- spread_bps: 178.0000
- Search budget spent: 191 (exhausted: false)
- Best avg edge on `screening`: 548.858470
- Invalid points during search: 36

## Edge vs the 0-line

| field | value |
|---|---|
| n | 992 |
| mean edge diff (oracle - reference) | 157.807787 |
| std error | 6.284680 |
| 95% interval | [145.490040, 170.125534] |

**Reminder:** per the one-sided-bound honesty constraint above, this is a lower bound on perfect-information value, not an upper bound on any specific submission's remaining headroom — a stronger submission clearing this number is expected and not itself informative about how much further headroom remains.

## Trade-triggered cursor staleness (steps since last executed trade)

| aggregate | value |
|---|---|
| mean of per-sim means | 31.269 |
| median of per-sim means | 17.709 |
| p95 of per-sim means | 99.450 |
| max of per-sim p95 | 3180.000 |
| max of per-sim max | 3180.000 |

## Fingerprint hardening-check trips (WHI-1248)

8 seed(s) tripped a fingerprint hardening check during Phase 1 and were re-run serially, one at a time (Phase 2), to recover a classified message — none was silently dropped from the paired statistic without being named here.

| seed | classification | message |
|---|---|---|
| 125 | SuspectedEarlyAdvance | submission shape violation during arbitrage buy search: monotonicity violated: input 9141.456372 -> output 92.563683, input 12811.674538 -> output 0.000000 (at crates/sim/src/curve_checks.rs:23:9) |
| 212 | SuspectedEarlyAdvance | submission shape violation during arbitrage sell search: monotonicity violated: input 55.576103 -> output 8927.894400, input 73.917708 -> output 0.000000 (at crates/sim/src/curve_checks.rs:23:9) |
| 243 | Other | submission shape violation during arbitrage sell search: monotonicity violated: input 57.470195 -> output 12818.081683, input 80.543997 -> output 0.000000 (at crates/sim/src/curve_checks.rs:23:9) |
| 268 | SuspectedEarlyAdvance | submission shape violation during arbitrage sell search: monotonicity violated: input 50.884955 -> output 8880.015696, input 71.314838 -> output 0.000000 (at crates/sim/src/curve_checks.rs:23:9) |
| 292 | Other | submission shape violation during arbitrage sell search: monotonicity violated: input 61.849943 -> output 10505.163893, input 82.262083 -> output 0.000000 (at crates/sim/src/curve_checks.rs:23:9) |
| 609 | Other | submission shape violation during arbitrage sell search: monotonicity violated: input 58.617415 -> output 10085.012559, input 82.151815 -> output 0.000000 (at crates/sim/src/curve_checks.rs:23:9) |
| 925 | Other | submission shape violation during arbitrage sell search: monotonicity violated: input 55.674754 -> output 9416.084889, input 78.027702 -> output 0.000000 (at crates/sim/src/curve_checks.rs:23:9) |
| 933 | SuspectedEarlyAdvance | submission shape violation during arbitrage sell search: monotonicity violated: input 45.781188 -> output 8031.732137, input 64.161951 -> output 0.000000 (at crates/sim/src/curve_checks.rs:23:9) |

## Per-sigma-tier slices (WHI-1248)

Slices purely by `regime.rs`'s own `sigma` tier (Low/Mid/High equal-width linear thirds of `HyperparameterVariance`'s sampling range), ignoring the fee/liquidity axes `regime::slice_paired_stats` also splits on — a claim specifically about the sigma axis, not the full regime. The "tier range" column is each tier's own true `[lo, hi)` sigma bounds (`regime::tier_bounds`), not a `config/bench.toml` `[grid] gbm_sigma_levels` entry — the three grid levels do not land one-per-tier (WHI-1249).

| sigma tier | tier range | n | mean diff | 95% CI |
|---|---|---|---|---|
| Low | [0.0001, 0.0024) | 350 | 71.111180 | [51.874998, 90.347361] |
| Mid | [0.0024, 0.0047) | 327 | 118.839503 | [101.542684, 136.136321] |
| High | [0.0047, 0.0070) | 315 | 294.590140 | [275.141342, 314.038938] |

**Converge/fan-out verdict:** HELD — the Low-sigma tier's 95% CI width (38.472363) is narrower than the High-sigma tier's (38.897595), consistent with the "converge at low sigma / fan out at high sigma" prediction (CI width used as the dispersion proxy, since `stats::PairedStat` does not expose a per-bin raw variance directly).

