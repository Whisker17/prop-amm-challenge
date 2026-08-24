# Foundry gas harness — DODO PMM / Flashbots ExamplePropAmm / Uniswap V2 / Uniswap V3

Real EVM gas, on the same footing, for four AMM curve families. **No Rust timing anywhere in
this directory.** Every number below was printed by `forge test` on this machine and pasted, not
computed by hand.

```
cd gas-harness
bash script/vendor-libs.sh     # once: populate lib/ from the pinned read-only checkouts
bash script/gas-snapshot.sh    # build, verify pins, verify null twins, regenerate all outputs
```

Outputs land in `../research-out/gas/`:

| file | contents |
| --- | --- |
| `gas-snapshot.csv` | one row per measured operation, 240 rows |
| `gas-snapshot.json` | the same rows plus run-level metadata |
| `touched-slots.csv` | `vm.record()` / `vm.accesses()` output for every layer B operation, 504 records |
| `bytecode-hashes.csv` | deployed runtime code size and hash of every contract layer C touches |

---

## 1. The three-number rule

**Every measured operation is reported as three numbers, always.**

| column | meaning |
| --- | --- |
| `rawGas` | what the EVM actually charged for the call as made |
| `nullTwinGas` | the same call, same calldata, against a twin with the same pragma, the same compiler settings, the same storage layout and the **same set of selectors**, with empty bodies |
| `adjustedGas` | `rawGas - nullTwinGas` |

`adjustedGas` is a **diagnostic only**. It is what is left after the function-dispatch,
calldata-copy and return-encoding overhead is removed, and it is comparable only *within one
compiler configuration*. Raw system gas and adjusted curve gas are ranked **separately** below
and neither is the conclusion on its own.

### Why the null twin must have the same selector count

Solidity dispatches external calls by **binary search over the sorted selector table**. A twin
with a different number of selectors searches a different tree and pays a different dispatch
tax. An earlier experiment in this workspace turned a true adjusted cost of 25 into 91 exactly
that way.

So every cell and every twin in this harness exposes **exactly the same seven selectors**:

```
quote(bool,uint256,uint256[6],uint256[2])
seedState(uint256[6])
seedTick(int24,int128)                        <- a NO-OP on everything except Uniswap V3
swapAlgorithmOracle(bool,uint256,uint256[2])
swapReferenceOracle(bool,uint256,bytes)
readState()
cellKind()
```

`seedTick` exists on the DODO, Flashbots and Uniswap V2 cells purely to keep the table the same
shape. `test/NullTwin.t.sol` proves the property holds, and prints the dispatch tax it buys:

```
[PASS] test_nullTwinDispatchIsIdenticalWithinACompiler()
  nullTwin quote gas, 0.8.28 (dodo): 1651
  nullTwin quote gas, 0.8.28 (flashbots): 1651
  nullTwin quote gas, 0.6.6  (univ2): 1402
  nullTwin quote gas, 0.7.6  (univ3): 1402
```

Two identically-shaped twins built by the same solc cost **exactly** the same (1651 = 1651).
Twins built by a *different* solc do not (1651 vs 1402). That 249-gas gap is the per-signature
compiler dispatch tax, it is real, and it is the reason `adjustedGas` may not be compared across
compilers. See §7.

---

## 2. Structure: a three-layer sandwich per algorithm

```
test  ──►  Adapter (=0.8.28, IDENTICAL BYTECODE for every algorithm and every twin)
             ──►  algorithm cell   at the algorithm's OWN exact pragma
             ──►  null twin        at the SAME pragma, same selectors, empty bodies
```

* **Outer shell** — `src/IAdapter.sol` + `src/Adapter.sol`, pinned at `=0.8.28`. The test only
  ever calls the `Adapter`. The cell address is an `immutable` (it lives in code, costs no
  `SLOAD`), so the shell adds no storage traffic of its own. Because the *same* `Adapter`
  bytecode fronts the real cell and the twin, the shell's cost cancels exactly in `adjustedGas`.
* **Algorithm cell** — at its own exact pragma, because that is what pins its codegen.
  `PMMPricing`, `DODOMath`, `DecimalMath`, `UniswapV2Library.getAmountOut` and
  `SwapMath.computeSwapStep` are all `internal` libraries: **solc inlines them, there is no
  `DELEGATECALL` to measure**, so the cell must be compiled by the compiler that inlines them.
* **Null twin** — one per cell (`src/cells/*/...Null.sol`). The Uniswap V3 twin inherits its
  storage layout from `src/cells/univ3/UniV3CellStorage.sol`, so "same storage layout" is
  compiler-enforced rather than promised.

Uniswap V3's A1 metric does not fit the six-word uniform state, so it gets a second, equally
symmetric shell: `src/IStepAdapter.sol` + `src/StepAdapter.sol`, again identical bytecode on
both sides.

### The uniform state and oracle words

Six state words and two oracle words, always six and two, so the **calldata cost is identical
for every algorithm**:

| algorithm | s0 | s1 | s2 | s3 | s4 | s5 | o0 | o1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| dodo | B | Q | B0 | Q0 | `uint8(RState)` | lpFeeRate | `i` (guide price, WAD) | `K` |
| flashbots | reserveX | reserveY | targetX | concentration | targetYReference | lock | multX | multY |
| univ2 | reserve0 | reserve1 | price0Cumulative | price1Cumulative | blockTimestampLast | — | ignored | ignored |
| univ3 | sqrtPriceX96 | liquidity | tick | tickSpacing | fee | — | ignored | ignored |

