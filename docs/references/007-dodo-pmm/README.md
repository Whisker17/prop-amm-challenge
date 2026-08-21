# 007 — DODO PMM (R=ONE, arbitrageur-as-oracle)

| Field | Value |
| --- | --- |
| **Name** | DODO Proactive Market Maker (PMM) |
| **Source form** | source (Solidity), library code — not a direct Solana submission |
| **Original material** | `PMMPricing.sol`, `DODOMath.sol`, `DecimalMath.sol`, `DVM*.sol`, `DPP*.sol`, `upstream-README.md` in this directory (unmodified) |
| **Known parameters** | see below |

## Original material location

Repository: <https://github.com/DODOEX/contractV2>, pinned at commit
`2f1bcdac7ef1beee7599a756e2eed26732c2536d`. All files are `Copyright 2020 DODO ZOO`,
`SPDX-License-Identifier: Apache-2.0` — copying is license-clean.

| File here | Upstream path (at the pinned commit) |
| --- | --- |
| `PMMPricing.sol` | `contracts/lib/PMMPricing.sol` |
| `DODOMath.sol` | `contracts/lib/DODOMath.sol` |
| `DecimalMath.sol` | `contracts/lib/DecimalMath.sol` |
| `DVM.sol`, `DVMStorage.sol`, `DVMTrader.sol` | `contracts/DODOVendingMachine/impl/…` |
| `DPPStorage.sol`, `DPPTrader.sol` | `contracts/DODOPrivatePool/impl/…` |
| `DPPOracle.sol`, `DPPOracle-Storage.sol` | `contracts/DODOPrivatePool/impl/DPPOracle/{DPPOracle.sol, DPPStorage.sol}` — kept as evidence of which variant is the *oracle* one, i.e. what this port is **not** |
| `upstream-README.md` | repo `README.md` |

## Mechanism

Confirmed directly from `PMMPricing.sol` / `DODOMath.sol` (not from memory):

- **State.** An anchor price `i` (fixed point, `DecimalMath.ONE = 1e18`), a curvature `K` in
  `[0, 1e18]`, live reserves `B`/`Q`, targets `B0`/`Q0`, and a regime flag `R` (`ONE`,
  `ABOVE_ONE`, `BELOW_ONE`).
- **Marginal price** (`getMidPrice`): on the shortage side with reserve `V` and target
  `V0 >= V`, `R_f = 1 - k + k*(V0/V)^2`; mid is `i*R_f` when base is short (`ABOVE_ONE`),
  `i/R_f` when quote is short (`BELOW_ONE`). `k=0` gives a constant price `i`; `k=1` gives
  `i*(V0/V)^2`, a constant-product curve.
- **Swap quote** (`_SolveQuadraticFunctionForTrade`): solves
  `(1-k)*V2^2 + b*V2 - k*V0^2 = 0` with
  `-b = (1-k)*V1 - k*V0^2/V1 - i*delta`, taking
  `V2 = (-b + sqrt(b^2 + 4(1-k)k*V0^2)) / (2(1-k))`; output is `V1 - V2`. Source
  special-cases `k=0` (`output = min(i*delta, V1)`) and `k=1`
  (`output = V1*t/(1+t)`, `t = i*delta*V1/V0^2` — algebraically the zero-fee CPMM output at
  `V0=V1`).
- **Fee** (`DVMTrader.querySellBase/querySellQuote`): `mtFee = mulFloor(output, mtFeeRate)`,
  then `output - mulFloor(output, lpFeeRate) - mtFee` — **fee on the output**, verified in
  source.

## Port target

`DPPOracle` (classic `i` = live feed) is unportable — this harness has no pre-arbitrage
price feed. `DVM` (fully oracle-free) is one-sided by construction
(`getPMMState()` hardcodes `Q0=0`, `R=ABOVE_ONE`) and bleeds under a GBM that drifts below
the initial price. The full R-state machine has three independent failure modes at real
states (see the porting issue / `NOTES.md` for the full enumeration). The port target is
**DPP's two-sided PMM, collapsed to `R = ONE`, with the arbitrageur as the oracle**: the
anchor `i` is re-pinned to the post-trade mid on every executed trade, so `B0=B`, `Q0=Q`
always hold at quote time and the R-state machine's inventory-drift-between-updates
machinery is dead code — re-anchoring per trade is the faithful rendering of `DPPOracle`'s
per-quote oracle read.

## Known parameters

| Name | Value | Meaning |
| --- | --- | --- |
| `K` | source has no single default — `DVM.init` only enforces `require(k <= 10**18)` | curvature, `k=1` is the source's own upper bound and its `k==ONE` special case |
| `_LP_FEE_RATE_` / `_MT_FEE_RATE_MODEL_` | source has no fixed default — set per-pool at init/via a fee-rate model contract | split fee on the output; this port collapses both into one `FEE_BPS` rate since no separate LP/maintainer recipients exist here |

Per `docs/DESIGN.md` §2.4, DODO ships no single tuned default for `k`/fee the way `005`'s
source did — every value in this port's frozen parameter space is derived from the
porting issue's own reasoning (containment against `001-cpmm-fee`, plausible concentration
range against the harness's own liquidity-multiplier axis), not carried over from a
DODO-side default.

## Fidelity note

This is a **source-available library port**, not a direct-submission-shaped source like
`005`'s: `PMMPricing.sol`/`DODOMath.sol` are 1e18-fixed-point Solidity library functions
operating on a general `V0 != V1` PMM state machine, not a `pinocchio` program. The port
collapses the general R-state machine to `R = ONE` (by construction, per-trade
re-anchoring), rescales from 1e18 to this harness's 1e9 nano fixed point, and rewrites the
quadratic solve in a form that survives this repo's own concavity checker (the rationalised
`V2 = 2k*V0^2/(sqrt(disc)+b_abs)` form in the `bSig` branch, per `docs/DESIGN.md` §2.9
cross-cutting finding #7) — see `strategies/007-dodo-pmm/NOTES.md` § Fidelity
self-assessment for the itemised list of every departure from the source's own arithmetic
and why each is within the fidelity contract's bounds.
