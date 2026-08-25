// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

import {LayerCFixtures} from "./LayerCFixtures.sol";

/// @notice Records the DEPLOYED (runtime) bytecode hash of every contract whose exact code the
///         layer C numbers depend on, and writes them to research-out/gas/bytecode-hashes.csv.
///
/// Two of these matter especially:
///   * `TestToken` -- the ONE token every pool in layer C trades. Token transfer
///     implementations move end-to-end gas by hundreds of gas and have nothing to do with any
///     curve, so the exact code has to be on the record.
///   * `UniswapV2Pair` vs `UniswapV2PairZeroFee` -- the canonical and benchmark-only variants.
///     Their hashes differ by exactly the patch in patches/uniswap-v2-pair-zero-fee.patch.
contract BytecodeTest is LayerCFixtures {
    string internal constant OUT = "../research-out/gas/bytecode-hashes.csv";

    function setUp() public {
        _setUpLayerC();
    }

    function test_recordBytecodeHashes() public {
        vm.writeFile(OUT, "");
        vm.writeLine(OUT, "contract,address,runtimeCodeSize,runtimeCodeHash,note");

        _record("TestToken", address(tokenX), "the ONE token traded by every layer C pool");
        _record("UniswapV2Pair", address(v2Canonical), "canonical-v2-30bps, unmodified upstream v1.0.1");
        _record(
            "UniswapV2PairZeroFee",
            address(v2ZeroFee),
            "modified-v2-zero-fee; differs from the row above by patches/uniswap-v2-pair-zero-fee.patch only"
        );
        _record("UniswapV3Pool(fee=0)", address(v3Pool), "real unmodified v3-core pool, fee 0 tickSpacing 60");
        _record("UniswapV3Pool(fee=3000)", address(v3Pool3000), "real unmodified v3-core pool, canonical 0.30% tier");
        _record("UniswapV3Factory", address(v3Factory), "real unmodified v3-core factory");
        _record("ExamplePropAmm", address(fbAmm), "real unmodified flashbots prop AMM");
        _record("PrioUpdateRegistry", address(fbRegistry), "real unmodified flashbots registry");
        _record("DodoPool", address(dodoPool), "HARNESS-AUTHORED minimal pool around the pinned PMMPricing library");
        _record("EndToEndRunner", address(runner), "the layer C trader");
        _record("TokenSink", address(runner.sink()), "counterparty for the token-only control");

        // The two V2 pairs must NOT have the same runtime code: if they did, the zero-fee
        // patch never took effect and the two V2 rows would be the same measurement twice.
        assertTrue(
            address(v2Canonical).codehash != address(v2ZeroFee).codehash,
            "the zero-fee patch produced identical bytecode: it did not take effect"
        );
        // and they must be close in size: a minimal patch, not a rewrite
        uint256 a = address(v2Canonical).code.length;
        uint256 b = address(v2ZeroFee).code.length;
        assertLt(a > b ? a - b : b - a, 200, "the zero-fee 'minimal patch' changed too much code");
    }

    function _record(string memory name, address at, string memory note) internal {
        vm.writeLine(
            OUT,
            string.concat(
                name,
                ",",
                vm.toString(at),
                ",",
                vm.toString(at.code.length),
                ",",
                vm.toString(at.codehash),
                ',"',
                note,
                '"'
            )
        );
        emit log_named_string(name, vm.toString(at.codehash));
    }
}
