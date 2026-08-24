// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

import {Fixtures} from "./Fixtures.sol";
import {IAdapter} from "../src/IAdapter.sol";
import {PrioUpdateRegistry} from "vendor/flashbots/PrioUpdateRegistry.sol";

/// @notice Guards the one property the whole `adjustedGas` column depends on.
///
/// Solidity dispatches external calls by BINARY SEARCH over the sorted selector table. A twin
/// with a different number of selectors searches a different tree and charges a different
/// dispatch tax, so `rawGas - nullTwinGas` stops meaning anything. An earlier experiment in
/// this workspace turned a true adjusted cost of 25 into 91 exactly that way.
///
/// These tests assert:
///   1. all seven uniform selectors resolve on every cell AND on every null twin
///   2. a selector that is NOT in the set reverts on both
///   3. the null twin's dispatch cost is identical for every cell built by the same compiler,
///      which is only possible if the tables have the same shape
contract NullTwinTest is Fixtures {
    function _pairs() internal returns (IAdapter[] memory reals, IAdapter[] memory twins) {
        PrioUpdateRegistry reg = new PrioUpdateRegistry(3600, 60);
        reals = new IAdapter[](5);
        twins = new IAdapter[](5);
        (reals[0], twins[0]) = deployDodo();
        (reals[1], twins[1]) = deployFlashbots(reg, 7, 300);
        (reals[2], twins[2]) = deployUniV2(false);
        (reals[3], twins[3]) = deployUniV2(true);
        (reals[4], twins[4]) = deployUniV3FullQuote();
    }

    /// @dev Correctly ABI-encoded payloads for all seven uniform selectors. A selector that is
    ///      absent from a contract falls through to the (non-existent) fallback and the call
    ///      reverts with EMPTY return data; a selector that IS present either succeeds or
    ///      reverts with a reason. That distinction is the test.
    function _uniformPayloads() internal pure returns (bytes[] memory data) {
        // A state and an amount that are benign for EVERY cell: `amountIn == 0` makes the
        // Uniswap V3 loop exit before its first iteration (a zero tickSpacing would otherwise
        // spin until out-of-gas, which also reverts with empty return data and would be
        // indistinguishable from a missing selector), and s[3] != 0 keeps DODO's V0 > 0.
        uint256[6] memory s = [uint256(1), 1, 0, 1, 0, 0];
        uint256[2] memory o = [uint256(1), 1];
        data = new bytes[](7);
        data[0] = abi.encodeCall(IAdapter.quote, (true, 0, s, o));
        data[1] = abi.encodeCall(IAdapter.seedState, (s));
        data[2] = abi.encodeCall(IAdapter.seedTick, (int24(0), int128(0)));
        data[3] = abi.encodeCall(IAdapter.swapAlgorithmOracle, (true, 0, o));
        // six words: a valid encoding of DodoCell.PricingStateCalldata, which is the only
        // cell that decodes this argument at all. Passing empty bytes instead would make the
        // ABI decoder revert with EMPTY return data and be indistinguishable from a missing
        // selector, which is what this test is trying to detect.
        data[4] = abi.encodeCall(
            IAdapter.swapReferenceOracle,
            (true, 0, abi.encode(uint256(1), uint256(1), uint256(1e20), uint256(1e17), uint256(0), bytes32(0)))
        );
        data[5] = abi.encodeCall(IAdapter.readState, ());
        data[6] = abi.encodeCall(IAdapter.cellKind, ());
    }

    function test_allUniformSelectorsResolveOnCellAndTwin() public {
        (IAdapter[] memory reals, IAdapter[] memory twins) = _pairs();
        bytes4[] memory sels = uniformSelectors();
        bytes[] memory payloads = _uniformPayloads();
        assertEq(sels.length, 7, "the uniform selector set is seven wide");
        assertEq(payloads.length, sels.length, "one payload per selector");
        for (uint256 s = 0; s < sels.length; s++) {
            assertEq(bytes4(payloads[s]), sels[s], "payload/selector mismatch");
        }

        for (uint256 k = 0; k < reals.length; k++) {
            for (uint256 s = 0; s < sels.length; s++) {
                _assertSelectorResolves(address(reals[k]), payloads[s], k, s, "cell");
                _assertSelectorResolves(address(twins[k]), payloads[s], k, s, "twin");
            }
        }
    }

    function _assertSelectorResolves(address target, bytes memory data, uint256 k, uint256 s, string memory side)
        internal
    {
        (bool ok, bytes memory ret) = target.call(data);
        if (!ok && ret.length == 0) {
            emit log_named_uint("pair index", k);
            emit log_named_uint("selector index", s);
            emit log_named_string("side", side);
            revert("selector does not resolve: selector table shape differs");
        }
    }

    function test_unknownSelectorRevertsOnCellAndTwin() public {
        (IAdapter[] memory reals, IAdapter[] memory twins) = _pairs();
        bytes memory data = abi.encodePacked(bytes4(0xdeadbeef), new bytes(10 * 32));
        for (uint256 k = 0; k < reals.length; k++) {
            (bool okReal,) = address(reals[k]).call(data);
            (bool okTwin,) = address(twins[k]).call(data);
            assertFalse(okReal, "cell accepted an unknown selector");
            assertFalse(okTwin, "twin accepted an unknown selector");
        }
    }

    /// @notice Two twins compiled by the SAME solc with the SAME selector table must cost the
    ///         same to dispatch. If they do not, the tables differ.
    function test_nullTwinDispatchIsIdenticalWithinACompiler() public {
        PrioUpdateRegistry reg = new PrioUpdateRegistry(3600, 60);
        (, IAdapter dodoNull) = deployDodo();
        (, IAdapter fbNull) = deployFlashbots(reg, 7, 300);
        (, IAdapter v2Null) = deployUniV2(false);
        (, IAdapter v3Null) = deployUniV3FullQuote();

        uint256 gDodo = _twinQuoteGas(dodoNull);
        uint256 gFb = _twinQuoteGas(fbNull);
        uint256 gV2 = _twinQuoteGas(v2Null);
        uint256 gV3 = _twinQuoteGas(v3Null);

        emit log_named_uint("nullTwin quote gas, 0.8.28 (dodo)", gDodo);
        emit log_named_uint("nullTwin quote gas, 0.8.28 (flashbots)", gFb);
        emit log_named_uint("nullTwin quote gas, 0.6.6  (univ2)", gV2);
        emit log_named_uint("nullTwin quote gas, 0.7.6  (univ3)", gV3);

        assertEq(gDodo, gFb, "0.8.28 twins must dispatch identically");
        // Across compilers the dispatch tax legitimately differs; that is a disclosed
        // confounder, not a bug, and it is exactly why adjustedGas is per-algorithm only.
        assertTrue(gV2 != gDodo || gV3 != gDodo, "cross-compiler dispatch tax should be visible");
    }

    function _twinQuoteGas(IAdapter twin) internal view returns (uint256) {
        uint256[6] memory s;
        uint256[2] memory o;
        twin.quote(true, 1, s, o); // warm
        twin.quote(true, 1, s, o);
        return vm.lastCallGas().gasTotalUsed;
    }

    function test_nullTwinIsStrictlyCheaperThanItsCell() public {
        (IAdapter[] memory reals, IAdapter[] memory twins) = _pairs();
        uint256[2] memory o = dodoOracle();
        uint256[6] memory s = dodoState(false);

        // only the DODO pair can be quoted with DODO state; the rest are checked with their own
        reals[0].quote(true, X0 / 100, s, o);
        uint256 raw = vm.lastCallGas().gasTotalUsed;
        twins[0].quote(true, X0 / 100, s, o);
        uint256 nul = vm.lastCallGas().gasTotalUsed;
        assertGt(raw, nul, "dodo: null twin must be cheaper than the cell");

        reals[1].quote(true, X0 / 100, flashbotsState(false), flashbotsOracle());
        raw = vm.lastCallGas().gasTotalUsed;
        twins[1].quote(true, X0 / 100, flashbotsState(false), flashbotsOracle());
        nul = vm.lastCallGas().gasTotalUsed;
        assertGt(raw, nul, "flashbots: null twin must be cheaper than the cell");

        reals[2].quote(true, X0 / 100, univ2State(false), emptyOracle());
        raw = vm.lastCallGas().gasTotalUsed;
        twins[2].quote(true, X0 / 100, univ2State(false), emptyOracle());
        nul = vm.lastCallGas().gasTotalUsed;
        assertGt(raw, nul, "univ2: null twin must be cheaper than the cell");
    }

    /// @notice `vm.lastCallGas().gasTotalUsed` must be EXACT, not merely close: the whole
    ///         method assumes zero variance across identical repeats.
    function test_gasMeasurementIsExactAcrossRepeats() public {
        (IAdapter dodo,) = deployDodo();
        uint256[6] memory s = dodoState(false);
        uint256[2] memory o = dodoOracle();
        dodo.quote(true, X0 / 100, s, o);
        uint256 first = vm.lastCallGas().gasTotalUsed;
        for (uint256 i = 0; i < 5; i++) {
            dodo.quote(true, X0 / 100, s, o);
            assertEq(vm.lastCallGas().gasTotalUsed, first, "gasTotalUsed is not deterministic");
        }
    }

    /// @notice vm.coolSlot must be applied to the contract that OWNS the slot. Cooling the
    ///         Adapter wrapper -- which holds no storage at all -- must change nothing.
    function test_coolSlotOnWrapperDoesNothing() public {
        (IAdapter dodo,) = deployDodo();
        (, bytes memory ret) = address(dodo).staticcall(abi.encodeWithSignature("cell()"));
        address cell = abi.decode(ret, (address));

        uint256 snap = vm.snapshotState();
        dodo.seedState(dodoState(false));
        dodo.swapAlgorithmOracle(true, X0 / 100, dodoOracle());
        uint256 warm;
        vm.revertToState(snap);

        dodo.seedState(dodoState(false));
        dodo.swapAlgorithmOracle(true, X0 / 100, dodoOracle());
        warm = vm.lastCallGas().gasTotalUsed;
        vm.revertToState(snap);

        dodo.seedState(dodoState(false));
        for (uint256 i = 0; i < 6; i++) {
            vm.coolSlot(address(dodo), bytes32(i)); // the WRAPPER: owns nothing
        }
        dodo.swapAlgorithmOracle(true, X0 / 100, dodoOracle());
        uint256 wrapperCooled = vm.lastCallGas().gasTotalUsed;
        vm.revertToState(snap);

        dodo.seedState(dodoState(false));
        for (uint256 i = 0; i < 6; i++) {
            vm.coolSlot(cell, bytes32(i)); // the CELL: owns slots 0..5
        }
        dodo.swapAlgorithmOracle(true, X0 / 100, dodoOracle());
        uint256 cellCooled = vm.lastCallGas().gasTotalUsed;
        vm.revertToState(snap);

        emit log_named_uint("warm", warm);
        emit log_named_uint("wrapper cooled", wrapperCooled);
        emit log_named_uint("cell cooled", cellCooled);

        assertEq(wrapperCooled, warm, "cooling the wrapper changed the number; it must not");
        assertEq(cellCooled - warm, 6 * 2000, "cooling the owner must cost exactly 2000 per slot");
    }
}
