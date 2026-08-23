out_of_competition: true

# ceiling-floating-trade-triggered-observation-orbic-oracle-vs-001-cpmm-fee — 2026-08-23

This is the ceiling lane (WHI-1247, `ceilings/README.md`): an out-of-competition measurement of an oracle re-anchor the arbitrageur cannot front-run. Nothing on this page is submittable or ranked.

**This run produced no number.** The same three honesty constraints (WHI-1247 § Context) — one-sided lower bound, mostly `retail volume x captured spread x flow share(spread)`, generalizing beyond the Orbic curve specifically — would bound what an edge figure means here, but they bind nothing until a valid re-evaluation exists. See "## Final re-evaluation: INVALID" below for why this run has none.

- Commit: `831284c`
- Segment: `observation`
- Simulations: 1000
- Steps: 10000
- Execution path: native (host-side, never BPF-compiled)
- Variant: floating
- Cursor mode: trade-triggered
- Reference (0-line): `001-cpmm-fee`

## Fitted point

- concentration: 2.3300
- spread_bps: 77.0000
- Search budget spent: 165 (exhausted: false)
- Best avg edge on `screening`: 639.575681
- Invalid points during search: 9

## Final re-evaluation: INVALID

The fitted point above was chosen from a search on the `screening` segment, but re-evaluating it on the full `observation` segment triggered a caught panic instead of producing a number. This is the documented failure mode in `docs/DESIGN.md` §2.4/§2.5 (WHI-1213): a point valid on `screening`'s seeds is not guaranteed valid on a different, larger seed set — exactly what made the Orbic family's own "quantization jitter" (`docs/DESIGN.md` §6.2, strategy `002`, WHI-1206, Canceled) probabilistic across seeds, not just across parameter values. The panic message below is a fixed placeholder: a non-string panic payload defeated this lane's own `panic_message` downcast, so the cause is not pinned down further than "a panic occurred" plus whatever source location the installed panic hook captured independently of the payload. No edge-vs-0-line or staleness numbers exist for this run; the absence of a crash on `screening` is not evidence this variant/point is safe on other segments or seeds.

Panic message: `panicked with a non-string payload (at crates/sim/src/curve_checks.rs:23:9)`