---

## 3. Layer A — quote maths

Inputs arrive as **calldata, never storage**, and the function is `view`/`pure`, so the measured
call is a `STATICCALL`. Opening inventory is the one from `crates/research/README.md`: fair price
100, `reserveX` 100, `reserveY` 10 000, zero fee, DODO `K = 0.1e18` paired with Flashbots
`concentration = 10`.

### Balanced state, sell base, all five trade sizes

| algorithm | 0.01% | 0.1% | 1% | 5% | 10% | nullTwin | adjusted @1% |
| --- | --- | --- | --- | --- | --- | --- | --- |
| dodo | 25 169 | 25 093 | 25 093 | 25 169 | 25 093 | 1 651 | **23 442** |
| flashbots | 2 592 | 2 592 | 2 592 | 2 592 | 2 592 | 1 651 | **941** |
| univ2-canonical-30bps | 2 130 | 2 130 | 2 130 | 2 130 | 2 130 | 1 402 | **728** |
| univ2-modified-zero-fee | 2 130 | 2 130 | 2 130 | 2 130 | 2 130 | 1 402 | **728** |
| univ3-zero-fee (A2) | 10 693 | 10 693 | 27 934 | 73 553 | 72 611 | 1 402 | **26 532** |

The DODO row moves by 76 gas between sizes: `DODOMath._sqrt` is a Newton iteration and takes one
more round trip for some inputs. That is the curve, not noise — `vm.lastCallGas().gasTotalUsed`
is exact, and `test_gasMeasurementIsExactAcrossRepeats` asserts five identical repeats.

The two Uniswap V2 rows are byte-identical in gas because `997 -> 1000` changes a constant, not
the operation count. Only the *output amount* differs (98.7157 vs 99.0099 for 1 base token in).

### Uniswap V3: TWO metrics, never conflated

The user requirement here is explicit, and the harness honours it:

* **A1 `primitiveStepGas`** — exactly **one** `SwapMath.computeSwapStep` call.
* **A2 `fullQuoteGas`** — the complete quote loop from input amount to final output, **including
  tick traversal**.

**Only A2 is comparable with the DODO / Flashbots / Uniswap V2 whole-quote numbers.** A1 is one
iteration of A2's loop and is comparable with nothing else in this document, including A2.

| metric | 0.01% | 0.1% | 1% | 5% | 10% |
| --- | --- | --- | --- | --- | --- |
| A1 primitiveStep, raw | 5 198 | 5 198 | 3 863 | 3 863 | 3 863 |
| A1 primitiveStep, adjusted (null 1 752) | 3 446 | 3 446 | 2 111 | 2 111 | 2 111 |
| A2 fullQuote, raw (buy base) | 10 867 | 10 885 | 27 599 | 79 078 | 79 050 |
| A2 tick crossings | 0 | 0 | 3 | 12 | 12 |

A1 costs *more* at the two smallest sizes because the step does not reach its target price and
takes the `getNextSqrtPriceFromInput` branch instead of the cheaper `getAmountsForRange` path.

The tick-crossing count is measured, not assumed: the harness runs the swap on a state snapshot,
reads the resulting tick, reverts, and counts the initialized ladder ticks in between. It
saturates at 11–12 because the seeded ladder has 11 initialized ticks per side; past that there
is genuinely nothing left to cross, which is why 5% and 10% cost the same.

---

## 4. Layer B — state update, with TWO oracle calipers

These are two different questions and the harness keeps them apart on purpose.

### B-algorithm-only — the number that MAY be compared across algorithms

The oracle price is delivered through the **same adapter calldata slot for every algorithm**
(`swapAlgorithmOracle(bool, uint256, uint256[6] oracle)`), so registry and calldata architecture
differences are excluded. Quote + reserve write, no risk logic, for all four.

1% of the input-side reserve, sell base, balanced:

| algorithm | warm raw | cold raw | nullTwin | adjusted (warm) |
| --- | --- | --- | --- | --- |
| univ2-canonical-30bps | 2 716 | 6 716 | 1 318 | 1 398 |
| univ2-modified-zero-fee | 2 716 | 6 716 | 1 318 | 1 398 |
| flashbots | 3 364 | 11 364 | 1 485 | 1 879 |
| univ3-zero-fee (3 crossings) | 28 573 | 38 573 | 1 318 | 27 255 |
| dodo | 46 115 | 58 115 | 1 485 | 44 630 |

### B-reference-system — a SYSTEM cost, NOT a claim about anybody's maths

Each algorithm's **real** oracle path:

| algorithm | real path exercised here | warm raw | cold raw | nullTwin |
| --- | --- | --- | --- | --- |
| dodo | pricing parameters as an ABI-decoded **calldata** struct + freshness bound (the Mantle architecture) | 46 777 | 56 777 | 1 758 |
| flashbots | unmodified `PrioUpdateRegistry.getState()` + the `_isTargetYLocked` writeback `swapXtoY` performs on every swap | 8 700 | 20 800 | 1 722 |
| univ2 | the `price0CumulativeLast` / `price1CumulativeLast` accumulator from `UniswapV2Pair._update`, via the pinned `UQ112x112` maths | 4 374 | 14 374 | 1 651 |
| univ3 | an observation written through the pinned `Oracle` library at cardinality 1 | 30 036 | 40 036 | 1 651 |

