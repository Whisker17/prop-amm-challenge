#!/usr/bin/env bash
# Populate gas-harness/lib/vendor/ from the pinned upstream sources, then verify
# every file against vendor-pins.sha256.
#
# No third-party source is vendored into this repository. `lib/` is .gitignore'd;
# this script fetches the pins into it, and `forge build` compiles from there.
# If a fetched file does not match its recorded sha256 the script fails and
# nothing is built, because every gas number in research-out/gas/ would otherwise
# be a measurement of something other than the pinned code.
#
#   usage: ./script/fetch-vendor.sh
#
# Pins:
#   Uniswap/v2-core       tag v1.0.1 = 4dd59067c76dea4a0e8e4bfdda41877a6b16dedc
#   Uniswap/v2-periphery  ed24991304291297c3b4a52818d02f46a17aa9a2
#   Uniswap/v3-core       tag v1.0.0 = e3589b192d0be27e100cd0daaf6c97204fdb1899
#   DODO vendor copies    mantle-propamm-contracts @ 07f6797 src/vendor/dodo/
#                         (itself DODOEX/contractV2 @ 8da3ee1ec50966fca9a2c80d424040c45c0f785e)
#   Flashbots             priority-update-registry @ da53117870c7bec96d71caebe1b3f94370aba3d6
#
# The read-only reference checkouts are only ever read from.

set -euo pipefail

HARNESS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VENDOR="$HARNESS/lib/vendor"
MANIFEST="$HARNESS/vendor-pins.sha256"
SCRATCH="${SCRATCH:-${TMPDIR:-/tmp}/gasbench-vendor}"
REF="${MANTLE_PROPAMM_CONTRACTS:-$HARNESS/../../mantle-propamm-contracts}"

V2_CORE_TAG=v1.0.1
V2_CORE_SHA=4dd59067c76dea4a0e8e4bfdda41877a6b16dedc
V3_CORE_TAG=v1.0.0
V3_CORE_SHA=e3589b192d0be27e100cd0daaf6c97204fdb1899
V2_PERIPHERY_SHA=ed24991304291297c3b4a52818d02f46a17aa9a2
FLASHBOTS_SHA=da53117870c7bec96d71caebe1b3f94370aba3d6
MANTLE_SHA=07f6797

if [[ ! -f "$MANIFEST" ]]; then
  echo "error: $MANIFEST is missing; it is the record of what may be compiled" >&2
  exit 1
fi

mkdir -p "$SCRATCH"

clone_at() {
  local url="$1" ref="$2" dir="$3"
  if [[ -d "$dir/.git" ]]; then
    return
  fi
  rm -rf "$dir"
  mkdir -p "$dir"
  git -C "$dir" init --quiet .
  git -C "$dir" remote add origin "$url"
  git -C "$dir" fetch --quiet --depth 1 origin "$ref"
  git -C "$dir" checkout --quiet FETCH_HEAD
}

echo "fetching pinned upstream sources into $SCRATCH"
clone_at https://github.com/Uniswap/v2-core.git "$V2_CORE_SHA" "$SCRATCH/v2-core"
clone_at https://github.com/Uniswap/v3-core.git "$V3_CORE_SHA" "$SCRATCH/v3-core"
clone_at https://github.com/Uniswap/v2-periphery.git "$V2_PERIPHERY_SHA" "$SCRATCH/v2-periphery"

for repo in v2-core v3-core v2-periphery; do
  echo "  $repo $(git -C "$SCRATCH/$repo" rev-parse HEAD)"
done

expect_head() {
  local dir="$1" want="$2" name="$3"
  local got
  got="$(git -C "$dir" rev-parse HEAD)"
  if [[ "$got" != "$want" ]]; then
    echo "error: $name is at $got, expected $want" >&2
    exit 1
  fi
}
expect_head "$SCRATCH/v2-core" "$V2_CORE_SHA" "v2-core ($V2_CORE_TAG)"
expect_head "$SCRATCH/v3-core" "$V3_CORE_SHA" "v3-core ($V3_CORE_TAG)"
expect_head "$SCRATCH/v2-periphery" "$V2_PERIPHERY_SHA" v2-periphery

