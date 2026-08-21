# fuzz-broken_inverted_kink — 2026-08-21

- Commit: `139a3bd`
- Segment: `fuzz (not a config/bench.toml segment)`
- Simulations: 0
- Steps: 0
- Execution path: native (fast path)

## bench fuzz — shape violation found (WHI-1212)

- Candidate: `tools/bench/tests/fixtures/broken_inverted_kink/lib.rs`
- Fuzz sweep size: 324 states x 2 sides (this run stopped at the first violation, not all of them)
- State: cell 0 (fee=30bps liq=0.4x sigma=0.0001) price=100.000000 reserve_x=40.000000 reserve_y=4000.000000 [on-invariant, zero storage]
- Side: buy X (input Y)
- Sample kind: dense sweep (moderate range)
- Message: concavity violated: slope rose from 0.800000000 to 0.888001892 between inputs 836.821077 and 1046.026093


