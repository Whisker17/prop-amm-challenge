#!/usr/bin/env bash
# Populate gas-harness/lib/ from the pinned READ-ONLY checkouts.
#
# lib/ is .gitignore'd: it holds third-party dependencies (forge-std, solady,
# openzeppelin-contracts) that are not this harness's source of truth. Run this once after
# cloning, before `forge build`.
#
# The read-only repositories are NEVER modified: this script only copies OUT of them.
set -euo pipefail

HARNESS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REF="${MANTLE_PROPAMM_CONTRACTS:-$HARNESS/../../mantle-propamm-contracts}"

if [[ ! -d "$REF/lib/priority-update-registry" ]]; then
  echo "error: cannot find mantle-propamm-contracts at $REF" >&2
  echo "       set MANTLE_PROPAMM_CONTRACTS to its path" >&2
  exit 1
fi

echo "reference checkout: $REF"
git -C "$REF" rev-parse --short HEAD                              || true
git -C "$REF/lib/priority-update-registry" rev-parse --short HEAD || true

mkdir -p "$HARNESS/lib/forge-std" "$HARNESS/lib/solady/src" "$HARNESS/lib/openzeppelin-contracts"

cp -R "$REF/lib/forge-std/src"                                  "$HARNESS/lib/forge-std/src"
cp -R "$REF/lib/priority-update-registry/lib/solady/src/utils"   "$HARNESS/lib/solady/src/utils"
cp -R "$REF/lib/openzeppelin-contracts/contracts"                "$HARNESS/lib/openzeppelin-contracts/contracts"

echo "ok: lib/ populated"