> **These are four different products.** This table must **not** be used to argue that one
> curve's *mathematics* is more expensive than another's. It says how much the surrounding price
> plumbing costs, and that plumbing was designed for four different purposes.

Two honest limitations of this table:

* The DODO reference path here decodes the calldata pricing struct and enforces the freshness
  bound, but does **not** re-derive and compare the priority-update-registry state hash the way
  `MantlePropAmmPool` does. The DODO reference number is therefore a **lower bound** on the real
  Mantle path.
* Uniswap V2's TWAP slots and Flashbots' `targetYReference` are seeded **non-zero**. A pair
  whose accumulator has never been written pays three `0 -> non-zero` `SSTORE`s (60 000 gas) on
  its first swap ever and never again; steady state is the comparable number. The one-off is
  measured separately, below.

### Cold, warm, first write, and the refund

`vm.coolSlot` is applied to the contract that **owns** the slots — the cell, never the `Adapter`
wrapper. `test_coolSlotOnWrapperDoesNothing` proves both halves of that:

```
[PASS] test_coolSlotOnWrapperDoesNothing()
  warm: 46115
  wrapper cooled: 46115      <- cooling the wrapper changes NOTHING
  cell cooled: 58115         <- cooling the owner costs exactly 6 x 2000
```

The six-word layout is shared by every cell, so these storage numbers are the harness-wide
baseline (measured on the DODO cell):

| operation | rawGas (gross, as metered) | nullTwin | gasRefunded |
| --- | --- | --- | --- |
| `seedState-firstWrite-0-to-nonzero` (fresh cell) | 94 516 | 1 097 | 0 |
| `seedState-overwrite-nonzero` (warm) | 2 316 | 1 097 | 0 |
| `seedState-zeroing-nonzero-to-0-sameTx` | 2 316 | 1 097 | **79 600** |
| `seedState-zeroing-nonzero-to-0-vmStoreSeeded` | 2 316 | 1 097 | **79 600** |

`rawGas` is the **gross** charge. `gasRefunded` is the EIP-3529 refund counter, which is applied
at transaction end and capped at `gasUsed / 5`. 79 600 = 4 × 19 900, i.e. the four non-zero slots
of the balanced DODO state.

**Disclosed limitation:** a forge test *is* one transaction, so a slot seeded inside the test has
a transaction-original value of 0 and EIP-2200 charges the dirty-slot price (100/slot) with the
full 19 900/slot refund. Seeding with `vm.store` instead gives the identical result — `vm.store`
journals like a normal write. This harness therefore **cannot** reach the cross-transaction case
(2 900/slot charge, 4 800/slot refund) from inside a single test, and both rows say so rather
than pretending otherwise.

### Touched slots

Enumerated with `vm.record()` / `vm.accesses()` and published in
`../research-out/gas/touched-slots.csv` (504 records). Excerpt — DODO, algorithm-only caliper:

```
algorithm,operation,accessKind,slot
dodo,stateUpdate-algorithmOracle,read,5
dodo,stateUpdate-algorithmOracle,read,4
dodo,stateUpdate-algorithmOracle,read,0
dodo,stateUpdate-algorithmOracle,read,1
dodo,stateUpdate-algorithmOracle,read,2
dodo,stateUpdate-algorithmOracle,read,3
dodo,stateUpdate-algorithmOracle,read,0
dodo,stateUpdate-algorithmOracle,read,1
dodo,stateUpdate-algorithmOracle,read,2
dodo,stateUpdate-algorithmOracle,read,4
dodo,stateUpdate-algorithmOracle,write,0
```

(`vm.accesses` is captured into **memory** and only pushed to the CSV after the last
`vm.revertToState`: `slotLines` is test-contract storage, and a state revert rolls that back too.
Getting this wrong silently produced an empty file on the first attempt.)

---

## 5. Layer C — end-to-end swap, real pools, real tokens, real transfers

One `TestToken` for every algorithm (§8 has its hashes), one `EndToEndRunner` as the trader, one
external call from the measured call to the pool in every case.

### Main table — zero fee, plus the token-only control

Reported **alongside** raw gas, never subtracted from it:

| algorithm | 0.01% | 0.1% | 1% | 5% | 10% | crossings @5% |
| --- | --- | --- | --- | --- | --- | --- |
| *token-only control* | *35 384* | *35 384* | *35 384* | *35 384* | *35 384* | — |
| univ2-modified-zero-fee | 58 186 | 58 186 | 58 186 | 58 186 | 58 186 | — |
| univ2-canonical-30bps | 58 612 | 58 612 | 58 612 | 58 612 | 58 612 | — |
| univ3-zero-fee | 65 986 | 65 986 | 125 979 | 267 530 | 266 588 | 11 |
| dodo (harness pool) | 98 254 | 98 178 | 98 178 | 98 254 | 98 178 | — |
| flashbots (real ExamplePropAmm) | 103 700 | 103 700 | 103 700 | 103 700 | 103 700 | — |

(sell base; the buy-base rows are in the CSV and differ by at most ~1 700.)

The control contains **no curve**: one `TestToken` transfer out, one external call, one transfer
back — the Uniswap V2 call shape with the arithmetic and the reserve slots removed. It is the
same 35 384 at every size, which is what you would expect from a pure transfer pair, and it is
roughly 60% of the cheapest end-to-end number. That is the point of publishing it.

