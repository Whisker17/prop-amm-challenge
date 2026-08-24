out_of_competition: true

# ceiling-anchored-fingerprint-l1-validation-orbic-oracle-vs-001-cpmm-fee — 2026-08-24

This is the ceiling lane (WHI-1247, `ceilings/README.md`): an out-of-competition measurement of an oracle re-anchor the arbitrageur cannot front-run. Nothing on this page is submittable or ranked.

**Three honesty constraints bound what this number means** (WHI-1247 § Context): (1) it is a one-sided **lower bound** on what perfect price knowledge is worth — Orbic-with-a-spread is one member of the perfect-information class, not its maximum, so this number does not bound the remaining headroom above a stronger submission from above; (2) once the quote is accurate and spread, the arbitrageur mostly stops trading against it, so most of the number is `retail volume x captured spread x flow share(spread)` — the only genuinely non-closed-form content is the flow-share-vs-spread curve the router grants against the normalizer's own sampled fee/liquidity; (3) that content generalizes to *any* oracle-centered quoter and carries little content specific to the Orbic curve itself.

- Commit: `492df79`
- Segment: `validation`
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
| n | 991 |
| mean edge diff (oracle - reference) | 161.468800 |
| std error | 6.465080 |
| 95% interval | [148.797477, 174.140123] |

**Reminder:** per the one-sided-bound honesty constraint above, this is a lower bound on perfect-information value, not an upper bound on any specific submission's remaining headroom — a stronger submission clearing this number is expected and not itself informative about how much further headroom remains.

## Trade-triggered cursor staleness (steps since last executed trade)

| aggregate | value |
|---|---|
| mean of per-sim means | 31.172 |
| median of per-sim means | 18.900 |
| p95 of per-sim means | 98.040 |
| max of per-sim p95 | 1085.000 |
| max of per-sim max | 1808.000 |

## Fingerprint hardening-check trips (WHI-1248)

9 seed(s) tripped a fingerprint hardening check during Phase 1 and were re-run serially, one at a time (Phase 2), to recover a classified message — none was silently dropped from the paired statistic without being named here.

| seed | classification | message |
|---|---|---|
| 2000010 | SuspectedEarlyAdvance | submission shape violation during arbitrage sell search: monotonicity violated: input 24.355287 -> output 10807.702715, input 34.133731 -> output 0.000000 (at crates/sim/src/curve_checks.rs:23:9) |
| 2000023 | SuspectedEarlyAdvance | submission shape violation during arbitrage sell search: monotonicity violated: input 60.497240 -> output 8191.408348, input 84.786375 -> output 0.000000 (at crates/sim/src/curve_checks.rs:23:9) |
| 2000056 | Other | submission shape violation during arbitrage sell search: monotonicity violated: input 24.915252 -> output 12483.956077, input 33.137954 -> output 0.000000 (at crates/sim/src/curve_checks.rs:23:9) |
| 2000088 | SuspectedEarlyAdvance | submission shape violation during arbitrage sell search: monotonicity violated: input 44.970364 -> output 9267.336695, input 63.025589 -> output 0.000000 (at crates/sim/src/curve_checks.rs:23:9) |
| 2000319 | SuspectedEarlyAdvance | submission shape violation during arbitrage sell search: monotonicity violated: input 43.850239 -> output 9365.747101, input 53.284272 -> output 0.000000 (at crates/sim/src/curve_checks.rs:23:9) |
| 2000335 | SuspectedEarlyAdvance | submission shape violation during arbitrage sell search: monotonicity violated: input 46.697644 -> output 11344.762206, input 65.446357 -> output 0.000000 (at crates/sim/src/curve_checks.rs:23:9) |
| 2000573 | Other | submission shape violation during arbitrage sell search: monotonicity violated: input 37.313884 -> output 10131.804994, input 49.628467 -> output 0.000000 (at crates/sim/src/curve_checks.rs:23:9) |
| 2000716 | SuspectedEarlyAdvance | submission shape violation during arbitrage sell search: monotonicity violated: input 53.110157 -> output 10189.329467, input 64.536388 -> output 0.000000 (at crates/sim/src/curve_checks.rs:23:9) |
| 2000895 | SuspectedEarlyAdvance | submission shape violation during arbitrage sell search: monotonicity violated: input 62.498044 -> output 9507.310448, input 87.590485 -> output 0.000000 (at crates/sim/src/curve_checks.rs:23:9) |

## Per-sigma-tier slices (WHI-1248)

Slices purely by `regime.rs`'s own `sigma` tier (Low/Mid/High equal-width linear thirds of `HyperparameterVariance`'s sampling range), ignoring the fee/liquidity axes `regime::slice_paired_stats` also splits on — a claim specifically about the sigma axis, not the full regime. The "tier range" column is each tier's own true `[lo, hi)` sigma bounds (`regime::tier_bounds`), not a `config/bench.toml` `[grid] gbm_sigma_levels` entry — the three grid levels do not land one-per-tier (WHI-1249).

| sigma tier | tier range | n | mean diff | 95% CI |
|---|---|---|---|---|
| Low | [0.0001, 0.0024) | 339 | 66.911770 | [47.006290, 86.817249] |
| Mid | [0.0024, 0.0047) | 349 | 140.536290 | [121.755982, 159.316599] |
| High | [0.0047, 0.0070) | 303 | 291.370713 | [271.149320, 311.592105] |

**Converge/fan-out verdict:** HELD — the Low-sigma tier's 95% CI width (39.810960) is narrower than the High-sigma tier's (40.442785), consistent with the "converge at low sigma / fan out at high sigma" prediction (CI width used as the dispersion proxy, since `stats::PairedStat` does not expose a per-bin raw variance directly).

