# Oracle-aware AMM curve comparison benchmark (research only)

This crate compares AMM curve designs under **identical external quotes,
identical initial capital, identical order flow, identical seeds and zero fees**:

1. Uniswap V2 (zero fee)
2. Flashbots `ExamplePropAmm`
3. DODO V2 PMM
4. Uniswap V3 — phase 2, not implemented yet

It is research-only. It does not participate in the challenge submission path,
does not change the `prop-amm` CLI, and does not modify `prop-amm-shared`,
`prop-amm-executor` or `prop-amm-sim`. It has no third-party dependencies beyond
the workspace crates it already builds against, so adding it cannot break an
offline build of the submission toolchain.

## Ground rules

* **No formula is modified.** Every curve is a literal port of a pinned Solidity
  source: integer operation order, `floor`/`ceil` choices, special-case
  branches, overflow-avoidance branches and state transitions are preserved.
  Where Solidity would revert, the port returns `None` and the adapter reports a
  zero quote (which the simulation already treats as "no trade").
* **No `f64` inside any curve.** All curve maths run on [`u256::U256`], a
  hand-written exact 256-bit integer verified against 1 706 Python
  arbitrary-precision reference vectors. The oracle price is quantised to a WAD
  integer exactly once, at the boundary ([`wad::price_to_wad`]), using the exact
  binary expansion of the `f64` — never by multiplying in floating point.
* **Expected values never come from this crate.** DODO expectations are the 19
  pinned upstream golden vectors; Flashbots expectations are produced by
  executing the pinned Solidity contract.

## Provenance

| Curve | Source | Pinned commit |
| --- | --- | --- |
| DODO V2 PMM | `DODOEX/contractV2` — `DecimalMath.sol`, `DODOMath.sol`, `PMMPricing.sol` | `8da3ee1ec50966fca9a2c80d424040c45c0f785e` |
| Flashbots prop AMM | `flashbots/priority-update-registry` — `src/ExamplePropAmm.sol` | `da53117870c7bec96d71caebe1b3f94370aba3d6` |
| Uniswap V2 | `UniswapV2Library.getAmountOut` | fee numerator `997 -> 1000` (zero fee); nothing else changed |

DODO state persistence follows the read-only Mantle reference implementation,
pinned as follows (also recorded in every `summary.json`):

| Item | Pin |
| --- | --- |
| repository | `mantle-propamm-contracts`, branch `feature/v2-dodo-curve`, commit `07f6797` |
| `src/MantlePropAmmPool.sol` | sha256 `28bd3d2fa70481ffa0ab61b2d0ff1a7914406fdeeb5814643eb5d8b12e9530c4` (last modified there by `5e4071b`) |
| `src/MantlePropAmmTypes.sol` | sha256 `bee141418f90a91e3f76f291468e47ba39d7865be27fb4ff172fe308149517c8` |
| design document | `docs/[TD] mantle PropAmm 合约设计.md`, sha256 `c395caa6eb5c68595274bf1c1af29882c23c1228fdf7a2dda019961fb7e52b3a` |
| vendored DODO | `DecimalMath` `27d9d19a…6352`, `DODOMath` `90f688a2…c448a`, `PMMPricing` `093b9ada…807e` |

Those files are byte-identical between `01a556a` (when they were read) and
`07f6797`; only deployment scripts and docs moved in between. Every result set
also records the **benchmark repository commit** that produced it, plus whether
the working tree was dirty at run time.

Fixture provenance is recorded in `tests/fixtures/dodo/SOURCES.md`,
`tests/fixtures/flashbots/SOURCES.md` and `tests/fixtures/u256/SOURCES.md`.

The read-only `mantle-propamm-contracts` checkout is never modified. The
Flashbots vector generator copies the pinned library into a temporary directory
and runs `forge` there.

## Layout

```
src/u256.rs             exact uint256 (add/sub/mul/div incl. Knuth D, no deps)
src/wad.rs              the single f64 boundary: price quantisation, nano <-> WAD
src/dodo/               DODO port: decimal_math, dodo_math, pmm_pricing, state
src/flashbots.rs        Flashbots ExamplePropAmm quote functions
src/univ2.rs            zero-fee Uniswap V2 getAmountOut
src/curves.rs           storage layouts + compute_swap / after_swap adapters
src/strategies.rs       strategy catalogue and the K <-> concentration table
src/probe.rs            equilibrium mid prices and the quote matrix
src/experiment.rs       the research simulation loop and batch runner
src/metrics.rs          per-run records, percentiles, per-strategy summaries
src/paired.rs           per-seed paired differences, 95% CI, oracle/shape attribution
src/report.rs           JSON / CSV / Chinese Markdown output
src/bin/research.rs     the research CLI (separate binary from `prop-amm`)
solidity/flashbots-golden/  the Solidity golden-vector generator
```

