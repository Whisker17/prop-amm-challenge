# fit-007b-k1-containment — 2026-08-25

- Commit: `231dd58`
- Segment: `screening`
- Simulations: 200
- Steps: 10000
- Execution path: native (fast path)

## Frozen parameter space

- `K_BPS` (u128): 10000..=10000
- `FEE_BPS` (u128): 66..=66


## Search budget

- Cap: 300
- Spent: 1
- Invalid: 0
- Stopped: converged


## Invalid points

None — every evaluated point produced a valid edge.


## Fast-path compile timing

- 3 warm compiles: min=0.262s, mean=0.417s, max=0.724s — MEETS the <1s target (max warm sample below it)
- (search phase plus the two final train/validation builds) Every sample is a `cargo build` invocation against the single, reused `.build/fast/` directory.


## Winning point

- K_BPS = 10000, FEE_BPS = 66
- Screening avg edge: 386.052462
- Train (1000 sims): avg edge 407.545843
- Validation (1000 sims): avg edge 403.257399


## Evaluated curve

- [10000, 66] -> 386.052462