**The `nullTwinGas` column on every layer C row is that control**, not a same-pragma empty-body
twin — there is no such thing as an "empty pool" that still moves tokens. So layer C still
carries all three numbers, with the baseline named explicitly in every row's `note`:

| algorithm (1%, sell base) | rawGas | nullTwinGas (= control) | adjustedGas |
| --- | --- | --- | --- |
| token-only-control | 35 384 | 0 *(it IS the baseline)* | 35 384 |
| univ2-modified-zero-fee | 58 186 | 35 384 | 22 802 |
| univ2-canonical-30bps | 58 612 | 35 384 | 23 228 |
| dodo (harness pool) | 98 178 | 35 384 | 62 794 |
| flashbots (real ExamplePropAmm) | 103 700 | 35 384 | 68 316 |
| univ3-zero-fee (3 crossings) | 125 979 | 35 384 | 90 595 |

The layer C `adjustedGas` is **not** "the curve's gas": it is everything the pool does that a
bare transfer pair does not, which includes the reentrancy guard, the reserve reads and writes,
the oracle path, the callback and the events, as well as the arithmetic. Raw gas is the number to
quote; the control is published so nobody has to guess how much of it is ERC-20 traffic.

### DODO and Flashbots are NOT rankable against each other in layer C

`src/pools/DodoPool.sol` is a **minimal pool this harness wrote** around the pinned `PMMPricing`
library. `ExamplePropAmm` is a **complete production contract**: pair registry, a cross-contract
read of `PrioUpdateRegistry`, `Ownable`, `ReentrancyGuard`, `SafeERC20`. Those are artefacts at
two different levels of completeness, not two implementations of the same thing, so the two layer
C rows do **not** constitute a ranking of DODO against Flashbots.

The data makes the trap visible rather than hiding it: in layer C, DODO is **cheaper** (98 178 vs
103 700) — **the opposite order from layers A and B**, where DODO is several times dearer. That
reversal measures wrapper completeness, not curve maths. It does not overturn the layer A/B
conclusion, and it must not be quoted in the other direction either.

A product-level DODO-vs-Flashbots gas comparison needs the real `MantlePropAmmPool` deployed in
this harness. It is not here, so this harness does not make that claim.

### Uniswap V2 appears TWICE, clearly separated, never mixed

| variant | what it is |
| --- | --- |
| `univ2-canonical-30bps` | **production reality.** Unmodified `UniswapV2Pair` at v2-core `v1.0.1`, 30 bps. |
| `univ2-modified-zero-fee` | **benchmark only.** The minimal patch below, so V2 sits on the same zero-fee footing as the others. |

The whole patch, `patches/uniswap-v2-pair-zero-fee.patch`:

```diff
--- src/vendor/univ2core/contracts/UniswapV2Pair.sol
+++ src/patched/UniswapV2PairZeroFee.sol
@@ -1,14 +1,14 @@
 pragma solidity =0.5.16;

-import './interfaces/IUniswapV2Pair.sol';
+import '../vendor/univ2core/contracts/interfaces/IUniswapV2Pair.sol';
   ... (six more import-path rewrites, forced by moving the file)

-contract UniswapV2Pair is IUniswapV2Pair, UniswapV2ERC20 {
+contract UniswapV2PairZeroFee is IUniswapV2Pair, UniswapV2ERC20 {
@@ -177,8 +177,8 @@
-        uint balance0Adjusted = balance0.mul(1000).sub(amount0In.mul(3));
-        uint balance1Adjusted = balance1.mul(1000).sub(amount1In.mul(3));
+        uint balance0Adjusted = balance0.mul(1000);
+        uint balance1Adjusted = balance1.mul(1000);
```

Plus `patches/uniswap-v2-library-zero-fee.patch`, which is line 46, `997 -> 1000`, the library
rename, and one import path.

Three things about the patch:

1. **Contamination avoided.** The patch lives in `src/patched/`, a directory that exists *only*
   for this purpose. It imports every other file — the ERC-20 base, the interfaces, `Math`,
   `UQ112x112`, `SafeMath` — from the untouched `src/vendor/univ2core/` tree. Not one byte of any
   vendored upstream file changed; `test/VendorPins.t.sol` asserts that by sha256, and
   `script/refetch-vendor-sources.sh` re-clones the pins and diffs them (it currently reports
   *"ok: src/vendor matches every Uniswap pin byte for byte"*). Nothing was ever written into
   either read-only repository.
2. **The rename is forced.** Forge keys build artifacts on the *source file name*, so two files
   both called `UniswapV2Pair.sol` would collide in `out/`. The contract had to be renamed with
   the file.
3. **`pairFor` is not patched and is never called.** Both pairs are deployed **directly** and
   their addresses kept, so the hard-coded init-code hash in `UniswapV2Library.pairFor` is never
   consulted and cannot silently point at the wrong pair. Deploying directly makes the *deployer*
   the pair's `factory`, so the test fixture implements `feeTo()` returning `address(0)` — which
   is exactly what the real factory returns with the protocol fee off, i.e. its mainnet state.

The two runtime bytecodes differ by 22 bytes (11 293 vs 11 271) and
`test_recordBytecodeHashes` fails the suite if they are ever equal — if they were, the patch had
not taken effect and the two rows would be the same measurement twice.