## Unified initial state

| | value |
| --- | --- |
| fair price | 100 |
| `reserveX` | 100 |
| `reserveY` | 10 000 |
| DODO | `B = B0 = 100`, `Q = Q0 = 10 000`, `R = ONE`, `lpFeeRate = 0` |
| Flashbots | `reserveX = targetX = 100`, `reserveY = 10 000`, no fee |
| Uniswap V2 | `reserveX = 100`, `reserveY = 10 000`, fee `0` |

## Parameter pairing

| DODO `K` | Flashbots `concentration` |
| --- | --- |
| `1e18` | 1 |
| `0.5e18` | 2 |
| `0.2e18` | 5 |
| `0.1e18` | 10 |
| `0.05e18` | 20 |
| `0.02e18` | 50 |
| `0.01e18` | 100 |
| `0.001e18` | 1000 |

`K ≈ 1 / concentration` matches only the **local** curvature near the
equilibrium point. It is not used to modify or substitute any formula, and no
claim of global curve equivalence is made. The quote matrix shows the two
families tracking each other closely for small orders and diverging as size
grows, which is exactly what that caveat means.

## Oracle injection path

Per step, in this order:

1. `fair_price = price.step()` (unmodified `GBMPriceProcess`)
2. quantise **once** to `priceWad`, and publish the same integer to every
   oracle-aware curve: DODO `i = priceWad`, Flashbots `multX = priceWad` with
   `multY = 1e18`
3. `Arbitrageur::execute_arb` against both AMMs (unmodified)
4. `RetailTrader::generate_orders` then `OrderRouter::route_order` (unmodified)

This is the **zero-latency** experiment: the price is published before any trade
in the same step, so the oracle-aware curves are never stale. A stale-oracle
(latency) variant would publish step `n - d`'s price at step `n`; it is not
implemented, and no latency result is reported.

Publication reuses `BpfAmm::set_initial_storage`, which copies from offset 0;
every research curve keeps its oracle-published field in `storage[0..32]`, so no
simulation code had to change. Uniswap V2 consumes no oracle by construction and
is included as the no-oracle baseline.

## Running

```bash
# mid price of every curve at the unified initial state (integer WAD equality)
cargo run --release -p prop-amm-research --bin research -- equilibrium

# quote matrix across order sizes, printed as slippage in bps
cargo run --release -p prop-amm-research --bin research -- quote-matrix --out research-out

# smoke benchmark
cargo run --release -p prop-amm-research --bin research -- \
  bench --simulations 20 --steps 1000 --out research-out/smoke-paired

# full benchmark (paired against the challenge normalizer)
cargo run --release -p prop-amm-research --bin research -- \
  bench --simulations 1000 --steps 10000 --out research-out/full-paired

# same seeds, no competitor: all retail flow reaches the curve under test
cargo run --release -p prop-amm-research --bin research -- \
  bench --simulations 1000 --steps 10000 --mode solo --out research-out/full-solo
```

Outputs per run directory:

| File | Contents |
| --- | --- |
| `REPORT.zh-CN.md` | Chinese report: setup, provenance, scope, health check, rankings, splits, paired statistics |
| `summary.json` / `summary.csv` | per-strategy distributions (mean, P5/P50/P95, min, max) and `curve_reverts` |
| `runs.csv` | one row per strategy per seed |
| `paired-stats.json` / `paired-deltas.csv` | per-seed paired differences with 95% CI, t, paired win rate, significance |
| `attribution.csv` | oracle-system vs curve-shape split of each strategy's advantage over Uniswap V2 |
| `quote-matrix.csv` / `.json`, `mid-price.json` | static probes at the opening inventory |

### Modes

* `paired` (default) — the curve under test occupies the challenge's submission
  slot and competes with the constant-product-with-fee normalizer for retail
  flow, so `retailFlowShare` is meaningful.
* `solo` — the competitor is a curve that never quotes, so the routed flow all
  reaches the curve under test. Useful for reading the curve in isolation.

Within a mode, all strategies see identical price paths and identical retail
orders for a given seed. The arbitrageur draws exactly one sample per step
against the curve under test regardless of family, so its search also starts
from the same size. (Across modes the arbitrage RNG stream differs, because the
normalizer competitor is priced closed-form and consumes no draw while the null
competitor does — compare within a mode, not across.)

## Metrics

Edge is measured from the AMM's point of view, marked at the external fair price,
using the same expressions the existing engine uses, so a positive edge is AMM
profit:

