#!/usr/bin/env bash
# Generate Flashbots prop-AMM golden vectors by EXECUTING the pinned Solidity.
#
# The pinned source repository is treated as strictly read-only: it is copied
# into a temporary directory, the generator test is dropped into the copy, and
# `forge test` runs there. Nothing is written back to the source tree.
#
#   usage: ./generate.sh [path-to-priority-update-registry]
#
# Default source path: ../../../../../mantle-propamm-contracts/lib/priority-update-registry
# Pinned commit:       da53117870c7bec96d71caebe1b3f94370aba3d6

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC="${1:-$HERE/../../../../../mantle-propamm-contracts/lib/priority-update-registry}"
DEST="$HERE/../../tests/fixtures/flashbots/golden-vectors.json"
EXPECTED_COMMIT="da53117870c7bec96d71caebe1b3f94370aba3d6"
EXPECTED_SHA256="5b22ac480c2e2145fc0dbe005361b447f7c8fbc1d56a72671b4cfe6ceb177ca1"

if [[ ! -f "$SRC/src/ExamplePropAmm.sol" ]]; then
  echo "error: ExamplePropAmm.sol not found under $SRC" >&2
  exit 1
fi

actual_commit="$(git -C "$SRC" rev-parse HEAD 2>/dev/null || echo unknown)"
actual_sha256="$(shasum -a 256 "$SRC/src/ExamplePropAmm.sol" | awk '{print $1}')"

echo "source repo:   $SRC"
echo "source commit: $actual_commit (expected $EXPECTED_COMMIT)"
echo "source sha256: $actual_sha256 (expected $EXPECTED_SHA256)"

if [[ "$actual_sha256" != "$EXPECTED_SHA256" ]]; then
  echo "error: ExamplePropAmm.sol does not match the pinned sha256" >&2
  exit 1
fi
if [[ "$actual_commit" != "$EXPECTED_COMMIT" ]]; then
  echo "warning: repository HEAD differs from the pinned commit" >&2
fi

WORK="$(mktemp -d "${TMPDIR:-/tmp}/flashbots-golden.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
echo "work dir:      $WORK"

cp -R "$SRC/src" "$WORK/src"
cp -R "$SRC/lib" "$WORK/lib"
cp "$SRC/foundry.toml" "$WORK/foundry.toml"
mkdir -p "$WORK/test"
cp "$HERE/FlashbotsGoldenGen.t.sol" "$WORK/test/"

# The copy needs write access so the generator can emit its JSON.
cat >> "$WORK/foundry.toml" <<'TOML'
fs_permissions = [{ access = "read-write", path = "./" }]
TOML

(
  cd "$WORK"
  VECTOR_OUT="./flashbots-golden-vectors.json" \
    forge test --match-contract FlashbotsGoldenGenTest -vv
)

mkdir -p "$(dirname "$DEST")"
cp "$WORK/flashbots-golden-vectors.json" "$DEST"
python3 -c "import json,sys; d=json.load(open(sys.argv[1])); print('vectors:', d['count'], 'parsed:', len(d['vectors']))" "$DEST"
echo "wrote $DEST"
shasum -a 256 "$DEST"
