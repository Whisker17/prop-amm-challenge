#!/usr/bin/env bash
# Generate Uniswap V3 FullMath / SafeCast / LiquidityMath reference vectors by
# EXECUTING the pinned Solidity at solc 0.7.6.
#
# The pinned source repository is treated as strictly read-only: the three
# libraries are copied into a temporary forge project, the generator is dropped
# in next to them, and `forge test` runs there. Nothing is written back.
#
#   usage: ./generate.sh [path-to-v3-core-checkout]
#
# Default source path: $V3_CORE_SRC, else /tmp/v3core-git. Obtain it with
#
#   git clone https://github.com/Uniswap/v3-core /tmp/v3core-git
#   git -C /tmp/v3core-git checkout e3589b192d0be27e100cd0daaf6c97204fdb1899
#
# Pinned commit: e3589b192d0be27e100cd0daaf6c97204fdb1899 (tag v1.0.0)

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC="${1:-${V3_CORE_SRC:-/tmp/v3core-git}}"
DEST_DIR="$HERE/../../tests/fixtures/u256"
EXPECTED_COMMIT="e3589b192d0be27e100cd0daaf6c97204fdb1899"

# file:sha256 pairs, verified before anything is executed.
PINNED_FILES=(
  "FullMath.sol:54087aee268a6938a85a408d7b14481b5c2c956c21508d5583f1bf48ec6d69ba"
  "SafeCast.sol:9aed494b56d3dd16b7d6535583ded2cdfb03dc80aaa919347b13d35fd597e8bf"
  "LiquidityMath.sol:84d20a16d5346f6ec4c12dff4df23dda5d46e52d33f18aaaaac2e9e36ce4a072"
)

if [[ ! -d "$SRC/contracts/libraries" ]]; then
  echo "error: $SRC/contracts/libraries not found (pass a v3-core checkout as \$1)" >&2
  exit 1
fi

actual_commit="$(git -C "$SRC" rev-parse HEAD 2>/dev/null || echo unknown)"
echo "source repo:   $SRC"
echo "source commit: $actual_commit (expected $EXPECTED_COMMIT)"
if [[ "$actual_commit" != "$EXPECTED_COMMIT" ]]; then
  echo "warning: repository HEAD differs from the pinned commit" >&2
fi

for entry in "${PINNED_FILES[@]}"; do
  file="${entry%%:*}"
  expected="${entry##*:}"
  actual="$(shasum -a 256 "$SRC/contracts/libraries/$file" | awk '{print $1}')"
  echo "  $file $actual (expected $expected)"
  if [[ "$actual" != "$expected" ]]; then
    echo "error: $file does not match the pinned sha256" >&2
    exit 1
  fi
done

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fullmath-vectors.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
echo "work dir:      $WORK"

mkdir -p "$WORK/src/v3-core" "$WORK/test"
for entry in "${PINNED_FILES[@]}"; do
  cp "$SRC/contracts/libraries/${entry%%:*}" "$WORK/src/v3-core/"
done
cp "$HERE/V3MathWrapper.sol" "$WORK/src/"
cp "$HERE/V3MathVectorGen.t.sol" "$WORK/test/"

# No lib/, no remappings, no network: the generator declares the cheatcode
# interface itself, so the project needs nothing but solc. `lint_on_build` must
# be off (forge 1.4.1's linter has no CLI opt-out), and every source file here
# carries an exact pragma so solc selection is reproducible.
cat > "$WORK/foundry.toml" <<'TOML'
[profile.default]
src = 'src'
test = 'test'
out = 'out'
libs = []
auto_detect_solc = true
offline = true
fs_permissions = [{ access = "read-write", path = "./" }]

[lint]
lint_on_build = false
TOML

(
  cd "$WORK"
  FULLMATH_OUT="./fullmath-solidity-vectors.json" \
  SAFECAST_OUT="./safecast-solidity-vectors.json" \
    forge test --match-contract V3MathVectorGenTest -vv
)

mkdir -p "$DEST_DIR"
cp "$WORK/fullmath-solidity-vectors.json" "$DEST_DIR/"
cp "$WORK/safecast-solidity-vectors.json" "$DEST_DIR/"

python3 - "$DEST_DIR/fullmath-solidity-vectors.json" "$DEST_DIR/safecast-solidity-vectors.json" <<'PY'
import json, sys
for path in sys.argv[1:]:
    d = json.load(open(path))
    reverts = sum(1 for v in d["vectors"] if v["reverted"])
    print(f"{path}: count={d['count']} parsed={len(d['vectors'])} reverts={reverts}")
PY

shasum -a 256 "$DEST_DIR/fullmath-solidity-vectors.json" "$DEST_DIR/safecast-solidity-vectors.json"