| variant | runtime code hash |
| --- | --- |
| `UniswapV2Pair` (canonical) | `0xa5ecf2d559601852d41aea8f685fa2e9e71414ffaea8742a0ee60ed00681be0f` |
| `UniswapV2PairZeroFee` (benchmark) | `0x62de0e3f234e4222306198bffae51ae73b291315ae49c85ed0ea711d8292fcf1` |

### Uniswap V3: zero fee needs no upstream change

`UniswapV3Factory.enableFeeAmount(0, 60)` succeeds — `fee` has no lower bound — so the zero-fee
V3 pool is the **real, unmodified** `UniswapV3Pool` from v3-core `v1.0.0`. End-to-end gas is
measured separately for 0, 1-few and many initialized tick crossings; the crossing count in the
CSV is read from the pool's own `slot0().tick` before and after, not assumed.

### The separate production-fee sensitivity table

The main comparison is zero-fee. Production-fee measurements are tagged
`layer = C-fee-sensitivity` so they cannot be mixed in by accident:

| algorithm | direction | 1% raw |
| --- | --- | --- |
| `univ3-production-3000` (real pool, canonical 0.30% tier) | sell-base | 210 136 |
| `univ3-production-3000` | buy-base | 179 914 |
| `dodo-production-lpFee-3bps` (`lpFeeRate = 3e14`) | sell-base | 98 178 |
| `dodo-production-lpFee-3bps` | buy-base | 96 835 |

`univ2-canonical-30bps` is the V2 production-fee measurement and already appears as its own,
separately labelled row in the main table.

---

## 6. Two rankings, stated separately

**Raw system gas, whole quote (layer A, 1%, balanced, sell base)** — cheapest first:

1. univ2 (either variant) 2 130
2. flashbots 2 592
3. dodo 25 093
4. univ3 A2 27 934 *(3 tick crossings; 10 693 at 0 crossings, which would move it to #3)*

**Adjusted curve gas (same rows, `raw - nullTwin`)** — cheapest first:

1. univ2 728
2. flashbots 941
3. dodo 23 442
4. univ3 A2 26 532 *(9 291 at 0 crossings)*

**In this data set the two rankings agree.** They are still published separately, because they
are only guaranteed to agree while the raw gaps are large compared with the cross-compiler
dispatch tax. That tax is 1 651 − 1 402 = **249 gas** here (§1), so any raw gap narrower than
249 gas between a 0.8.28 algorithm and a 0.6.6/0.7.6 algorithm *would* reverse under
subtraction. The narrowest cross-compiler gap actually observed is
flashbots 2 592 − univ2 2 130 = 462, which is wide enough that the order survives; the univ3 A2
figure only changes rank because its tick-crossing count changes, not because of the tax. Do not
assume the agreement holds for a different curve, a different size ladder or a different solc.

**Raw end-to-end gas (layer C, 1%, sell base)** — measurements, ordered, **not a ranking**:

| algorithm | rawGas | what the contract actually is |
| --- | --- | --- |
| *token-only control* | *35 384* | *no curve at all* |
| univ2-modified-zero-fee | 58 186 | upstream pair, one-line fee patch |
| univ2-canonical-30bps | 58 612 | upstream pair, unmodified |
| univ3-zero-fee | 65 986 @0 / 125 979 @3 | upstream pool, unmodified |
| dodo | 98 178 | **harness-authored minimal pool** |
| flashbots | 103 700 | **complete production contract** |

The last two rows are ordered here only because the table is sorted; they are **not comparable**
(§5). Note that this order is the *reverse* of layers A and B — that is the wrapper-completeness
gap, not a curve result.

### The one cross-algorithm conclusion this harness supports

**Under a unified quote and state-update methodology, Flashbots is clearly cheaper than DODO.**

| methodology | flashbots | dodo | ratio |
| --- | --- | --- | --- |
| layer A whole quote, raw, median | 2 588 | 24 426 | 9.4× |
| layer B algorithm-only state update, raw, median | 3 355 | 44 321 | 13.2× |

Both calipers point the same way and both hold architecture and oracle delivery fixed. Nothing
else here is a cross-algorithm ranking: layer C compares artefacts of different completeness,
the reference-system oracle column compares products rather than curves, `adjustedGas` is a
diagnostic, and V3's A1 is not a quote. The question "which curve is cheaper in production"
requires the real `MantlePropAmmPool` and is **not** answered by this harness.

---

## 7. Confounders — disclosed, not hidden

1. **Per-signature compiler dispatch tax, whose sign depends on which compiler you subtract.**
   The identical seven-selector twin costs 1 651 under solc 0.8.28 and 1 402 under both 0.6.6 and
   0.7.6. Every cross-compiler `adjustedGas` comparison silently contains that ±249-gas term:
   subtracting a 0.8.28 twin removes 249 gas *more* than subtracting a 0.7.6 twin, so the same
   pair of curves can rank one way on raw gas and the other way on adjusted gas whenever their
   raw gap is under 249. It happens not to reverse any ranking in the numbers published here
   (§6), and that is luck about the size of the gaps, not a property of the method.
   `adjustedGas` is only safe *within* one compiler configuration.
2. **Selector-table shape.** Mitigated as far as it can be: all seven selectors on every cell
   and every twin, asserted in `test/NullTwin.t.sol`. Not mitigated for the A1 shell, which has
   one selector and a different signature — which is one more reason A1 is comparable with
   nothing.
3. **Pool and proxy architecture, and call depth.** Layer C entry points *cannot* have one
   shape: V2 is "transfer then call", V3 is "call then pay in a callback", DODO and
   ExamplePropAmm are "approve then call". That is real architecture, it is a real cost, and no
   amount of harness design can normalise it away without ceasing to measure the real contracts.
   Layer B's algorithm-only caliper is the number that *is* architecture-neutral.
4. **The oracle read paths are different products.** A calldata struct, a
   `PrioUpdateRegistry` lane, a `UQ112x112` TWAP accumulator and a V3 observation ring are not
   four implementations of one thing. See the warning in §4.
5. **Token transfer implementation.** One `TestToken` for everything, deliberately minimal and
   deliberately un-optimised, with its source and runtime hashes recorded (§8). A fee-on-transfer
   token, a rebasing token or a blocklist check would move every layer C number by hundreds of
   gas for reasons unrelated to any curve.
6. **Each algorithm is compiled with its own canonical config, because its pragma pins it.**
   0.5.16 / 999999 / istanbul for the V2 pair, 0.6.6 / 999999 / istanbul for the V2 quote
   library, 0.7.6 / 800 / istanbul for V3, 0.8.28 / 200 / prague for DODO and Flashbots. These
   are the upstream projects' own build settings (§9), not this harness's preference. They are
   not interchangeable and the resulting bytecode is not comparable instruction-for-instruction.
7. **Codegen target vs execution spec.** The *execution* EVM is Prague for every algorithm, so
   EIP-2929 cold/warm and EIP-3529 refunds are identical for all of them. The *codegen* target
   differs (istanbul for the older compilers, prague for 0.8.28) because solc 0.5.16 cannot
   target prague. This affects instruction selection slightly; it does not affect the gas
   schedule the measurements run under.
8. **Storage packing.** The layer A / layer B cells all use the same six full words, on purpose,
   so those layers compare like with like. The *real* contracts do not: `UniswapV2Pair` packs
   `reserve0`/`reserve1`/`blockTimestampLast` into a single slot, `ExamplePropAmm` spreads
   `TradingPair` over several, `UniswapV3Pool` packs `slot0`. Layer C measures the real
   contracts and therefore the real packing.
9. **Harness-authored code, labelled as such.** Three pieces are not upstream:
   * `src/cells/univ3/UniV3FullQuoteCell.sol` — a line-by-line transcription of
     `UniswapV3Pool.swap`'s loop with `feeGrowthGlobal`/protocol fee, `ticks.cross` (replaced by
     a direct `liquidityNet` read, because a `view` quote cannot write) and the final state
     writes removed. Every arithmetic call is the pinned upstream library. All three removals
     make the number **smaller**, and the real pool is measured end-to-end in layer C.
   * `src/pools/DodoPool.sol` — a minimal pool around the pinned `PMMPricing`. **Not a deployed
     DODO contract.** DODO ships DVM/DSP/DPP and Mantle ships `MantlePropAmmPool`; each wraps the
     same library in a different amount of vault, LP-token, risk-cap and replay-protection
     machinery, and measuring any of them would have measured *that wrapper*. This pool is
     matched to the `UniswapV2Pair` call shape (one contract, one pull, one push) so the layer C
     row is comparable with V2 — and it is **not** comparable with a production DODO deployment.
   * `src/cells/flashbots/FlashbotsCell.sol` — `_quoteXtoY` / `_quoteYtoX` transcribed with the
     operation order, division points and operand order preserved; only the source of the pair
     fields changed. The real `ExamplePropAmm` is measured unmodified in layer C.
