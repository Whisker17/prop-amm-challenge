// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

import {Fixtures} from "./Fixtures.sol";
import {IAdapter} from "../src/IAdapter.sol";
import {PrioUpdateRegistry} from "vendor/flashbots/PrioUpdateRegistry.sol";

/// @notice Smoke probe: every cell deploys, quotes a non-zero amount, and its null twin
///         answers on the same selectors. Run first; if this fails nothing downstream is
///         meaningful.
contract ProbeTest is Fixtures {
    function test_probeLayerA() public {
        (IAdapter dodo, IAdapter dodoNull) = deployDodo();
        PrioUpdateRegistry reg = new PrioUpdateRegistry(3600, 60);
        (IAdapter fb, IAdapter fbNull) = deployFlashbots(reg, 1, 60);
        (IAdapter v2, IAdapter v2Null) = deployUniV2(false);
        (IAdapter v2z,) = deployUniV2(true);

        uint256 amountIn = X0 / 100; // 1% of the base reserve

        uint256 qDodo = dodo.quote(true, amountIn, dodoState(false), dodoOracle());
        uint256 qFb = fb.quote(true, amountIn, flashbotsState(false), flashbotsOracle());
        uint256 qV2 = v2.quote(true, amountIn, univ2State(false), emptyOracle());
        uint256 qV2z = v2z.quote(true, amountIn, univ2State(false), emptyOracle());

        emit log_named_uint("dodo    quote 1% sell base", qDodo);
        emit log_named_uint("flash   quote 1% sell base", qFb);
        emit log_named_uint("univ2   quote 1% sell base (30bps)", qV2);
        emit log_named_uint("univ2   quote 1% sell base (zero fee)", qV2z);

        assertGt(qDodo, 0, "dodo quote zero");
        assertGt(qFb, 0, "flashbots quote zero");
        assertGt(qV2, 0, "univ2 quote zero");
        assertGt(qV2z, qV2, "zero-fee v2 must return strictly more than 30bps v2");

        assertEq(dodoNull.quote(true, amountIn, dodoState(false), dodoOracle()), 0);
        assertEq(fbNull.quote(true, amountIn, flashbotsState(false), flashbotsOracle()), 0);
        assertEq(v2Null.quote(true, amountIn, univ2State(false), emptyOracle()), 0);
    }

    function test_probeUniV3() public {
        (IAdapter v3, IAdapter v3Null) = deployUniV3FullQuote();
        uint256[6] memory s = univ3State(false, V3_FEE_ZERO);
        s[2] = uint256(int256(int24(46054)));
        v3.seedState(s);
        uint256 q = v3.quote(false, Y0 / 100, s, emptyOracle());
        emit log_named_uint("univ3   full quote 1% sell quote", q);
        assertGt(q, 0, "univ3 full quote zero");
        assertEq(v3Null.quote(false, Y0 / 100, s, emptyOracle()), 0);
    }

    function test_probeGasMechanism() public {
        (IAdapter dodo, IAdapter dodoNull) = deployDodo();
        uint256[6] memory s = dodoState(false);
        uint256[2] memory o = dodoOracle();

        dodo.quote(true, X0 / 100, s, o);
        uint256 raw = vm.lastCallGas().gasTotalUsed;
        dodoNull.quote(true, X0 / 100, s, o);
        uint256 nullTwin = vm.lastCallGas().gasTotalUsed;

        emit log_named_uint("dodo quote rawGas", raw);
        emit log_named_uint("dodo quote nullTwinGas", nullTwin);
        emit log_named_uint("dodo quote adjustedGas", raw - nullTwin);
        assertGt(raw, nullTwin, "raw must exceed null twin");

        // vm.lastCallGas must be EXACT: repeat the identical call, demand the identical number.
        dodo.quote(true, X0 / 100, s, o);
        assertEq(vm.lastCallGas().gasTotalUsed, raw, "lastCallGas is not deterministic");
    }
}
