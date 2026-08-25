// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

import {LayerCFixtures} from "./LayerCFixtures.sol";
import {IExamplePropAmmMinimal} from "../src/pools/EndToEndRunner.sol";

/// @notice Layer C smoke: every real pool executes a real swap with real transfers.
contract LayerCTest is LayerCFixtures {
    function setUp() public {
        _setUpLayerC();
    }

    function test_v2CanonicalSwap() public {
        uint256 amountIn = X0 / 100;
        uint256 out = v2AmountOut(false, true, amountIn);
        bool xIs0 = address(tokenX) < address(tokenY);
        uint256 before = tokenY.balanceOf(address(runner));
        runner.swapUniV2(v2Canonical, tokenX, amountIn, xIs0, out);
        emit log_named_uint("v2 canonical raw end-to-end gas", vm.lastCallGas().gasTotalUsed);
        assertEq(tokenY.balanceOf(address(runner)) - before, out);
    }

    function test_v2ZeroFeeSwap() public {
        uint256 amountIn = X0 / 100;
        uint256 out = v2AmountOut(true, true, amountIn);
        bool xIs0 = address(tokenX) < address(tokenY);
        uint256 before = tokenY.balanceOf(address(runner));
        runner.swapUniV2(v2ZeroFee, tokenX, amountIn, xIs0, out);
        emit log_named_uint("v2 zero-fee raw end-to-end gas", vm.lastCallGas().gasTotalUsed);
        assertEq(tokenY.balanceOf(address(runner)) - before, out);
        assertGt(out, v2AmountOut(false, true, amountIn), "zero-fee must pay strictly more");
    }

    function test_v3Swap() public {
        (, int24 tickBefore,,,,,) = v3Pool.slot0();
        // sell quote for base -> price of token1 in token0 rises when X is token0
        runner.swapUniV3(v3Pool, !xIsToken0, Y0 / 100);
        uint256 g = vm.lastCallGas().gasTotalUsed;
        (, int24 tickAfter,,,,,) = v3Pool.slot0();
        emit log_named_uint("v3 raw end-to-end gas", g);
        emit log_named_int("v3 tick before", tickBefore);
        emit log_named_int("v3 tick after", tickAfter);
        emit log_named_uint("v3 tick crossings", v3Crossings(tickBefore, tickAfter));
        assertTrue(tickAfter != tickBefore, "price did not move");
    }

    function test_dodoSwap() public {
        uint256 before = tokenY.balanceOf(address(runner));
        runner.swapDodo(dodoPool, true, X0 / 100, PRICE_WAD, DODO_K, 0);
        emit log_named_uint("dodo raw end-to-end gas", vm.lastCallGas().gasTotalUsed);
        assertGt(tokenY.balanceOf(address(runner)), before);
    }

    function test_flashbotsSwap() public {
        uint256 before = tokenY.balanceOf(address(runner));
        runner.swapFlashbotsXtoY(IExamplePropAmmMinimal(address(fbAmm)), fbPairId, X0 / 100);
        emit log_named_uint("flashbots raw end-to-end gas", vm.lastCallGas().gasTotalUsed);
        assertGt(tokenY.balanceOf(address(runner)), before);
    }

    function test_tokenControl() public {
        runner.controlTransferPair(tokenX, tokenY, X0 / 100, Y0 / 100);
        emit log_named_uint("token-only control gas", vm.lastCallGas().gasTotalUsed);
    }
}
