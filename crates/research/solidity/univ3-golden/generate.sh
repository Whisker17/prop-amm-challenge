#!/usr/bin/env bash
# Generate Uniswap V3 golden vectors by EXECUTING the pinned v3-core Solidity.
#
# Nothing in the output is derived from a reimplementation: every expected value
# is returned by `TickMath` / `SqrtPriceMath` / `SwapMath` / `LiquidityMath` or by
# a real `UniswapV3Pool` deployed from a real `UniswapV3Factory`, all compiled
# from Uniswap/v3-core at the pinned commit.
#
# The source checkout is treated as strictly read-only: it is copied into a fresh
# temporary directory, the 0.7.6 wrappers and the 0.8.30 generator test are
# dropped into the copy, and `forge test` runs there. Nothing is written back.
#
#   usage: ./generate.sh [path-to-v3-core-checkout]
#
# With no argument the script uses (and, if absent, creates) a cached clone at
# $UNIV3_SRC_CACHE, default ${TMPDIR:-/tmp}/univ3-core-cache.
#
# Pinned source: Uniswap/v3-core tag v1.0.0
#                = commit e3589b192d0be27e100cd0daaf6c97204fdb1899
#                solc 0.7.6

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FIXTURES="$HERE/../../tests/fixtures/univ3"

EXPECTED_COMMIT="e3589b192d0be27e100cd0daaf6c97204fdb1899"
V3_TAG="v1.0.0"
V3_REMOTE="https://github.com/Uniswap/v3-core.git"

# path (relative to the checkout root) -> expected sha256
FILES=(
  "contracts/libraries/FullMath.sol:54087aee268a6938a85a408d7b14481b5c2c956c21508d5583f1bf48ec6d69ba"
  "contracts/libraries/TickMath.sol:83cf64b2ca84001effd16e007b49bac5359143b6c3132bfe42907b2426a0c5f5"
  "contracts/libraries/SqrtPriceMath.sol:ddd62e3a94346248677f30f1ab009ef015e71e4b8696dcca890eeabc9dc6c149"
  "contracts/libraries/SwapMath.sol:d6cb9a153be4ea9fb2377ef88641ef7979b5cee6933162f1b732d0289e26e1b6"
  "contracts/libraries/LiquidityMath.sol:84d20a16d5346f6ec4c12dff4df23dda5d46e52d33f18aaaaac2e9e36ce4a072"
  "contracts/UniswapV3Pool.sol:d515775b7f3ffe921dd70aca86b8bad16280fa4c122425d82b4dbea4dc564a7a"
  "contracts/UniswapV3Factory.sol:a9b78256f8a0ea95d464c96995015103e9681d0e1a144ab4e947d3de8715a38e"
)

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    sha256sum "$1" | awk '{print $1}'
  fi
}

# ---------------------------------------------------------------- source ----

SRC="${1:-}"
if [[ -z "$SRC" ]]; then
  SRC="${UNIV3_SRC_CACHE:-${TMPDIR:-/tmp}/univ3-core-cache}"
  SRC="${SRC%/}"
  if [[ ! -d "$SRC/.git" ]]; then
    echo "cloning $V3_REMOTE @ $V3_TAG into $SRC"
    git clone --quiet --depth 1 --branch "$V3_TAG" "$V3_REMOTE" "$SRC"
  fi
fi

if [[ ! -f "$SRC/contracts/UniswapV3Pool.sol" ]]; then
  echo "error: UniswapV3Pool.sol not found under $SRC" >&2
  exit 1
fi

actual_commit="$(git -C "$SRC" rev-parse HEAD 2>/dev/null || echo unknown)"
echo "source repo:   $SRC"
echo "source commit: $actual_commit (expected $EXPECTED_COMMIT)"
if [[ "$actual_commit" != "$EXPECTED_COMMIT" ]]; then
  echo "error: checkout is not at the pinned v3-core commit" >&2
  exit 1
