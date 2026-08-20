# strategies/

The strategy registry (`docs/DESIGN.md` §4.2). One directory per candidate:
`NNN-<slug>/{lib.rs,NOTES.md}`, `NNNb-<slug>` for a §2.9 variant. `lib.rs` is the self-contained,
committed submission source — the exact artifact a tag would submit. `NOTES.md` is the durable
provenance/fidelity/frozen-space/numbers record.

Sits outside `crates/` so no candidate is ever mistaken for upstream code.

| Id | Name | Source form | Status | Current numbers |
| --- | --- | --- | --- | --- |
| 001-cpmm-fee | 001 CPMM Fee | inherited (starter, unmodified mechanism) | fitted (WHI-1194) — the 0-line | `FEE_BPS = 66`; avg edge 399.97, seeds `0..=999`, 10,000 steps, native (train 406.14, validation 401.80 — `NOTES.md` § Fitted point) |
| 002-normalizer-as-submission | 002 Normalizer As Submission | ported (faithful reimplementation of `crates/shared/src/normalizer.rs`) | measured (WHI-1194) — no free parameter, the opponent's own mechanism at its default fee | avg edge 200.08, seeds `0..=999`, 10,000 steps, native (`NOTES.md` § Measured numbers) |
