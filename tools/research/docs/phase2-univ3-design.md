# Phase 2 design: Uniswap V3 (design only, not implemented)

Phase 1 (Uniswap V2 / Flashbots / DODO) is complete and green. This document is
the design for adding Uniswap V3 under the same rules. **No V3 code exists yet**;
nothing in this document has been implemented or measured.

The non-negotiable requirement: real V3 semantics — ticks, `sqrtPriceX96`, active
liquidity and cross-tick traversal. A single virtual constant product over a
fixed range is *not* Uniswap V3 and must not be presented as such.

## 1. Sources to pin

Port from `Uniswap/v3-core` at one pinned commit (recorded with per-file sha256,
exactly as the DODO and Flashbots ports are):

| File | What it gives us |
| --- | --- |
| `libraries/FullMath.sol` | `mulDiv`, `mulDivRoundingUp` (512-bit intermediate) |
| `libraries/TickMath.sol` | `getSqrtRatioAtTick`, `getTickAtSqrtRatio`, `MIN/MAX_SQRT_RATIO` |
| `libraries/SqrtPriceMath.sol` | `getNextSqrtPriceFromInput/Output`, `getAmount0Delta`, `getAmount1Delta` |
| `libraries/SwapMath.sol` | `computeSwapStep` |
| `libraries/BitMath.sol`, `libraries/TickBitmap.sol` | `nextInitializedTickWithinOneWord` |
| `libraries/Tick.sol`, `libraries/LiquidityMath.sol` | `liquidityNet` bookkeeping, `addDelta` |
| `UniswapV3Pool.sol` (`swap`) | the swap loop, state ordering, `exactInput` handling |

Fee is set to zero (`fee = 0`, `feeProtocol = 0`) to match the benchmark. That is
a *parameter*, not a formula change: `computeSwapStep` is ported verbatim and
called with `feePips = 0`.

## 2. New integer primitives required

The existing `U256` covers phase 1. V3 needs three additions, each with its own
Python-bigint reference vectors:

1. **`mul_div(a, b, denominator)` on a 512-bit intermediate.** `U256::full_mul`
   already produces the 512-bit product; the missing piece is 512 ÷ 256
   division. Generalise the existing Knuth-D routine to accept an arbitrary limb
   slice as dividend instead of a fixed `[u32; 8]`. `mulDivRoundingUp` then
   follows upstream exactly (`mulDiv` plus a remainder test).
2. **`U160` / Q64.96 handling.** `sqrtPriceX96` values are `uint160`. Keep them in
   `U256` and add explicit range assertions where upstream has
   `require(sqrtPrice <= MAX_SQRT_RATIO)`.
3. **`TickMath` constant tables.** `getSqrtRatioAtTick` is a hard-coded chain of
   magic constants; they must be transcribed literally (all 20 of them) and the
   result checked against the pinned Solidity for every tick in
   `[MIN_TICK, MAX_TICK]` at a stride plus all boundaries.

`getTickAtSqrtRatio` uses a `log2` bit-scan with its own magic constants and must
likewise be transcribed rather than replaced by a float log.

## 3. Golden vectors

Same discipline as Flashbots: **execute the pinned Solidity**, never derive
expectations from Rust.

`tools/research/solidity/univ3-golden/generate.sh` will:

1. Clone `Uniswap/v3-core` at the pinned commit into a temp dir (or copy an
   existing pinned checkout), never modifying any source repository.
2. Deploy `UniswapV3Factory`, create a `fee = 0` / `tickSpacing = 1` pool with two
   mock 18-decimal tokens (the zero-fee tier is not enabled on mainnet; enabling
   it in a local factory is a deployment parameter, not a formula change, and
   will be documented as such).
3. `initialize` at the sqrt price for price 100, mint the designed position set,
   then record, for a grid of exact-input swaps in both directions:
   `amountIn`, `amountOut`, and post-swap `sqrtPriceX96`, `tick`, `liquidity`.
4. Include swaps deliberately sized to cross 1, 2 and many initialized ticks, to
   exhaust one side of the range entirely, and to stop exactly on a tick
   boundary.

The Rust port must then match `amountOut`, `sqrtPriceX96`, `tick` and `liquidity`
integer-for-integer — the same bar the DODO port meets on its 19 vectors.

Additionally: `TickMath` and `SqrtPriceMath` get their own per-function vector
files, because a bug there is far easier to localise at the function level than
through a full swap.

## 4. State representation in the 1024-byte storage

