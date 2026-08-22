# fit-008-lagging-vwap-fee-anchor — 2026-08-22

- Commit: `7e539b6+dirty`
- Segment: `screening`
- Simulations: 200
- Steps: 10000
- Execution path: native (fast path)

## Frozen parameter space

- `ARB_K_BPS` (u64): 5200..=5200
- `COUNTER_K_BPS` (u64): 1000..=1000
- `TARGET_BASE_BPS` (u64): 16..=16
- `SIZE_K_BPS` (u64): 2900..=2900


## Search budget

- Cap: 300
- Spent: 1
- Invalid: 0
- Stopped: converged


## Invalid points

None — every evaluated point produced a valid edge.


## Fast-path compile timing

- 3 warm compiles: min=0.250s, mean=0.514s, max=1.034s — EXCEEDS the <1s target (max warm sample 1.034s) — see NOTES.md for whether this reflects system contention rather than the fast path itself
- (search phase plus the two final train/validation builds) Every sample is a `cargo build` invocation against the single, reused `.build/fast/` directory.


## Winning point

- ARB_K_BPS = 5200, COUNTER_K_BPS = 1000, TARGET_BASE_BPS = 16, SIZE_K_BPS = 2900
- Screening avg edge: 478.537310
- Train (1000 sims): avg edge 509.422465
- Validation (1000 sims): avg edge 503.907498


## Evaluated curve

- [5200, 1000, 16, 2900] -> 478.537310


