# 006 — Hedged PnL (Linear Price Impact Model)

| Field | Value |
| --- | --- |
| **Name** | Hedged PnL |
| **Source form** | prose (HackMD doc) |
| **Original material** | `prop-amm-design-and-analysis.md` in this directory (verbatim download) |
| **Known parameters** | none — see below |

## Original material location

<https://hackmd.io/15KsANu8TOyH5MG3I6NTcA>, downloaded verbatim via its `/download`
endpoint (a plain-markdown mirror of the rendered note — no rendering/paraphrase applied).

## Important provenance flag — read before treating this as a straightforward port

This document is titled "Prop AMM Design & Analysis" and describes the **evolution of the
competition itself**, contrasting a "Simple AMM competition" (fee-only control, scored on
"Total Edge") against what it calls "the Prop AMM sequel" (full price-function control,
scored on **Hedged PnL**, volatility sampled from `U[0.01%, 0.70%]`). Its own references
section links `https://www.ammchallenge.com/prop-amm` and
`https://github.com/benedictbrady/prop-amm-challenge` — i.e. it claims to describe *this*
challenge.

**The scoring description does not match this repo's measured configuration — the
volatility range does.** Checked directly against `crates/shared/src/config.rs`:
`gbm_sigma_min = 0.0001` (0.01%), `gbm_sigma_max = 0.007` (0.7%), sampled per simulation —
an exact match for the doc's `U[0.01%, 0.70%]`, and actually the strongest evidence that
this document really does describe *this* challenge's simulator. The mismatch is narrower
than it first looks: this challenge's own simulator (`crates/shared/src/config.rs`,
`AGENTS.md`) scores on **average edge** (`SimResult`'s `submission_edge`, `docs/DESIGN.md`
§2.1) — a per-trade accumulated quantity — not the doc's own `HPnL = (Σ Δx_t)·p_T + (Σ
Δy_t)` terminal-inventory formula, and the doc itself names that exact distinction ("Edge
effectively assumes hedging at every individual step, HPnL accounts for cumulative
inventory risk held until the end of the simulation"). Per `docs/DESIGN.md` §2.9,
"Provenance is mandatory. Prose-only strategies may be second-hand or simply wrong" — the
scoring-metric mismatch is exactly that case, flagged rather than silently resolved. Two
readings are possible and this freeze does **not** pick between them:

1. The doc describes an earlier draft, or a different variant, of the challenge rules that
   never shipped as this repo's actual simulator.
2. "Hedged PnL" as a *name* is a red herring, and the actually-transferable content is the
   **mechanism** in the doc's "Linear Price Impact Model" section — a two-sided,
   quadratic-trading-function AMM with cross-impact terms, independent of which scoring
   rule it's judged by.

## Mechanism — the portable part (§ Linear Price Impact Model, quoted structure)

Marginal buy/sell prices move linearly with signed order flow:

```
Δp+ = -k++ Δx+ + k+- Δx-
Δp- = -k-+ Δx+ + k-- Δx-
```

Integrating yields a piecewise-quadratic trading function (AMM buys X for `Δx > 0`, sells
X for `Δx < 0`):

```
Δy = -p+ Δx+ + (1/2) k++ (Δx+)^2      if Δx > 0
Δy =  p- Δx- - (1/2) k-- (Δx-)^2      if Δx < 0
```

with **cross-impact**: when the arbitrageur trades one side back to fair, the *other*
side's marginal price is adjusted proportionally to the cross-impact coefficient ratio
(e.g. `p- -= (k-+/k++)(p+ - s)` after an X-overpriced correction). This is a genuinely
different curve family from both the piecewise-linear liquidity curve (`003`) and the
fee-only CPMM variants (`001`, `004`, `005`): it's a *linear-marginal-price* / quadratic
payoff family with four independent slope parameters and no separate "fee" term at all —
the impact slopes themselves are what capture spread.

## Known parameters

**None given.** The document states the functional form only — `k++`, `k+-`, `k-+`, `k--`
have no suggested numeric values anywhere in the source. Per `docs/DESIGN.md` §2.4, the
porting issue must propose and freeze its own parameter space for these four coefficients
before any search runs; there is no original-material anchor point to start from, unlike
`001`/`004`/`005` which each ship at least one concrete constant set.

## Fidelity note

Source form is prose-only and, per the flag above, its claimed **scoring metric** is not
directly usable (this repo scores on average edge, not HPnL) — only the linear-price-impact
mechanism itself should be treated as frozen here, not the doc's claimed scoring rule. This
is the lowest-fidelity, highest-provenance-risk entry in this freeze; the porting issue's
`NOTES.md` fidelity self-assessment should say so explicitly (§2.9).