if [[ ! -d "$REF/lib/priority-update-registry" ]]; then
  echo "error: cannot find mantle-propamm-contracts at $REF" >&2
  echo "       set MANTLE_PROPAMM_CONTRACTS to its path" >&2
  exit 1
fi
echo "  mantle-propamm-contracts $(git -C "$REF" rev-parse --short HEAD) (expected $MANTLE_SHA)"
echo "  priority-update-registry $(git -C "$REF/lib/priority-update-registry" rev-parse HEAD)"
echo "                           (expected $FLASHBOTS_SHA)"

echo "populating $VENDOR"
rm -rf "$VENDOR"
mkdir -p "$VENDOR/dodo" "$VENDOR/flashbots" "$VENDOR/univ2core" \
         "$VENDOR/univ2periphery" "$VENDOR/univ3"

# DODO and Flashbots come from the read-only reference checkout.
cp "$REF/src/vendor/dodo/DecimalMath.sol" \
   "$REF/src/vendor/dodo/DODOMath.sol" \
   "$REF/src/vendor/dodo/PMMPricing.sol" "$VENDOR/dodo/"
cp "$REF/lib/priority-update-registry/src/ExamplePropAmm.sol" \
   "$REF/lib/priority-update-registry/src/PrioUpdateRegistry.sol" "$VENDOR/flashbots/"

# Uniswap comes from the pinned clones.
cp -R "$SCRATCH/v2-core/contracts" "$VENDOR/univ2core/contracts"
cp "$SCRATCH/v2-periphery/contracts/libraries/UniswapV2Library.sol" \
   "$SCRATCH/v2-periphery/contracts/libraries/SafeMath.sol" "$VENDOR/univ2periphery/"
cp -R "$SCRATCH/v3-core/contracts/libraries" "$VENDOR/univ3/libraries"
cp -R "$SCRATCH/v3-core/contracts/interfaces" "$VENDOR/univ3/interfaces"
for f in UniswapV3Pool.sol UniswapV3Factory.sol UniswapV3PoolDeployer.sol NoDelegateCall.sol; do
  cp "$SCRATCH/v3-core/contracts/$f" "$VENDOR/univ3/$f"
done

# PMMPricing.sol (vendored, byte-identical) imports `RState` from
# `../../MantlePropAmmTypes.sol`. That resolves to lib/ now that the vendored
# tree lives under lib/vendor/, so put the harness's own shim there. It is our
# file, not a pinned upstream one, so it is not in the manifest.
cp "$HARNESS/types/MantlePropAmmTypes.sol" "$HARNESS/lib/MantlePropAmmTypes.sol"

echo "verifying against $(basename "$MANIFEST")"
fail=0
missing=0
while read -r expected path; do
  [[ -z "$path" ]] && continue
  if [[ ! -f "$VENDOR/$path" ]]; then
    echo "  MISSING $path" >&2
    missing=$((missing + 1))
    continue
  fi
  actual="$(shasum -a 256 "$VENDOR/$path" | awk '{print $1}')"
  if [[ "$actual" != "$expected" ]]; then
    echo "  MISMATCH $path" >&2
    echo "    expected $expected" >&2
    echo "    actual   $actual" >&2
    fail=$((fail + 1))
  fi
done < "$MANIFEST"

extra=0
while read -r found; do
  rel="${found#"$VENDOR/"}"
  if ! grep -q "  $rel\$" "$MANIFEST"; then
    echo "  UNPINNED $rel" >&2
    extra=$((extra + 1))
  fi
done < <(find "$VENDOR" -name '*.sol' | sort)

if (( fail || missing || extra )); then
  echo >&2
  echo "vendor verification FAILED: $fail mismatched, $missing missing, $extra unpinned" >&2
  exit 1
fi

echo "ok: $(wc -l < "$MANIFEST" | tr -d ' ') files match their pins"