fi

fail=0
for entry in "${FILES[@]}"; do
  rel="${entry%%:*}"
  want="${entry##*:}"
  if [[ ! -f "$SRC/$rel" ]]; then
    echo "error: missing $rel" >&2
    fail=1
    continue
  fi
  got="$(sha256_of "$SRC/$rel")"
  printf '  %-40s %s\n' "$rel" "$got"
  if [[ "$got" != "$want" ]]; then
    echo "error: $rel sha256 mismatch (expected $want)" >&2
    fail=1
  fi
done
if [[ "$fail" -ne 0 ]]; then
  echo "error: pinned source verification failed" >&2
  exit 1
fi
echo "all pinned sha256 values verified"

# --------------------------------------------------------------- forge-std --

FORGE_STD="${FORGE_STD:-}"
if [[ -z "$FORGE_STD" ]]; then
  for candidate in \
    "$HERE/../../../../../mantle-propamm-contracts/lib/priority-update-registry/lib/forge-std" \
    "${TMPDIR:-/tmp}/univ3-forge-std"; do
    if [[ -f "$candidate/src/Test.sol" ]]; then
      FORGE_STD="$candidate"
      break
    fi
  done
fi
if [[ -z "$FORGE_STD" ]]; then
  FORGE_STD="${TMPDIR:-/tmp}/univ3-forge-std"
  echo "cloning forge-std v1.15.0 into $FORGE_STD"
  git clone --quiet --depth 1 --branch v1.15.0 https://github.com/foundry-rs/forge-std.git "$FORGE_STD"
fi
echo "forge-std:     $FORGE_STD ($(git -C "$FORGE_STD" rev-parse HEAD 2>/dev/null || echo unknown))"

# ------------------------------------------------------------------ work ----

WORK="$(mktemp -d "${TMPDIR:-/tmp}/univ3-golden.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
echo "work dir:      $WORK"

cp -R "$SRC/contracts" "$WORK/contracts"
mkdir -p "$WORK/lib" "$WORK/test" "$WORK/contracts/golden"
cp -R "$FORGE_STD" "$WORK/lib/forge-std"
cp "$HERE/UniV3GoldenGen.t.sol" "$WORK/test/"

# forge 1.4.1's built-in lint rejects some legacy constructs on build, and every
# harness file must pin an exact solc. `auto_detect_solc` resolves each file's
# own compilation unit: the wrappers below and UniswapV3Pool.sol say =0.7.6, the
# generator test says =0.8.30.
cat > "$WORK/foundry.toml" <<'TOML'
[profile.default]
src = "contracts"
test = "test"
libs = ["lib"]
out = "out"
auto_detect_solc = true
optimizer = true
optimizer_runs = 200
offline = true
fs_permissions = [{ access = "read-write", path = "./" }]

[lint]
lint_on_build = false
TOML

# ---------------------------------------------------------- 0.7.6 wrappers --
#
# The 0.8.30 generator test may not import 0.7.6 sources, so the pinned
# libraries are exposed through external wrappers compiled at =0.7.6 and reached
# from the test with `deployCode` plus a locally declared interface. The
# wrappers add no arithmetic: every body is a single delegation.

cat > "$WORK/contracts/golden/TickMathWrapper.sol" <<'SOL'
// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity =0.7.6;

import '../libraries/TickMath.sol';

// External surface for Uniswap/v3-core at e3589b19, contracts/libraries/TickMath.sol.
contract TickMathWrapper {
    function getSqrtRatioAtTick(int24 tick) external pure returns (uint160) {
        return TickMath.getSqrtRatioAtTick(tick);
    }

    function getTickAtSqrtRatio(uint160 sqrtPriceX96) external pure returns (int24) {
        return TickMath.getTickAtSqrtRatio(sqrtPriceX96);
    }

    function constants()
        external
        pure
        returns (
            int24 minTick,
            int24 maxTick,
            uint160 minSqrtRatio,
            uint160 maxSqrtRatio
        )
    {
        return (TickMath.MIN_TICK, TickMath.MAX_TICK, TickMath.MIN_SQRT_RATIO, TickMath.MAX_SQRT_RATIO);
    }
}
SOL

