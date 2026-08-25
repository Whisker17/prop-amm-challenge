// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

import {Test} from "forge-std/Test.sol";

/// @notice Fails the suite if any vendored source has drifted from its pin.
///
/// The hashes are sha256 of the file bytes, computed by the EVM's own `sha256` precompile over
/// `vm.readFileBinary`, so they can be checked against `shasum -a 256` by hand. If someone
/// edits a vendored upstream file -- deliberately or by accident -- every gas number in
/// research-out/gas becomes a measurement of something else, and this test says so.
contract VendorPinsTest is Test {
    function _assertSha256(string memory path, bytes32 expected) internal view {
        bytes32 actual = sha256(vm.readFileBinary(path));
        assertEq(actual, expected, path);
    }

    // ---- DODO PMM: byte-identical copies of mantle-propamm-contracts @ 07f6797
    //      src/vendor/dodo/*, itself a 0.8.x transcription of
    //      DODOEX/contractV2 @ 8da3ee1ec50966fca9a2c80d424040c45c0f785e
    function test_dodoVendorPins() public view {
        _assertSha256(
            "lib/vendor/dodo/DODOMath.sol",
            0x90f688a26a7c6ad63b7f84b1c04cd61609540e14269f26810ad2cd80004c448a
        );
        _assertSha256(
            "lib/vendor/dodo/DecimalMath.sol",
            0x27d9d19a79982c79bd9faa5ea2c2039be319b256acc898de282179d7bf256352
        );
        _assertSha256(
            "lib/vendor/dodo/PMMPricing.sol",
            0x093b9adab96a57230d240984860c23580a2407e6085a3e57948d820ffe50807e
        );
    }

    // ---- Flashbots priority-update-registry @ da53117870c7bec96d71caebe1b3f94370aba3d6
    function test_flashbotsVendorPins() public view {
        _assertSha256(
            "lib/vendor/flashbots/ExamplePropAmm.sol",
            0x5b22ac480c2e2145fc0dbe005361b447f7c8fbc1d56a72671b4cfe6ceb177ca1
        );
        _assertSha256(
            "lib/vendor/flashbots/PrioUpdateRegistry.sol",
            0xe8797fbd1d2330e918209e360ee9f98d09813df1b0fee620d657585de8ded234
        );
    }

    // ---- Uniswap v2-core @ v1.0.1 = 4dd59067c76dea4a0e8e4bfdda41877a6b16dedc
    function test_uniV2CoreVendorPins() public view {
        _assertSha256(
            "lib/vendor/univ2core/contracts/UniswapV2Pair.sol",
            0x43a5421b31415868367b62bfa161ca10bcee03778873faad905f5a3e2cce9cbd
        );
        _assertSha256(
            "lib/vendor/univ2core/contracts/UniswapV2Factory.sol",
            0xe0cef3e874a68cbcc5986451b1fec180ba5ff5699f27a256b7c10fafefe36b99
        );
    }

    // ---- Uniswap v2-periphery @ ed24991304291297c3b4a52818d02f46a17aa9a2
    function test_uniV2PeripheryVendorPins() public view {
        _assertSha256(
            "lib/vendor/univ2periphery/UniswapV2Library.sol",
            0x4f83e9334f833568fa47b36e9ceca435f6c2962760a0596b043c4e538d0fd9f2
        );
    }

    // ---- Uniswap v3-core @ v1.0.0 = e3589b192d0be27e100cd0daaf6c97204fdb1899
    function test_uniV3CoreVendorPins() public view {
        _assertSha256(
            "lib/vendor/univ3/libraries/FullMath.sol",
            0x54087aee268a6938a85a408d7b14481b5c2c956c21508d5583f1bf48ec6d69ba
        );
        _assertSha256(
            "lib/vendor/univ3/libraries/TickMath.sol",
            0x83cf64b2ca84001effd16e007b49bac5359143b6c3132bfe42907b2426a0c5f5
        );
        _assertSha256(
            "lib/vendor/univ3/libraries/SqrtPriceMath.sol",
            0xddd62e3a94346248677f30f1ab009ef015e71e4b8696dcca890eeabc9dc6c149
        );
        _assertSha256(
            "lib/vendor/univ3/libraries/SwapMath.sol",
            0xd6cb9a153be4ea9fb2377ef88641ef7979b5cee6933162f1b732d0289e26e1b6
        );
        _assertSha256(
            "lib/vendor/univ3/UniswapV3Pool.sol",
            0xd515775b7f3ffe921dd70aca86b8bad16280fa4c122425d82b4dbea4dc564a7a
        );
        _assertSha256(
            "lib/vendor/univ3/UniswapV3Factory.sol",
            0xa9b78256f8a0ea95d464c96995015103e9681d0e1a144ab4e947d3de8715a38e
        );
    }

    /// @notice The BENCHMARK-ONLY patched files. Pinned so that "the minimal patch" cannot
    ///         quietly grow. The diffs themselves are in patches/.
    function test_patchedFilePins() public view {
        _assertSha256(
            "src/patched/UniswapV2PairZeroFee.sol",
            0x0fe0a8182a413a856dc7aa1fbf6d43bb5861a7da1b4136f25786d87293434004
        );
        _assertSha256(
            "src/patched/UniswapV2LibraryZeroFee.sol",
            0xe8fce40def8c7e7ad41f976c817c67aa936e8a597f113c16d5ed1011a73d384d
        );
    }
}
