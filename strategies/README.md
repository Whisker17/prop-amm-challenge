# strategies/

The strategy registry (`docs/DESIGN.md` §4.2). One directory per candidate:
`NNN-<slug>/{lib.rs,NOTES.md}`, `NNNb-<slug>` for a §2.9 variant. `lib.rs` is the self-contained,
committed submission source — the exact artifact a tag would submit. `NOTES.md` is the durable
provenance/fidelity/frozen-space/numbers record.

Sits outside `crates/` so no candidate is ever mistaken for upstream code.

| Id | Name | Source form | Status | Current numbers |
| --- | --- | --- | --- | --- |
| 000-normalizer | 000 Normalizer (as submission) | inherited (`crates/shared/src/normalizer.rs`, unmodified mechanism) | **baseline, not a ranked candidate** — docs/DESIGN.md §2.8's protocol self-check; no frozen space, no fitting (WHI-1195) | see `results/2026-08-20-l1.md`, `results/2026-08-20-grid.md` |
| 001-cpmm-fee | 001 CPMM Fee | inherited (starter, unmodified mechanism) | fitted (WHI-1194) — the 0-line | `FEE_BPS = 66`; avg edge 399.97, seeds `0..=999`, 10,000 steps, native (train 406.14, validation 401.80 — `NOTES.md` § Fitted point) |
| 005-vol-adaptive-cpmm-fee | 005 Vol-Adaptive CPMM Fee | source (Rust, direct submission shape) — `dcccrypto/percolator-perp-liquidity`'s `EdgeMax_CumVar.rs` | fitted (WHI-1209) — beats the 0-line | `FEE_LO=5, A_NUM=13, B_DEN=1265`; avg edge 422.93, seeds `0..=999`, 10,000 steps, native (train 430.79, validation 425.95 — `NOTES.md` § Search); 27-cell grid vs. `001`: 17/27 cells favor `005` (`results/2026-08-21-grid.md`, explained in `NOTES.md` § Grid mode) |