10. **Trade-size saturation in the V3 tick ladder.** The seeded ladder has 11 initialized ticks
    per side. Past that the price keeps moving but there is nothing left to cross, which is why
    the 5% and 10% V3 rows report the same crossing count and nearly the same gas.
11. **`vm.lastCallGas().gasTotalUsed`, not `gasleft()` deltas.** The former is exact (asserted
    over five repeats); the latter is not. The primary number is never a `gasleft()` delta.
12. **Every measurement is warmed once first.** The cold-account surcharge (2 600) is paid by a
    throwaway call before the measured one, so it is not attributed to a curve. Cold *storage* is
    reported separately and deliberately, via `vm.coolSlot`.

---

## 8. `TestToken` — the one token

| | |
| --- | --- |
| source | `src/TestToken.sol`, sha256 `184014564e9b65370e74083cfd0533de84eefc59df5426f3718307586d69c1f0` |
| runtime code hash | `0x3bf3d6d196134e6d841d22ae8ddb033181df908a63f8ea904b5bf8da9822d39d` |
| runtime code size | 1 634 bytes |
| decimals | 18 (required: `ExamplePropAmm.createPair` enforces `decimalsX + xRetain == decimalsY + yRetain`) |

Full list of layer C runtime hashes: `../research-out/gas/bytecode-hashes.csv`.

---

## 9. Every pin

### Upstream sources, verified by sha256 in `test/VendorPins.t.sol`