* `retailEdge`, `arbitrageEdge` (normally negative), `arbitrageLoss = max(0, -arbitrageEdge)`
* `netEdge = retailEdge + arbitrageEdge`
* `retailFlowShare` = the curve's retail notional / total retail notional
* `arbCount`, `arbNotional`
* `finalInventoryDeviation`, `maxInventoryDeviation`, both `(reserveX - initialX) / initialX`
* P5 / P50 / P95, mean, min and max for each of the above
* `positiveNetRate` — the share of seeds on which *this* strategy's `netEdge` was
  positive. It is a marginal rate, **not** a comparison against another curve.

### Paired statistics

Every strategy sees the same seeds, so cross-curve claims are made on **per-seed
differences**, which cancel the shared price path and order flow:

* `Flashbots - DODO` on every row of the pairing table
* every strategy against the zero-fee Uniswap V2 baseline

each with mean, P5/P50/P95, standard error, a 95% confidence interval (normal
approximation, `mean ± 1.96 · SE`), the t statistic, and the **paired win rate**
— the share of seeds on which the treatment beat the baseline. A ~50% paired win
rate with |t| < 2 means the two curves are indistinguishable, no matter how their
marginal means happen to rank.

### Oracle advantage vs curve shape

`attribution.csv` splits each strategy's advantage over zero-fee Uniswap V2 into:

* **oracle-system advantage** = `mean(anchor - univ2)`
* **curve-shape effect** = `mean(strategy - anchor)`

where the anchor is the oracle-aware strategy in the same family whose curve
shape coincides with a zero-fee constant product at the opening inventory:
`DODO K = 1e18` (the constant-product case of the PMM) and
`Flashbots concentration = 1` (its virtual quote reserve equals the real one when
`reserveX == targetX`). The quote matrix shows both quoting bit-identically to
Uniswap V2 at that point. The two components sum exactly to
`mean(strategy - univ2)`.

The split is anchored at the opening inventory. It is not a claim that an
oracle-aware curve equals constant product elsewhere — once the oracle moves, the
anchor reprices and a passive constant product does not. Only means decompose;
percentiles are not split.

## Scope: curve-only

This benchmark compares **quote functions**. Pool-level admission and risk
control are out of scope and are not implemented:

* Flashbots `_isTargetYLocked` (the targetY emergency lock). It guards the swap
  path only; upstream `quoteXtoY` / `quoteYtoX` do not pass through it either.
* The Mantle pool's notional / reserve / inventory-deviation caps, which are
  Mantle policy rather than DODO pricing.
* Balance, allowance and transfer checks.

Consequently `curve_reverts = 0` means **the ported pricing maths never hit a
revert branch** (overflow, underflow, division by zero) across the whole run. It
is *not* a claim that a full on-chain swap would have succeeded.

## Fidelity notes

Things that are deliberately preserved or explicitly bounded:

* **DODO `_sqrt(2) == 2`.** The upstream Babylonian loop starts at `y = x` with
  `z = x/2 + 1`, so for `x = 2` it never iterates. This is upstream behaviour at
  the pinned commit and is kept, with a test pinning it.
* **DODO target persistence.** A target is written back only when `RState`
  changes, and then only on the side matching the trade direction, exactly as
  `MantlePropAmmPool.swap` does. `tests/dodo_state_machine.rs` asserts this over
  a randomised trade sequence, plus the exact return-to-one case.
* **Exact pre-trade reserves in `after_swap`.** The DODO adapter keeps the
  reserves recorded at the last commit in its own storage. Because the simulation
  only changes reserves through an executed swap, those are exactly the pre-trade
  reserves, so the `after_swap` re-evaluation reproduces the state transition the
  quote implied without reconstructing anything from post-trade values.
* **The simulation ledger is `f64`/nano, and that is unchanged.** Reserves enter
  a curve as `nano -> WAD` (exact `* 1e9`) and quotes return as `WAD -> nano`
  (truncating). Curve maths are integer throughout; the ledger itself is the
  existing simulation's, shared identically by every strategy.
* **Runtime shape checks are not involved.** `prop-amm-sim`'s monotonicity /
  concavity guard only applies to an AMM named `submission`; research strategies
  carry their own ids, so the guard is inert here. It was not modified.
* **Mantle risk overlays are not part of the curve** (see *Scope* above).
  Output-exceeds-reserve is still rejected with a zero quote, which is what the
  simulation already does for any curve.

## Phase 2: Uniswap V3

Not implemented. When it is, it must carry the official tick / `sqrtPriceX96` /
active-liquidity semantics including cross-tick traversal. A single virtual
constant product with a fixed range is not Uniswap V3 and will not be presented
as such.
