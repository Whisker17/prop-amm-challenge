# 002 — Orbic (Flashbots `ExamplePropAmm`)

| Field | Value |
| --- | --- |
| **Name** | Orbic |
| **Source form** | Solidity (to port) |
| **Original material** | `ExamplePropAmm.sol` in this directory — full contract, unmodified |
| **Known parameters** | see below |

## Original material location

<https://github.com/flashbots/priority-update-registry/blob/main/src/ExamplePropAmm.sol>,
pinned at commit `da53117870c7bec96d71caebe1b3f94370aba3d6`. The contract itself is
MIT-licensed (SPDX header) and states it was "Adapted from
https://github.com/fahimahmedx/prop-amm" — that upstream is not fetched here; only the
Flashbots example, which is what "Orbic" points to.

`ExamplePropAmm.sol` in this directory is the file as pinned, byte-for-byte. Its sibling
`PrioUpdateRegistry.sol` (the on-chain "priority update" oracle the contract reads
parameters from) is **not** copied — it is Flashbots-specific block-builder infrastructure
with no analogue in this challenge's simulator (`crates/shared`), so porting only needs the
pricing/lock mechanism below, not the registry plumbing.

## Mechanism (for the porting issue)

A single trading pair, market-maker-supplied liquidity, priced off three oracle-published
parameters (`concentration`, `multX`, `multY`) rather than raw reserves alone:

- `v0 = targetX * concentration`
- `K = v0^2 * multX / multY`
- `base = v0 + reserveX - targetX`
- X→Y: `amountOut = K/base - K/(base + amountXIn)`
- Y→X: `amountOut = base - K/(K/base + amountYIn)`

This is a constant-product curve shifted by the "concentration" term (`v0`) rather than a
plain `x*y=k` — larger `concentration` concentrates liquidity around the current target,
similar in spirit to a virtual-reserves / concentrated-liquidity CPMM variant.

An emergency lock (`_isTargetYLocked`) freezes trading if a derived `targetY` value drifts
more than 5% below its own running maximum (`targetYReference`) — a circuit breaker, not a
pricing input; whether it has any analogue worth porting (there is no external oracle in
this simulator to go stale) is a call for the porting issue, not this freeze.

## Known parameters

| Name | Constraint / default | Source |
| --- | --- | --- |
| `concentration` | integer, `1 <= concentration < 2000` | `createPair`'s `InvalidConcentration` check |
| `multX`, `multY` | oracle-published, no on-chain bound | `PairParameters` — no default; market maker must publish before any swap |
| `maxParameterAge` | constructor-supplied, seconds | staleness bound for the *oracle read*, not a curve parameter |
| lock threshold | fixed `500` (of `10000`) = 5% | `_isTargetYLocked`, not currently exposed as a tunable |

None of these are the parameter space the porting issue searches over — that space is
declared and frozen in that issue's own `NOTES.md` per `docs/DESIGN.md` §2.4, using this
table only as the starting point (this contract's own bounds and mechanism).

## Fidelity note

The registry-read staleness bound (`maxParameterAge`) and the market-maker-gated liquidity
model don't map cleanly onto this challenge's single-file, stateless-oracle
`compute_swap`/`after_swap` interface (`crates/submission-sdk`) — the porting issue will
need to decide what a submission's own analogue of "oracle-published parameters" is (most
likely: values baked into `PARAMS` and/or updated via `after_swap`'s 1024-byte storage,
since there is no external registry to read from at swap time). That decision belongs to
the porting issue, not this freeze.