cat > "$WORK/contracts/golden/SqrtPriceMathWrapper.sol" <<'SOL'
// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity =0.7.6;

import '../libraries/SqrtPriceMath.sol';

// External surface for Uniswap/v3-core at e3589b19, contracts/libraries/SqrtPriceMath.sol.
/// The two `getAmount0Delta` / `getAmount1Delta` upstream overloads are given
/// distinct wrapper names so the 0.8.30 caller can declare one flat interface:
///   *DeltaRounded -> the (uint160,uint160,uint128,bool roundUp) overload
///   *DeltaSigned  -> the (uint160,uint160,int128 liquidity) overload
contract SqrtPriceMathWrapper {
    function getNextSqrtPriceFromInput(
        uint160 sqrtPX96,
        uint128 liquidity,
        uint256 amountIn,
        bool zeroForOne
    ) external pure returns (uint160) {
        return SqrtPriceMath.getNextSqrtPriceFromInput(sqrtPX96, liquidity, amountIn, zeroForOne);
    }

    function getNextSqrtPriceFromOutput(
        uint160 sqrtPX96,
        uint128 liquidity,
        uint256 amountOut,
        bool zeroForOne
    ) external pure returns (uint160) {
        return SqrtPriceMath.getNextSqrtPriceFromOutput(sqrtPX96, liquidity, amountOut, zeroForOne);
    }

    function getAmount0DeltaRounded(
        uint160 sqrtRatioAX96,
        uint160 sqrtRatioBX96,
        uint128 liquidity,
        bool roundUp
    ) external pure returns (uint256) {
        return SqrtPriceMath.getAmount0Delta(sqrtRatioAX96, sqrtRatioBX96, liquidity, roundUp);
    }

    function getAmount1DeltaRounded(
        uint160 sqrtRatioAX96,
        uint160 sqrtRatioBX96,
        uint128 liquidity,
        bool roundUp
    ) external pure returns (uint256) {
        return SqrtPriceMath.getAmount1Delta(sqrtRatioAX96, sqrtRatioBX96, liquidity, roundUp);
    }

    function getAmount0DeltaSigned(
        uint160 sqrtRatioAX96,
        uint160 sqrtRatioBX96,
        int128 liquidity
    ) external pure returns (int256) {
        return SqrtPriceMath.getAmount0Delta(sqrtRatioAX96, sqrtRatioBX96, liquidity);
    }

    function getAmount1DeltaSigned(
        uint160 sqrtRatioAX96,
        uint160 sqrtRatioBX96,
        int128 liquidity
    ) external pure returns (int256) {
        return SqrtPriceMath.getAmount1Delta(sqrtRatioAX96, sqrtRatioBX96, liquidity);
    }
}
SOL

cat > "$WORK/contracts/golden/SwapMathWrapper.sol" <<'SOL'
// SPDX-License-Identifier: BUSL-1.1
pragma solidity =0.7.6;

import '../libraries/SwapMath.sol';

// External surface for Uniswap/v3-core at e3589b19, contracts/libraries/SwapMath.sol.
contract SwapMathWrapper {
    function computeSwapStep(
        uint160 sqrtRatioCurrentX96,
        uint160 sqrtRatioTargetX96,
        uint128 liquidity,
        int256 amountRemaining,
        uint24 feePips
    )
        external
        pure
        returns (
            uint160 sqrtRatioNextX96,
            uint256 amountIn,
            uint256 amountOut,
            uint256 feeAmount
        )
    {
        return SwapMath.computeSwapStep(sqrtRatioCurrentX96, sqrtRatioTargetX96, liquidity, amountRemaining, feePips);
    }
}
SOL