V3 state does **not** reduce to `(reserve_x, reserve_y)`, so the adapter cannot
be a pure function of the instruction's reserves the way V2 is. Layout plan:

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 32 | reserved for the oracle field (unused: V3 consumes no oracle) |
| 32 | 32 | `sqrtPriceX96` |
| 64 | 4 | current `tick` (i32) |
| 68 | 16 | current `liquidity` (u128) |
| 84 | 4 | `tickSpacing` |
| 88 | 4 | initialized tick count `n` |
| 96 | 20·n | per tick: `tick` (i32) + `liquidityNet` (i128) |

With `n <= 40` this fits comfortably. The position set is therefore capped at 20
positions, which is ample for a realistic liquidity distribution.

Consequences to state plainly in the report:

* The V3 curve carries its own inventory in `sqrtPriceX96`/`liquidity`; the
  simulation ledger tracks the same trades in `f64`. `after_swap` advances the V3
  state from the executed amount, exactly as the DODO adapter advances its
  target/`RState`, so the two stay in lockstep to within the ledger's nano
  quantisation.
* A tick bitmap is unnecessary at this scale: `nextInitializedTickWithinOneWord`
  can be replaced by a linear scan over the stored tick array **only if** the
  scan returns the identical next-initialized tick in every case. That is a
  lookup-structure substitution, not a formula change, and the golden vectors
  (which exercise multi-tick crossings) are what proves it. If any doubt remains,
  port `TickBitmap` + `BitMath` literally instead.

## 5. Initial state and the one unavoidable deviation

The benchmark requires identical initial capital: `reserveX = 100`,
`reserveY = 10 000` at price 100.

For a position with liquidity `L` over `[pa, pb]` at current price `p`:

```
amountX = L · (1/√p − 1/√pb)
amountY = L · (√p − √pa)
```

Choosing bounds geometrically symmetric around `p` (`√pa = √p/g`, `√pb = √p·g`)
gives `p·amountX = amountY`, i.e. exactly the 50/50 value split that
`(100, 10 000)` at price 100 represents. So a symmetric range is the right shape,
and `g` is the concentration knob comparable to the Flashbots `concentration`
(with the same "local curvature only" caveat).

The deviation: tick bounds must be multiples of `tickSpacing`, and `√pa`, `√pb`,
`L` are integers, so the position cannot hold *exactly* `100` and `10 000`. Plan:
pick `tickSpacing = 1`, round the bounds to ticks, then solve `L` to match
`reserveY = 10 000` exactly and report the residual on X. At `tickSpacing = 1`
the residual is under one basis point; it will be printed in the report rather
than hidden, and the same `L`-selection rule will be used for every `g` so the
comparison stays internally consistent.

## 6. Parameter pairing

`g` (range half-width in sqrt-price terms) is the V3 analogue of the Flashbots
`concentration` and the DODO `K`. The proposed rows extend the existing table by
matching the *marginal* depth at the equilibrium point:

for a V3 position, `dY/dX` at `p` and the depth to move price by a given
fraction are both determined by `L` and `g`; solving "same price impact for a
1 X order at the equilibrium point" gives one `g` per existing table row. The
concrete values will be computed and pinned by the same `equilibrium` /
`quote-matrix` probes phase 1 already has, and the report will show, as it does
now, that the match is local only.

## 7. What V3 does and does not add to the comparison

* Like Uniswap V2, V3 consumes **no oracle**: an LP with a static position cannot
  reprice on external information. V3 therefore joins V2 as a no-oracle baseline,
  and phase 1's headline result — the oracle-aware curves lose far less to
  arbitrage — is exactly what V3 lets us quantify against a *concentrated* passive
  curve rather than a flat one.
* The interesting new dimension is range exit: once price leaves the range the
  position becomes single-sided and stops quoting on one side. The metrics already
  capture this (`retailFlowShare`, inventory deviation), and `curve_reverts` plus a
  new "steps with zero active liquidity" counter will make it explicit.

## 8. Order of work (same discipline as phase 1)

1. Write the failing tests first: `TickMath`, `SqrtPriceMath`, `SwapMath`,
   then full-swap golden vectors.
2. Add `mul_div` / 512-bit division with Python-bigint reference vectors.
3. Port the libraries verbatim; make the per-function vectors pass.
4. Port the `UniswapV3Pool.swap` loop; make the full-swap vectors pass integer
   for integer (`amountOut`, `sqrtPriceX96`, `tick`, `liquidity`).
5. Build the adapter + storage layout; verify the equilibrium mid price matches
   the other curves at the initial state.
6. Extend the quote matrix, then run smoke (20 × 1 000) and only then the full
   (1 000 × 10 000) benchmark on the same seeds as phase 1.