| repository | pin | files vendored under `src/vendor/` | upstream build settings |
| --- | --- | --- | --- |
| `Uniswap/v2-core` | tag `v1.0.1` = `4dd59067c76dea4a0e8e4bfdda41877a6b16dedc` | `univ2core/contracts/**` | `.waffle.json`: solc 0.5.16, optimizer on, runs 999999, evm istanbul |
| `Uniswap/v2-periphery` | `ed24991304291297c3b4a52818d02f46a17aa9a2` | `univ2periphery/UniswapV2Library.sol`, `SafeMath.sol` | `.waffle.json`: solc 0.6.6, optimizer on, runs 999999, evm istanbul |
| `Uniswap/v3-core` | tag `v1.0.0` = `e3589b192d0be27e100cd0daaf6c97204fdb1899` | `univ3/**` | `hardhat.config.ts`: solc 0.7.6, optimizer on, runs 800, `bytecodeHash: 'none'` |
| `flashbots/priority-update-registry` | `da53117870c7bec96d71caebe1b3f94370aba3d6` | `flashbots/ExamplePropAmm.sol`, `PrioUpdateRegistry.sol` | `foundry.toml`: optimizer on, runs 200; no pinned solc (pragmas `^0.8.20` / `0.8.28`), pinned here to 0.8.28 |
| `mantle-propamm-contracts` (read-only) | `07f6797`, vendoring `DODOEX/contractV2` `8da3ee1ec50966fca9a2c80d424040c45c0f785e` | `dodo/{DecimalMath,DODOMath,PMMPricing}.sol` | `foundry.toml`: solc 0.8.28, optimizer on, runs 200, evm prague, `bytecode_hash = "none"` |

Individual file hashes (sha256, matching `shasum -a 256`, all asserted in
`test/VendorPins.t.sol`):

```
90f688a26a7c6ad63b7f84b1c04cd61609540e14269f26810ad2cd80004c448a  src/vendor/dodo/DODOMath.sol
27d9d19a79982c79bd9faa5ea2c2039be319b256acc898de282179d7bf256352  src/vendor/dodo/DecimalMath.sol
093b9adab96a57230d240984860c23580a2407e6085a3e57948d820ffe50807e  src/vendor/dodo/PMMPricing.sol
5b22ac480c2e2145fc0dbe005361b447f7c8fbc1d56a72671b4cfe6ceb177ca1  src/vendor/flashbots/ExamplePropAmm.sol
e8797fbd1d2330e918209e360ee9f98d09813df1b0fee620d657585de8ded234  src/vendor/flashbots/PrioUpdateRegistry.sol
43a5421b31415868367b62bfa161ca10bcee03778873faad905f5a3e2cce9cbd  src/vendor/univ2core/contracts/UniswapV2Pair.sol
e0cef3e874a68cbcc5986451b1fec180ba5ff5699f27a256b7c10fafefe36b99  src/vendor/univ2core/contracts/UniswapV2Factory.sol
4f83e9334f833568fa47b36e9ceca435f6c2962760a0596b043c4e538d0fd9f2  src/vendor/univ2periphery/UniswapV2Library.sol
54087aee268a6938a85a408d7b14481b5c2c956c21508d5583f1bf48ec6d69ba  src/vendor/univ3/libraries/FullMath.sol
83cf64b2ca84001effd16e007b49bac5359143b6c3132bfe42907b2426a0c5f5  src/vendor/univ3/libraries/TickMath.sol
ddd62e3a94346248677f30f1ab009ef015e71e4b8696dcca890eeabc9dc6c149  src/vendor/univ3/libraries/SqrtPriceMath.sol
d6cb9a153be4ea9fb2377ef88641ef7979b5cee6933162f1b732d0289e26e1b6  src/vendor/univ3/libraries/SwapMath.sol
d515775b7f3ffe921dd70aca86b8bad16280fa4c122425d82b4dbea4dc564a7a  src/vendor/univ3/UniswapV3Pool.sol
a9b78256f8a0ea95d464c96995015103e9681d0e1a144ab4e947d3de8715a38e  src/vendor/univ3/UniswapV3Factory.sol
3b9fa1b914db525f63d52beb626626db5a960e8dc1b43e5edb0c5a1cac509656  src/patched/UniswapV2PairZeroFee.sol
f6975232b81eba0ff5dd74fddda0f41a0b05ff128d1cc821eb56d2c20547e0d1  src/patched/UniswapV2LibraryZeroFee.sol
```

### Toolchain

| | |
| --- | --- |
| forge | `1.4.1-v1.4.1`, commit `cf7746048646f2ecff48246dd61e265e49ab16f0` |
| execution EVM spec | `prague`, identical for every algorithm |
| solc versions used | 0.5.16, 0.6.6, 0.7.6, 0.8.28 — all from the local svm cache, `offline = true` |
| forge-std | from `mantle-propamm-contracts/lib/forge-std` @ `77041d2ce690e692d6e03cc812b57d1ddaa4d505` |
| OpenZeppelin | 5.4.0, from `mantle-propamm-contracts/lib/openzeppelin-contracts` |
| solady | from `priority-update-registry/lib/solady` |

### Two forge configuration facts this harness depends on

* `[lint] lint_on_build = false` — forge 1.4.1's built-in lint makes v2-core exit 1 on legacy
  Yul, and there is no `--no-lint` flag.
* Per-path compiler pinning needs **both** `[[profile.default.additional_compiler_profiles]]`
  (candidate settings) **and** `[[profile.default.compilation_restrictions]]` (which files must
  satisfy which constraints). Restrictions alone produce
  `Missing profile satisfying settings restrictions for ...`. Note the capitalised
  `bytecode_hash = "None"` inside those tables, against lowercase `"none"` at profile level.