cat > "$WORK/contracts/golden/LiquidityMathWrapper.sol" <<'SOL'
// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity =0.7.6;

import '../libraries/LiquidityMath.sol';

// External surface for Uniswap/v3-core at e3589b19, contracts/libraries/LiquidityMath.sol.
contract LiquidityMathWrapper {
    function addDelta(uint128 x, int128 y) external pure returns (uint128) {
        return LiquidityMath.addDelta(x, y);
    }
}
SOL

# ------------------------------------------------------------------- run ----

(
  cd "$WORK"
  PER_FUNCTION_OUT="./univ3-per-function-vectors.json" \
  GOLDEN_OUT="./univ3-golden-vectors.json" \
    forge test --match-contract UniV3GoldenGenTest -vv
)

mkdir -p "$FIXTURES"
cp "$WORK/univ3-per-function-vectors.json" "$FIXTURES/per-function-vectors.json"
cp "$WORK/univ3-golden-vectors.json" "$FIXTURES/golden-vectors.json"

python3 - "$FIXTURES/per-function-vectors.json" "$FIXTURES/golden-vectors.json" <<'PY'
import json, sys

pf = json.load(open(sys.argv[1]))
print("per-function-vectors.json")
total = 0
for key, value in pf.items():
    if isinstance(value, list):
        print(f"  {key}: {len(value)}")
        total += len(value)
print(f"  TOTAL: {total} (declared {pf['counts']['total']})")

gv = json.load(open(sys.argv[2]))
print("golden-vectors.json")
gtotal = 0
for key, value in gv.items():
    if isinstance(value, list):
        print(f"  {key}: {len(value)} (declared {gv['counts'].get(key)})")
        gtotal += len(value)
        assert len(value) == gv['counts'].get(key), f"{key} count mismatch"
print(f"  TOTAL: {gtotal} (declared {gv['counts']['total']})")
assert gtotal == gv['counts']['total'], "golden total mismatch"

# Coverage the Rust side depends on. Fail loudly rather than shipping a
# fixture that silently lost a case.
swaps = [r for k, v in gv.items() if isinstance(v, list) for r in v if r.get("kind") == "swap"]
seq = gv["sequence"]
assert any(r["exactIn"] for r in swaps), "no exact-input swap vectors"
assert any(not r["exactIn"] for r in swaps), "no exact-output swap vectors"
assert any(r["zeroForOne"] for r in swaps), "no zeroForOne swap vectors"
assert any(not r["zeroForOne"] for r in swaps), "no oneForZero swap vectors"
crossings = {r.get("crossings", 0) for r in swaps + seq if not r.get("reverted")}
assert 0 in crossings, "no zero-crossing vectors"
assert any(c >= 1 for c in crossings), "no single-crossing vectors"
assert any(c >= 2 for c in crossings), "no multi-crossing vectors"
assert len(seq) >= 200, "the consecutive sequence must be at least 200 swaps"
mints = [r for k, v in gv.items() if isinstance(v, list) for r in v if r.get("kind") == "mint"]
full = [m for m in mints if m["scenario"] == "full-range"]
assert full, "no full-range mint recorded"
assert full[0]["amount0"] == "99999999999999999946", f"full-range amount0 {full[0]['amount0']}"
assert full[0]["amount1"] == "9999999999999999999946", f"full-range amount1 {full[0]['amount1']}"
assert gv["source"]["commit"] == "e3589b192d0be27e100cd0daaf6c97204fdb1899"
print(f"  crossing counts observed: {sorted(crossings)}")
print("  coverage checks passed")
PY

echo "wrote $FIXTURES/per-function-vectors.json"
echo "wrote $FIXTURES/golden-vectors.json"
sha256_of "$FIXTURES/per-function-vectors.json"
sha256_of "$FIXTURES/golden-vectors.json"
