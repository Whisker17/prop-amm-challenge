# Out-of-competition oracle-aware research snapshots

out_of_competition: true

These trees were imported from `origin/research/oracle-aware-curve-benchmark` at
`1856c8c`. The reports themselves record binary SHA `26fe1f1`
(`26fe1f1b651c3b93d91cce954919f1eb0016efcc`) as the commit the binary was built
from, not the repository HEAD at import time.

These numbers are **not** §2 ranked results. They **must not** be cited in
`strategies/**/NOTES.md`, `ceilings/**/NOTES.md`, or any `results/compare-*.md`.

Protocol difference vs the other two number streams:

- this crate: zero fee + a shared zero-latency oracle published before arb and
  retail (DODO PMM vs Flashbots ExamplePropAmm vs Uni V2/V3)
- `bench ceiling`: spread-bearing host-side re-anchor the arb cannot front-run,
  paired against the 0-line `001-cpmm-fee`
- ranked stack: submittable `lib.rs` files under `strategies/`, measured by
  `tools/bench` into `results/`

Committed snapshot directories (report bodies not rewritten):

- `final-paired-with-v3/`
- `final-solo-with-v3/`
- `gas/`

How to re-run: `tools/research/README.md`. A re-run of the 1000×10000 bench or
the Foundry gas harness is **out of scope for this import**; the snapshots are
provenance-bearing artefacts, not regenerated.
