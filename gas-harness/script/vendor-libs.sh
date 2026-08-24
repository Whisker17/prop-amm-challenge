#!/usr/bin/env bash
# Populate gas-harness/lib/ with the TEST-HARNESS dependencies: forge-std,
# solady and openzeppelin-contracts.
#
# These are not the measured code. The pinned curve sources are a separate job,
# done by fetch-vendor.sh, which verifies them against vendor-pins.sha256; these
# three are only what the tests are written against.
#
# lib/ is .gitignore'd. The read-only reference repositories are NEVER modified:
# this script only copies out of them.
#
# **Idempotent.** Each destination is removed before it is written, because
# `cp -R a b` copies *into* `b` when `b` already exists — a second run would
# otherwise produce lib/forge-std/src/src and a build that fails in a way that
# looks like a stale artifact cache.
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

# Only these three trees, never lib/ as a whole: lib/vendor/ belongs to
# fetch-vendor.sh and the two scripts must not fight over it.
rm -rf "$HARNESS/lib/forge-std" \
       "$HARNESS/lib/solady" \
       "$HARNESS/lib/openzeppelin-contracts"
mkdir -p "$HARNESS/lib/forge-std" "$HARNESS/lib/solady/src" "$HARNESS/lib/openzeppelin-contracts"

cp -R "$REF/lib/forge-std/src"                                  "$HARNESS/lib/forge-std/src"
cp -R "$REF/lib/priority-update-registry/lib/solady/src/utils"   "$HARNESS/lib/solady/src/utils"
cp -R "$REF/lib/openzeppelin-contracts/contracts"                "$HARNESS/lib/openzeppelin-contracts/contracts"

# A nested copy means the guard above failed and the build would break oddly.
for nested in lib/forge-std/src/src lib/solady/src/utils/utils \
              lib/openzeppelin-contracts/contracts/contracts; do
  if [[ -d "$HARNESS/$nested" ]]; then
    echo "error: $nested exists — the copy nested, so this run was not idempotent" >&2
    exit 1
  fi
done

echo "ok: lib/ test-harness dependencies populated"