* Only the concrete v2-core contracts are pinned to `=0.5.16`; `interfaces/` and `libraries/`
  carry floating `>=0.5.0` pragmas and are pulled into whichever compilation unit imports them
  (0.5.16 for the pair, 0.6.6 for the periphery library) — which is what upstream does.
  `UQ112x112.sol` carries an **exact** `=0.5.16` and therefore cannot be imported into the 0.6.6
  V2 cell; its two functions are transcribed inline there, with the note attached.

---

## 10. Layout

**No third-party source is committed to this repository.** `lib/` is `.gitignore`d and is
rebuilt from the pins by `script/fetch-vendor.sh`, which verifies every file's sha256 against
`vendor-pins.sha256` and aborts before anything is compiled if one differs.

```
gas-harness/
  foundry.toml                     four compilers, one project; per-path pinning
  remappings.txt                   vendor/ -> lib/vendor/, mantle-types/ -> lib/
  .gitattributes                   *.patch is exempt from whitespace checks (context lines)
  README.md                        this file
  vendor-pins.sha256               the 52 upstream files that may be compiled
  patches/                         the two zero-fee diffs (audit record, not build input)
  types/
    MantlePropAmmTypes.sol         the RState enum only, so the DODO copies stay byte-identical
  script/
    vendor-libs.sh                 forge-std / solady / openzeppelin into lib/
                                   (test-harness deps, NOT the measured code)
    fetch-vendor.sh                clone the pins, verify every sha256, populate lib/vendor/
    regenerate-patches.sh          rebuild patches/ from the pinned sources
    render-gas-report.py           REPORT-gas.zh-CN.md from gas-snapshot.json
    gas-snapshot.sh                the whole pipeline; regenerates research-out/gas
  lib/                             GITIGNORED. Written by the two script/ fetchers:
    vendor/                          the pinned upstream sources, byte-identical
    MantlePropAmmTypes.sol           copy of types/, so PMMPricing's relative import resolves
    forge-std/ solady/ openzeppelin-contracts/   test-harness dependencies
  src/
    IAdapter.sol  Adapter.sol      the ONE outer shell (=0.8.28)
    IStepAdapter.sol StepAdapter.sol   the A1-only shell (=0.8.28)
    TestToken.sol                  the ONE token
    cells/dodo/                    DodoCell + DodoCellNull                (=0.8.28)
    cells/flashbots/               FlashbotsCell + FlashbotsCellNull      (=0.8.28)
    cells/univ2/                   UniV2Cell, UniV2CellZeroFee, UniV2CellNull   (=0.6.6)
    cells/univ3/                   UniV3CellStorage, UniV3FullQuoteCell(+Null),
                                   UniV3StepCell(+Null)                  (=0.7.6)
    patched/                       UniswapV2PairZeroFee, UniswapV2LibraryZeroFee
    pools/                         DodoPool, EndToEndRunner
    targets/                       BuildUniV2Core, BuildUniV3Core — import-only, so the
                                   lib/ contracts `deployCode` needs get compiled
  test/
    Fixtures.sol                   states, size ladder, cross-compiler deployment
    LayerCFixtures.sol             real pools, real tokens
    VendorPins.t.sol               sha256 of every vendored file
    NullTwin.t.sol                 selector-set, dispatch, coolSlot and determinism guards
    Bytecode.t.sol                 layer C runtime code hashes
    Probe.t.sol                    smoke: every cell quotes a sane number
    RecordProbe.t.sol              proves vm.record/vm.accesses survives snapshot reverts
    LayerC.t.sol                   smoke: every real pool executes a real swap
    GasSnapshot.t.sol              the sweep that writes research-out/gas
```

Outputs, all regenerated by `./script/gas-snapshot.sh`:

```
research-out/gas/gas-snapshot.csv       one row per measured operation
research-out/gas/gas-snapshot.json      the same rows + run metadata, including
                                        benchmarkCommit and benchmarkWorkingTreeDirty
research-out/gas/touched-slots.csv      layer B vm.record()/vm.accesses()
research-out/gas/bytecode-hashes.csv    layer C deployed runtime code hashes
research-out/gas/REPORT-gas.zh-CN.md    generated report (do not hand-edit)
research-out/gas/MANIFEST.sha256        sha256 of all of the above
```

`benchmarkCommit` is stamped by the shell, not by Solidity — the generator cannot ask git
anything. A bare `forge test` therefore rewrites the JSON *without* provenance;
`render-gas-report.py` refuses to run on such a file rather than emit an untraceable report.

## 11. Test matrix actually covered

| dimension | layer A | layer B | layer C |
| --- | --- | --- | --- |
| states | balanced, off-target | balanced, off-target | balanced |
| directions | buy, sell | buy, sell | buy, sell |
| trade sizes | 0.01 / 0.1 / 1 / 5 / 10 % | 1 % | 0.01 / 0.1 / 1 / 5 / 10 % |
| fees | zero-fee main; 30 bps V2 as its own labelled row | as layer A | zero-fee main; `C-fee-sensitivity` table separate |
| cold / warm | n/a (`STATICCALL`, no storage) | both, plus first-write and zeroing | warm |
| V3 tick crossings | measured per row (0 / 3 / 11 / 12) | measured per row | measured per row (0 / 3 / 11 / 12) |

Layer C is measured in the balanced state only. Extending it to the off-target state means
re-seeding four real pools consistently, which the harness does not yet do; that is a gap, not a
result.
