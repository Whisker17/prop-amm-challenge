# strategies/

The strategy registry (`docs/DESIGN.md` §4.2). One directory per candidate:
`NNN-<slug>/{lib.rs,NOTES.md}`, `NNNb-<slug>` for a §2.9 variant. `lib.rs` is the self-contained,
committed submission source — the exact artifact a tag would submit. `NOTES.md` is the durable
provenance/fidelity/frozen-space/numbers record.

Sits outside `crates/` so no candidate is ever mistaken for upstream code.

| Id | Name | Source form | Status | Current numbers |
| --- | --- | --- | --- | --- |
| 001-cpmm-fee | 001 CPMM Fee | inherited (starter, unmodified mechanism) | unfit — structure only (WHI-1193); fit is WHI-1194 | avg edge 210.50, seeds `0..=999`, 10,000 steps, native (= starter anchor, `docs/DESIGN.md` §2.1) |
