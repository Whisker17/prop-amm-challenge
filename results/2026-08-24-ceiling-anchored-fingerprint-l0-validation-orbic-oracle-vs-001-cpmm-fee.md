out_of_competition: true

# ceiling-anchored-fingerprint-l0-validation-orbic-oracle-vs-001-cpmm-fee — 2026-08-24

This is the ceiling lane (WHI-1247, `ceilings/README.md`): an out-of-competition measurement of an oracle re-anchor the arbitrageur cannot front-run. Nothing on this page is submittable or ranked.

**Three honesty constraints bound what this number means** (WHI-1247 § Context): (1) it is a one-sided **lower bound** on what perfect price knowledge is worth — Orbic-with-a-spread is one member of the perfect-information class, not its maximum, so this number does not bound the remaining headroom above a stronger submission from above; (2) once the quote is accurate and spread, the arbitrageur mostly stops trading against it, so most of the number is `retail volume x captured spread x flow share(spread)` — the only genuinely non-closed-form content is the flow-share-vs-spread curve the router grants against the normalizer's own sampled fee/liquidity; (3) that content generalizes to *any* oracle-centered quoter and carries little content specific to the Orbic curve itself.

- Commit: `492df79`
- Segment: `validation`
- Simulations: 1000
- Steps: 10000
- Execution path: native (host-side, never BPF-compiled)
- Variant: anchored
- Cursor mode: fingerprint-l0
- Reference (0-line): `001-cpmm-fee`

## Fitted point

- concentration: 99.6200
- spread_bps: 95.0000
- Search budget spent: 179 (exhausted: false)
- Best avg edge on `screening`: 791.470498

## Edge vs the 0-line

| field | value |
|---|---|
| n | 999 |
| mean edge diff (oracle - reference) | 400.295089 |
| std error | 6.514270 |
| 95% interval | [387.527354, 413.062823] |

**Reminder:** per the one-sided-bound honesty constraint above, this is a lower bound on perfect-information value, not an upper bound on any specific submission's remaining headroom — a stronger submission clearing this number is expected and not itself informative about how much further headroom remains.

## Trade-triggered cursor staleness (steps since last executed trade)

| aggregate | value |
|---|---|
| mean of per-sim means | 4.380 |
| median of per-sim means | 3.475 |
| p95 of per-sim means | 9.703 |
| max of per-sim p95 | 154.000 |
| max of per-sim max | 260.000 |

## Fingerprint hardening-check trips (WHI-1248)

1 seed(s) tripped a fingerprint hardening check during Phase 1 and were re-run serially, one at a time (Phase 2), to recover a classified message — none was silently dropped from the paired statistic without being named here.

| seed | classification | message |
|---|---|---|
| 2000504 | SuspectedEarlyAdvance | submission shape violation during arbitrage buy search: monotonicity violated: input 8861.368937 -> output 227.975755, input 12419.134343 -> output 0.000000 (at crates/sim/src/curve_checks.rs:23:9) |

## Per-sigma-tier slices (WHI-1248)

Slices purely by `regime.rs`'s own `sigma` tier (Low/Mid/High equal-width linear thirds of `HyperparameterVariance`'s sampling range), ignoring the fee/liquidity axes `regime::slice_paired_stats` also splits on — a claim specifically about the sigma axis, not the full regime. The "tier range" column is each tier's own true `[lo, hi)` sigma bounds (`regime::tier_bounds`), not a `config/bench.toml` `[grid] gbm_sigma_levels` entry — the three grid levels do not land one-per-tier (WHI-1249).

| sigma tier | tier range | n | mean diff | 95% CI |
|---|---|---|---|---|
| Low | [0.0001, 0.0024) | 339 | 263.483173 | [248.973806, 277.992540] |
| Mid | [0.0024, 0.0047) | 349 | 358.504837 | [343.339849, 373.669826] |
| High | [0.0047, 0.0070) | 311 | 596.320931 | [576.507105, 616.134758] |

**Converge/fan-out verdict:** HELD — the Low-sigma tier's 95% CI width (29.018734) is narrower than the High-sigma tier's (39.627653), consistent with the "converge at low sigma / fan out at high sigma" prediction (CI width used as the dispersion proxy, since `stats::PairedStat` does not expose a per-bin raw variance directly).

## L=0 analytic envelope check (WHI-1248)

- Simulated `L=0` avg edge: 802.284247
- Closed-form upper envelope: 1502.562225
- Scope caveat: the envelope is a retail-flow-only quantity (`sum(retail volume_y)`, per the issue's own wording); the simulated avg edge also includes whatever the arbitrageur itself contributes, so the two are not a strictly apples-to-apples comparison — see `analytic_envelope_l0_upper_bound`'s doc comment.
- **Sits below the envelope: PASS** (802.284247 <= 1502.562225).

