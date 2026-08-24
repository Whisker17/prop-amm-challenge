#!/usr/bin/env bash
# Regenerate everything under research-out/gas/ from scratch.
#
#   research-out/gas/gas-snapshot.csv       one row per measured operation
#   research-out/gas/gas-snapshot.json      the same rows, plus run-level metadata
#   research-out/gas/touched-slots.csv      vm.record()/vm.accesses() output for layer B
#   research-out/gas/bytecode-hashes.csv    deployed runtime code hash of every layer C contract
#
# The generators are forge TESTS, not forge scripts, because they need `vm.coolSlot`,
# `vm.snapshotState`/`vm.revertToState`, `vm.record`/`vm.accesses` and `vm.lastCallGas`, and
# because a failing assertion should fail the run rather than write a wrong number.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

echo "== forge version =="
forge --version

echo
echo "== toolchain libraries (forge-std, solady, openzeppelin) =="
# Distinct from the pins below: these are test-harness dependencies, not the
# measured code. They live in the same ignored lib/ directory.
./script/vendor-libs.sh

echo
echo "== fetch and verify the pinned upstream sources =="
# No third-party source lives in this repository. lib/ is .gitignore'd and is
# rebuilt from the pins here; a sha256 mismatch aborts before anything is built.
./script/fetch-vendor.sh

echo
echo "== regenerate the patch audit record from the pins =="
./script/regenerate-patches.sh

echo
echo "== build (four compilers, one project) =="
forge build

echo
echo "== provenance and structure guards =="
# these MUST pass before any number is written
forge test --match-path 'test/VendorPins.t.sol' -vv
forge test --match-path 'test/NullTwin.t.sol'   -vv

echo
echo "== layer C bytecode hashes =="
forge test --match-path 'test/Bytecode.t.sol' -vv

echo
echo "== the sweep =="
forge test --match-path 'test/GasSnapshot.t.sol' -vv

echo
echo "== stamp the benchmark commit into the JSON =="
# The Solidity generator cannot ask git anything, so the provenance is injected
# here, from the repository that produced the numbers.
BENCH_COMMIT="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
if [[ -n "$(git status --porcelain 2>/dev/null)" ]]; then BENCH_DIRTY=true; else BENCH_DIRTY=false; fi
export BENCH_COMMIT BENCH_DIRTY
python3 - <<'PYSTAMP'
import json, os
path = "../research-out/gas/gas-snapshot.json"
with open(path) as handle:
    document = json.load(handle)
document["benchmarkCommit"] = os.environ["BENCH_COMMIT"]
document["benchmarkWorkingTreeDirty"] = os.environ["BENCH_DIRTY"] == "true"
document["vendorPinsManifest"] = "gas-harness/vendor-pins.sha256"
with open(path, "w") as handle:
    json.dump(document, handle, indent=1)
    handle.write("\n")
print(f"  benchmarkCommit={document['benchmarkCommit']} dirty={document['benchmarkWorkingTreeDirty']}")
PYSTAMP

echo
echo "== chinese report =="
python3 ./script/render-gas-report.py

echo
echo "== manifest =="
# Explicitly excluding MANIFEST.sha256 rather than relying on redirect ordering.
(
  cd ../research-out/gas
  find . -maxdepth 1 -type f ! -name MANIFEST.sha256 -print0 \
    | sort -z \
    | xargs -0 shasum -a 256 > MANIFEST.sha256
  cat MANIFEST.sha256
)

echo
echo "== output =="
wc -l ../research-out/gas/gas-snapshot.csv \
      ../research-out/gas/gas-snapshot.json \
      ../research-out/gas/touched-slots.csv \
      ../research-out/gas/bytecode-hashes.csv
