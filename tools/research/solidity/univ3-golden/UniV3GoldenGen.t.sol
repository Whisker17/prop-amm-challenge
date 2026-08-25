// SPDX-License-Identifier: MIT
pragma solidity =0.8.30;

// Golden-vector generator for Uniswap V3.
//
// Executed against a pristine copy of
//   Uniswap/v3-core @ e3589b192d0be27e100cd0daaf6c97204fdb1899 (tag v1.0.0)
// by `generate.sh`, which verifies the commit and the sha256 of every pinned
// file before it runs and never writes into the source checkout.
//
// Every expected value below is produced by executing that Solidity:
//   * per-function vectors come from `TickMath` / `SqrtPriceMath` / `SwapMath` /
//     `LiquidityMath`, reached through the =0.7.6 wrapper contracts that
//     `generate.sh` drops into the copy (a 0.8.x file may not import a 0.7.6
//     source, so the wrappers are reached with `deployCode` plus the locally
//     declared interfaces below);
//   * full-swap vectors come from a real `UniswapV3Pool`, created by a real
//     `UniswapV3Factory` with `enableFeeAmount(0, 1)`, initialised at exactly
//     price 100 and loaded with real minted positions.
//
// Nothing here is derived from the Rust port; the Rust port is tested against
// this output.

import {Test} from "forge-std/Test.sol";
import {console} from "forge-std/console.sol";

// --------------------------------------------------------------------------
// Locally declared interfaces for the =0.7.6 artifacts (never imported).
// --------------------------------------------------------------------------

interface ITickMathW {
    function getSqrtRatioAtTick(int24 tick) external pure returns (uint160);

    function getTickAtSqrtRatio(uint160 sqrtPriceX96) external pure returns (int24);

    function constants() external pure returns (int24, int24, uint160, uint160);
}

interface ISqrtPriceMathW {
    function getNextSqrtPriceFromInput(uint160, uint128, uint256, bool) external pure returns (uint160);

    function getNextSqrtPriceFromOutput(uint160, uint128, uint256, bool) external pure returns (uint160);

    function getAmount0DeltaRounded(uint160, uint160, uint128, bool) external pure returns (uint256);

    function getAmount1DeltaRounded(uint160, uint160, uint128, bool) external pure returns (uint256);

    function getAmount0DeltaSigned(uint160, uint160, int128) external pure returns (int256);

    function getAmount1DeltaSigned(uint160, uint160, int128) external pure returns (int256);
}

interface ISwapMathW {
    function computeSwapStep(uint160, uint160, uint128, int256, uint24)
        external
        pure
        returns (uint160, uint256, uint256, uint256);
}

interface ILiquidityMathW {
    function addDelta(uint128, int128) external pure returns (uint128);
}

interface IV3Factory {
    function enableFeeAmount(uint24 fee, int24 tickSpacing) external;

    function createPool(address tokenA, address tokenB, uint24 fee) external returns (address pool);
}

interface IV3Pool {
    function initialize(uint160 sqrtPriceX96) external;

    function mint(address recipient, int24 tickLower, int24 tickUpper, uint128 amount, bytes calldata data)
        external
        returns (uint256 amount0, uint256 amount1);

    function swap(
        address recipient,
        bool zeroForOne,
        int256 amountSpecified,
        uint160 sqrtPriceLimitX96,
        bytes calldata data
    ) external returns (int256 amount0, int256 amount1);

    function slot0() external view returns (uint160, int24, uint16, uint16, uint16, uint8, bool);

    function liquidity() external view returns (uint128);

    function ticks(int24) external view returns (uint128, int128, uint256, uint256, int56, uint160, uint32, bool);

    function token0() external view returns (address);

    function token1() external view returns (address);
}

/// Minimal 18-decimal token. The pool only calls `balanceOf` and `transfer`.
contract GenToken {
    uint8 public constant decimals = 18;
    string public name;
    string public symbol;
    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    constructor(string memory name_, string memory symbol_) {
        name = name_;
        symbol = symbol_;
    }

    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
        totalSupply += amount;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        return true;
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        balanceOf[msg.sender] -= amount;
        balanceOf[to] += amount;
        return true;
    }

    function transferFrom(address from, address to, uint256 amount) external returns (bool) {
        allowance[from][msg.sender] -= amount;
        balanceOf[from] -= amount;
        balanceOf[to] += amount;
        return true;
    }
}

contract UniV3GoldenGenTest is Test {
    // TickMath constants, re-read from the deployed wrapper in setUp and asserted.
    int24 private constant MIN_TICK = -887272;
    int24 private constant MAX_TICK = 887272;
    uint160 private constant MIN_SQRT_RATIO = 4295128739;
    uint160 private constant MAX_SQRT_RATIO = 1461446703485210103287273052203988822378723970342;

    /// sqrt(100) * 2^96 == 10 * 2^96, i.e. exactly price 100 of token1 per token0.
    uint160 private constant SQRT_PRICE_100 = 792281625142643375935439503360;
    int24 private constant TICK_100 = 46054;

    uint256 private constant TOKEN_SUPPLY = 1e33;

    ITickMathW private tickMath;
    ISqrtPriceMathW private sqrtPriceMath;
    ISwapMathW private swapMath;
    ILiquidityMathW private liquidityMath;

    // JSON writer state.
    string private outPath;
    bool private rowFirst;
    uint256 private arrayRows;
    uint256 private totalRows;
    string private countsBody;
    bool private countsFirst;

    function setUp() public {
        vm.warp(1_700_000_000);
        tickMath = ITickMathW(deployCode("TickMathWrapper.sol:TickMathWrapper"));
        sqrtPriceMath = ISqrtPriceMathW(deployCode("SqrtPriceMathWrapper.sol:SqrtPriceMathWrapper"));
        swapMath = ISwapMathW(deployCode("SwapMathWrapper.sol:SwapMathWrapper"));
        liquidityMath = ILiquidityMathW(deployCode("LiquidityMathWrapper.sol:LiquidityMathWrapper"));

        (int24 minTick, int24 maxTick, uint160 minRatio, uint160 maxRatio) = tickMath.constants();
        assertEq(minTick, MIN_TICK, "MIN_TICK");
        assertEq(maxTick, MAX_TICK, "MAX_TICK");
        assertEq(uint256(minRatio), uint256(MIN_SQRT_RATIO), "MIN_SQRT_RATIO");
        assertEq(uint256(maxRatio), uint256(MAX_SQRT_RATIO), "MAX_SQRT_RATIO");
        // getSqrtRatioAtTick(46054), i.e. the tick-aligned price just below the
        // exact sqrt price for 100. Cross-checked twice against an independent
        // transcription of the pinned TickMath before being pinned here.
        assertEq(
            uint256(tickMath.getSqrtRatioAtTick(TICK_100)),
            792281450588003167884250659085,
            "tick 46054 ratio"
        );
        // The exact price for 100 sits strictly between tick 46054 and 46055,
        // which is why the pool is initialised at SQRT_PRICE_100 rather than at
        // a tick-aligned value.
        assertLt(uint256(tickMath.getSqrtRatioAtTick(TICK_100)), uint256(SQRT_PRICE_100), "tick below price");
        assertGt(uint256(tickMath.getSqrtRatioAtTick(TICK_100 + 1)), uint256(SQRT_PRICE_100), "next tick above price");
        assertEq(tickMath.getTickAtSqrtRatio(SQRT_PRICE_100), TICK_100, "price 100 tick");
    }

    // ======================================================================
    // JSON plumbing
    // ======================================================================

    function _open(string memory path, string memory note) private {
        outPath = path;
        rowFirst = true;
        arrayRows = 0;
        totalRows = 0;
        countsBody = "";
        countsFirst = true;
        vm.writeFile(path, "{\n");
        vm.writeLine(path, "  \"source\": {");
        vm.writeLine(path, "    \"repository\": \"https://github.com/Uniswap/v3-core\",");
        vm.writeLine(path, "    \"tag\": \"v1.0.0\",");
        vm.writeLine(path, "    \"commit\": \"e3589b192d0be27e100cd0daaf6c97204fdb1899\",");
        vm.writeLine(path, "    \"solc\": \"0.7.6\",");
        vm.writeLine(path, "    \"generator\": \"tools/research/solidity/univ3-golden/UniV3GoldenGen.t.sol\",");
        vm.writeLine(path, string.concat("    \"note\": \"", note, "\""));
        vm.writeLine(path, "  },");
    }

    function _close() private {
        vm.writeLine(outPath, string.concat("  \"counts\": {", countsBody, ", \"total\": ", vm.toString(totalRows), "}"));
        vm.writeLine(outPath, "}");
    }

    function _beginArray(string memory key) private {
        vm.writeLine(outPath, string.concat("  \"", key, "\": ["));
        rowFirst = true;
        arrayRows = 0;
    }

    function _endArray(string memory key) private {
        vm.writeLine(outPath, "  ],");
        countsBody = string.concat(
            countsBody, countsFirst ? "" : ", ", "\"", key, "\": ", vm.toString(arrayRows)
        );
        countsFirst = false;
    }

    function _row(string memory body) private {
        vm.writeLine(outPath, string.concat(rowFirst ? "    {" : "   ,{", body, "}"));
        rowFirst = false;
        arrayRows += 1;
        totalRows += 1;
    }

    // Big values (anything that can exceed 2^53) are emitted as decimal strings.
    function _u(string memory k, uint256 v) internal pure returns (string memory) {
        return string.concat("\"", k, "\": \"", vm.toString(v), "\"");
    }

    function _i(string memory k, int256 v) internal pure returns (string memory) {
        return string.concat("\"", k, "\": \"", vm.toString(v), "\"");
    }

    // Small values (ticks, fee pips, counters) stay JSON numbers.
    function _n(string memory k, int256 v) internal pure returns (string memory) {
        return string.concat("\"", k, "\": ", vm.toString(v));
    }

    function _b(string memory k, bool v) internal pure returns (string memory) {
        return string.concat("\"", k, "\": ", v ? "true" : "false");
    }

    function _s(string memory k, string memory v) internal pure returns (string memory) {
        return string.concat("\"", k, "\": \"", v, "\"");
    }

    function _j(string memory a, string memory b) internal pure returns (string memory) {
        return string.concat(a, ", ", b);
    }

    // ======================================================================
    // (1) per-function vectors
    // ======================================================================

    function test_generatePerFunctionVectors() public {
        _open(
            vm.envOr("PER_FUNCTION_OUT", string("./univ3-per-function-vectors.json")),
            "every value returned by the pinned v3-core libraries; feePips=3000 lives in its own section"
        );

        _emitGetSqrtRatioAtTick();
        _emitGetTickAtSqrtRatio();
        _emitNextSqrtPrice();
        _emitAmountDeltas();
        _emitComputeSwapStep("computeSwapStepZeroFee", 0);
        _emitComputeSwapStep("computeSwapStepFee3000", 3000);
        _emitAddDelta();

        _close();
        console.log("per-function rows:", totalRows);
        assertGt(totalRows, 2000, "expected a broad per-function vector set");
    }

    function _specialTicks() private pure returns (int24[] memory t) {
        t = new int24[](33);
        t[0] = MIN_TICK;
        t[1] = MIN_TICK + 1;
        t[2] = MIN_TICK + 2;
        t[3] = MAX_TICK;
        t[4] = MAX_TICK - 1;
        t[5] = MAX_TICK - 2;
        t[6] = 0;
        t[7] = 1;
        t[8] = -1;
        t[9] = 2;
        t[10] = -2;
        t[11] = TICK_100;
        t[12] = TICK_100 - 1;
        t[13] = TICK_100 + 1;
        t[14] = -TICK_100;
        t[15] = -TICK_100 - 1;
        t[16] = -TICK_100 + 1;
        // one tick per bit of the |tick| decomposition inside getSqrtRatioAtTick,
        // plus the neighbours of each power of two.
        t[17] = 1;
        t[18] = 3;
        t[19] = 7;
        t[20] = 15;
        t[21] = 31;
        t[22] = 63;
        t[23] = 127;
        t[24] = 255;
        t[25] = 511;
        t[26] = 1023;
        t[27] = 4095;
        t[28] = 16383;
        t[29] = 65535;
        t[30] = 262143;
        t[31] = 524288;
        t[32] = 524287;
    }

    function _emitGetSqrtRatioAtTick() private {
        _beginArray("getSqrtRatioAtTick");

        int24[] memory special = _specialTicks();
        for (uint256 i; i < special.length; ++i) {
            _rowSqrtRatioAtTick(special[i]);
            if (special[i] > 0) {
                _rowSqrtRatioAtTick(-special[i]);
            }
        }
        // ±2^k for every representable k, which walks every branch of the
        // bit decomposition in getSqrtRatioAtTick.
        for (uint256 k; k < 20; ++k) {
            int24 tick = int24(int256(1) << k);
            _rowSqrtRatioAtTick(tick);
            _rowSqrtRatioAtTick(-tick);
        }
        // Dense sweep of the whole legal range (prime-ish stride, so the sample
        // is not aligned to any power of two).
        for (int256 t = int256(MIN_TICK); t <= int256(MAX_TICK); t += 2003) {
            _rowSqrtRatioAtTick(int24(t));
        }

        _endArray("getSqrtRatioAtTick");

        _beginArray("getSqrtRatioAtTickReverts");
        _rowSqrtRatioAtTickRevert(MIN_TICK - 1);
        _rowSqrtRatioAtTickRevert(MAX_TICK + 1);
        _rowSqrtRatioAtTickRevert(MIN_TICK - 100);
        _rowSqrtRatioAtTickRevert(MAX_TICK + 100);
        _rowSqrtRatioAtTickRevert(type(int24).min);
        _rowSqrtRatioAtTickRevert(type(int24).max);
        _endArray("getSqrtRatioAtTickReverts");
    }

    function _rowSqrtRatioAtTick(int24 tick) private {
        _row(_j(_n("tick", int256(tick)), _u("sqrtPriceX96", uint256(tickMath.getSqrtRatioAtTick(tick)))));
    }

    function _rowSqrtRatioAtTickRevert(int24 tick) private {
        bool reverted;
        try tickMath.getSqrtRatioAtTick(tick) returns (uint160) {
            reverted = false;
        } catch {
            reverted = true;
        }
        require(reverted, "expected getSqrtRatioAtTick to revert");
        _row(_j(_n("tick", int256(tick)), _b("reverted", true)));
    }

    function _emitGetTickAtSqrtRatio() private {
        _beginArray("getTickAtSqrtRatio");

        // The two ends of the legal domain: min is inclusive, max is exclusive.
        _rowTickAtSqrtRatio(MIN_SQRT_RATIO);
        _rowTickAtSqrtRatio(MIN_SQRT_RATIO + 1);
        _rowTickAtSqrtRatio(MIN_SQRT_RATIO + 2);
        _rowTickAtSqrtRatio(MAX_SQRT_RATIO - 1);
        _rowTickAtSqrtRatio(MAX_SQRT_RATIO - 2);
        _rowTickAtSqrtRatio(MAX_SQRT_RATIO - 3);
        // Exactly price 100, and the Q96 identity point.
        _rowTickAtSqrtRatio(SQRT_PRICE_100);
        _rowTickAtSqrtRatio(79228162514264337593543950336);

        // Values straddling tick boundaries: r-1, r and r+1 for a spread of ticks.
        int24[] memory special = _specialTicks();
        for (uint256 i; i < special.length; ++i) {
            _straddle(special[i]);
            if (special[i] > 0) {
                _straddle(-special[i]);
            }
        }
        for (int256 t = int256(MIN_TICK) + 1; t < int256(MAX_TICK); t += 4001) {
            _straddle(int24(t));
        }

        _endArray("getTickAtSqrtRatio");

        _beginArray("getTickAtSqrtRatioReverts");
        _rowTickAtSqrtRatioRevert(MAX_SQRT_RATIO); // exclusive upper bound
        _rowTickAtSqrtRatioRevert(MAX_SQRT_RATIO + 1);
        _rowTickAtSqrtRatioRevert(type(uint160).max);
        _rowTickAtSqrtRatioRevert(MIN_SQRT_RATIO - 1);
        _rowTickAtSqrtRatioRevert(0);
        _rowTickAtSqrtRatioRevert(1);
        _endArray("getTickAtSqrtRatioReverts");
    }

    function _straddle(int24 tick) private {
        uint160 ratio = tickMath.getSqrtRatioAtTick(tick);
        if (ratio > MIN_SQRT_RATIO) {
            _rowTickAtSqrtRatio(ratio - 1);
        }
        // The domain of getTickAtSqrtRatio is [MIN_SQRT_RATIO, MAX_SQRT_RATIO):
        // the upper end is EXCLUSIVE, so getSqrtRatioAtTick(MAX_TICK) is itself
        // not a legal input and must be skipped here rather than reverting the
        // whole generator. It is covered in the reverts array instead.
        if (ratio < MAX_SQRT_RATIO) {
            _rowTickAtSqrtRatio(ratio);
        }
        if (ratio + 1 < MAX_SQRT_RATIO) {
            _rowTickAtSqrtRatio(ratio + 1);
        }
    }

    function _rowTickAtSqrtRatio(uint160 ratio) private {
        _row(_j(_u("sqrtPriceX96", uint256(ratio)), _n("tick", int256(tickMath.getTickAtSqrtRatio(ratio)))));
    }

    function _rowTickAtSqrtRatioRevert(uint160 ratio) private {
        bool reverted;
        try tickMath.getTickAtSqrtRatio(ratio) returns (int24) {
            reverted = false;
        } catch {
            reverted = true;
        }
        require(reverted, "expected getTickAtSqrtRatio to revert");
        _row(_j(_u("sqrtPriceX96", uint256(ratio)), _b("reverted", true)));
    }

    function _priceGrid() private view returns (uint160[] memory p) {
        p = new uint160[](6);
        p[0] = MIN_SQRT_RATIO;
        p[1] = tickMath.getSqrtRatioAtTick(-TICK_100);
        p[2] = 79228162514264337593543950336; // tick 0
        p[3] = SQRT_PRICE_100;
        p[4] = tickMath.getSqrtRatioAtTick(400000);
        p[5] = MAX_SQRT_RATIO - 1;
    }

    function _liquidityGrid() private pure returns (uint128[] memory l) {
        l = new uint128[](5);
        l[0] = 1;
        l[1] = 1e6;
        l[2] = 1e18;
        l[3] = 1e21;
        l[4] = 1e27;
    }

    function _amountGrid() private pure returns (uint256[] memory a) {
        a = new uint256[](7);
        a[0] = 0;
        a[1] = 1; // 1 wei
        a[2] = 1e6;
        a[3] = 1e18;
        a[4] = 1e21;
        a[5] = 1e24;
        a[6] = 1 << 128; // huge
    }

    function _emitNextSqrtPrice() private {
        uint160[] memory prices = _priceGrid();
        uint128[] memory liq = _liquidityGrid();
        uint256[] memory amounts = _amountGrid();

        _beginArray("getNextSqrtPriceFromInput");
        for (uint256 i; i < prices.length; ++i) {
            for (uint256 j; j < liq.length; ++j) {
                for (uint256 k; k < amounts.length; ++k) {
                    _rowNextFromInput(prices[i], liq[j], amounts[k], true);
                    _rowNextFromInput(prices[i], liq[j], amounts[k], false);
                }
            }
        }
        // The documented `require` branches.
        _rowNextFromInput(0, 1e18, 1e18, true);
        _rowNextFromInput(SQRT_PRICE_100, 0, 1e18, true);
        _endArray("getNextSqrtPriceFromInput");

        _beginArray("getNextSqrtPriceFromOutput");
        for (uint256 i; i < prices.length; ++i) {
            for (uint256 j; j < liq.length; ++j) {
                for (uint256 k; k < amounts.length; ++k) {
                    _rowNextFromOutput(prices[i], liq[j], amounts[k], true);
                    _rowNextFromOutput(prices[i], liq[j], amounts[k], false);
                }
            }
        }
        _rowNextFromOutput(0, 1e18, 1e18, true);
        _rowNextFromOutput(SQRT_PRICE_100, 0, 1e18, true);
        _endArray("getNextSqrtPriceFromOutput");
    }

    function _rowNextFromInput(uint160 p, uint128 l, uint256 amount, bool zeroForOne) private {
        string memory head = _j(
            _j(_u("sqrtPX96", uint256(p)), _u("liquidity", uint256(l))),
            _j(_u("amountIn", amount), _b("zeroForOne", zeroForOne))
        );
        try sqrtPriceMath.getNextSqrtPriceFromInput(p, l, amount, zeroForOne) returns (uint160 q) {
            _row(_j(head, _u("sqrtQX96", uint256(q))));
        } catch {
            _row(_j(head, _b("reverted", true)));
        }
    }

    function _rowNextFromOutput(uint160 p, uint128 l, uint256 amount, bool zeroForOne) private {
        string memory head = _j(
            _j(_u("sqrtPX96", uint256(p)), _u("liquidity", uint256(l))),
            _j(_u("amountOut", amount), _b("zeroForOne", zeroForOne))
        );
        try sqrtPriceMath.getNextSqrtPriceFromOutput(p, l, amount, zeroForOne) returns (uint160 q) {
            _row(_j(head, _u("sqrtQX96", uint256(q))));
        } catch {
            _row(_j(head, _b("reverted", true)));
        }
    }

    function _pricePairs() private view returns (uint160[] memory a, uint160[] memory b) {
        a = new uint160[](9);
        b = new uint160[](9);
        uint160 p100 = SQRT_PRICE_100;
        uint160 tick0 = 79228162514264337593543950336;
        a[0] = p100;
        b[0] = tickMath.getSqrtRatioAtTick(TICK_100 + 1);
        a[1] = tickMath.getSqrtRatioAtTick(TICK_100 + 1);
        b[1] = p100; // reversed order of the same pair
        a[2] = p100;
        b[2] = tickMath.getSqrtRatioAtTick(TICK_100 + 1000);
        a[3] = tickMath.getSqrtRatioAtTick(TICK_100 - 1000);
        b[3] = p100;
        a[4] = tick0;
        b[4] = p100;
        a[5] = MIN_SQRT_RATIO;
        b[5] = MAX_SQRT_RATIO - 1;
        a[6] = p100;
        b[6] = p100; // zero-width
        a[7] = MIN_SQRT_RATIO;
        b[7] = MIN_SQRT_RATIO + 1;
        a[8] = MAX_SQRT_RATIO - 2;
        b[8] = MAX_SQRT_RATIO - 1;
    }

    function _emitAmountDeltas() private {
        (uint160[] memory pa, uint160[] memory pb) = _pricePairs();
        uint128[] memory liq = _liquidityGrid();

        _beginArray("getAmount0Delta");
        for (uint256 i; i < pa.length; ++i) {
            for (uint256 j; j < liq.length; ++j) {
                _rowAmountDelta(true, pa[i], pb[i], liq[j], true);
                _rowAmountDelta(true, pa[i], pb[i], liq[j], false);
            }
        }
        _rowAmountDelta(true, 0, SQRT_PRICE_100, 1e18, true); // require(sqrtRatioAX96 > 0)
        _endArray("getAmount0Delta");

        _beginArray("getAmount1Delta");
        for (uint256 i; i < pa.length; ++i) {
            for (uint256 j; j < liq.length; ++j) {
                _rowAmountDelta(false, pa[i], pb[i], liq[j], true);
                _rowAmountDelta(false, pa[i], pb[i], liq[j], false);
            }
        }
        _endArray("getAmount1Delta");

        int128[] memory signed = new int128[](6);
        signed[0] = 1;
        signed[1] = -1;
        signed[2] = 1e18;
        signed[3] = -1e18;
        signed[4] = 1e21;
        signed[5] = -1e21;

        _beginArray("getAmount0DeltaSigned");
        for (uint256 i; i < pa.length; ++i) {
            for (uint256 j; j < signed.length; ++j) {
                _rowAmountDeltaSigned(true, pa[i], pb[i], signed[j]);
            }
        }
        _endArray("getAmount0DeltaSigned");

        _beginArray("getAmount1DeltaSigned");
        for (uint256 i; i < pa.length; ++i) {
            for (uint256 j; j < signed.length; ++j) {
                _rowAmountDeltaSigned(false, pa[i], pb[i], signed[j]);
            }
        }
        _endArray("getAmount1DeltaSigned");
    }

    function _rowAmountDelta(bool zero, uint160 a, uint160 b, uint128 l, bool roundUp) private {
        string memory head = _j(
            _j(_u("sqrtRatioAX96", uint256(a)), _u("sqrtRatioBX96", uint256(b))),
            _j(_u("liquidity", uint256(l)), _b("roundUp", roundUp))
        );
        if (zero) {
            try sqrtPriceMath.getAmount0DeltaRounded(a, b, l, roundUp) returns (uint256 amount) {
                _row(_j(head, _u("amount", amount)));
            } catch {
                _row(_j(head, _b("reverted", true)));
            }
        } else {
            try sqrtPriceMath.getAmount1DeltaRounded(a, b, l, roundUp) returns (uint256 amount) {
                _row(_j(head, _u("amount", amount)));
            } catch {
                _row(_j(head, _b("reverted", true)));
            }
        }
    }

    function _rowAmountDeltaSigned(bool zero, uint160 a, uint160 b, int128 l) private {
        string memory head = _j(
            _j(_u("sqrtRatioAX96", uint256(a)), _u("sqrtRatioBX96", uint256(b))), _i("liquidity", int256(l))
        );
        if (zero) {
            try sqrtPriceMath.getAmount0DeltaSigned(a, b, l) returns (int256 amount) {
                _row(_j(head, _i("amount", amount)));
            } catch {
                _row(_j(head, _b("reverted", true)));
            }
        } else {
            try sqrtPriceMath.getAmount1DeltaSigned(a, b, l) returns (int256 amount) {
                _row(_j(head, _i("amount", amount)));
            } catch {
                _row(_j(head, _b("reverted", true)));
            }
        }
    }

    function _emitComputeSwapStep(string memory key, uint24 feePips) private {
        _beginArray(key);

        int24[] memory offsets = new int24[](8);
        offsets[0] = -1000;
        offsets[1] = -100;
        offsets[2] = -10;
        offsets[3] = -1;
        offsets[4] = 1;
        offsets[5] = 10;
        offsets[6] = 100;
        offsets[7] = 1000;

        uint128[] memory liq = new uint128[](3);
        liq[0] = 1e12;
        liq[1] = 1e21;
        liq[2] = 1e27;

        int256[] memory remaining = new int256[](10);
        remaining[0] = 1;
        remaining[1] = 1e12;
        remaining[2] = 1e18;
        remaining[3] = 1e21;
        remaining[4] = 1e24;
        remaining[5] = -1;
        remaining[6] = -1e12;
        remaining[7] = -1e18;
        remaining[8] = -1e21;
        remaining[9] = -1e24;

        int24[] memory centres = new int24[](2);
        centres[0] = TICK_100;
        centres[1] = 0;

        for (uint256 c; c < centres.length; ++c) {
            uint160 current = tickMath.getSqrtRatioAtTick(centres[c]);
            for (uint256 i; i < offsets.length; ++i) {
                uint160 target = tickMath.getSqrtRatioAtTick(centres[c] + offsets[i]);
                for (uint256 j; j < liq.length; ++j) {
                    for (uint256 k; k < remaining.length; ++k) {
                        _rowComputeSwapStep(current, target, liq[j], remaining[k], feePips);
                    }
                }
            }
        }

        _endArray(key);
    }

    function _rowComputeSwapStep(uint160 current, uint160 target, uint128 l, int256 remaining, uint24 feePips)
        private
    {
        string memory head = _j(
            _j(_u("sqrtRatioCurrentX96", uint256(current)), _u("sqrtRatioTargetX96", uint256(target))),
            _j(_u("liquidity", uint256(l)), _i("amountRemaining", remaining))
        );
        head = _j(head, _n("feePips", int256(uint256(feePips))));
        head = _j(head, _b("zeroForOne", current >= target));
        head = _j(head, _b("exactIn", remaining >= 0));

        try swapMath.computeSwapStep(current, target, l, remaining, feePips) returns (
            uint160 next, uint256 amountIn, uint256 amountOut, uint256 feeAmount
        ) {
            string memory tail = _j(
                _j(_u("sqrtRatioNextX96", uint256(next)), _u("amountIn", amountIn)),
                _j(_u("amountOut", amountOut), _u("feeAmount", feeAmount))
            );
            _row(_j(_j(head, tail), _b("reachedTarget", next == target)));
        } catch {
            _row(_j(head, _b("reverted", true)));
        }
    }

    function _emitAddDelta() private {
        _beginArray("addDelta");
        _rowAddDelta(0, 0);
        _rowAddDelta(0, 1);
        _rowAddDelta(1, -1);
        _rowAddDelta(1, 1);
        _rowAddDelta(1e21, 1e21);
        _rowAddDelta(1e21, -1e21);
        _rowAddDelta(1e21, -1);
        _rowAddDelta(type(uint128).max, -1);
        _rowAddDelta(type(uint128).max, 0);
        _rowAddDelta(uint128(uint256(type(uint128).max) >> 1), type(int128).max);
        _endArray("addDelta");

        _beginArray("addDeltaReverts");
        _rowAddDeltaRevert(0, -1); // 'LS'
        _rowAddDeltaRevert(5, -6); // 'LS'
        _rowAddDeltaRevert(type(uint128).max, 1); // 'LA'
        _rowAddDeltaRevert(type(uint128).max, type(int128).max); // 'LA'
        _rowAddDeltaRevert(1, type(int128).min); // uint128(-y) wraps to 2^127 in 0.7.6
        _endArray("addDeltaReverts");
    }

    function _rowAddDelta(uint128 x, int128 y) private {
        _row(
            _j(
                _j(_u("x", uint256(x)), _i("y", int256(y))),
                _u("z", uint256(liquidityMath.addDelta(x, y)))
            )
        );
    }

    function _rowAddDeltaRevert(uint128 x, int128 y) private {
        bool reverted;
        try liquidityMath.addDelta(x, y) returns (uint128) {
            reverted = false;
        } catch {
            reverted = true;
        }
        require(reverted, "expected addDelta to revert");
        _row(_j(_j(_u("x", uint256(x)), _i("y", int256(y))), _b("reverted", true)));
    }

    // ======================================================================
    // (2) full-swap vectors
    // ======================================================================

    struct Pool {
        IV3Pool pool;
        GenToken token0;
        GenToken token1;
        int24[] candidates;
        uint256 balance0;
        uint256 balance1;
        int24 outerLower;
        int24 outerUpper;
    }

    struct SwapCase {
        string label;
        bool zeroForOne;
        int256 amountSpecified;
        uint160 limit;
    }

    struct SwapObservation {
        uint160 preSqrt;
        int24 preTick;
        uint128 preLiquidity;
        bool reverted;
        int256 amount0;
        int256 amount1;
        uint160 postSqrt;
        int24 postTick;
        uint128 postLiquidity;
        uint256 ticksCrossed;
    }

    uint256 private swapRows;
    uint256 private sequenceRows;

    function test_generateFullSwapVectors() public {
        _open(
            vm.envOr("GOLDEN_OUT", string("./univ3-golden-vectors.json")),
            "real zero-fee UniswapV3Pool created by a real UniswapV3Factory with enableFeeAmount(0, 1)"
        );

        // (1) Full range: the benchmark's own opening position.
        _beginArray("fullRange");
        _runScenario("full-range", _fullRangeTicks(), _fullRangeLiquidity());
        _endArray("fullRange");

        // (2) Concentrated ladders. Each is a separate pool so the scenarios
        //     cannot contaminate one another. Widths are chosen so that the
        //     trade grid below crosses zero, one and several initialized ticks.
        _beginArray("concentratedNarrow");
        _runScenario("concentrated-60", _ladderTicks(60, 1), uint128(1e21));
        _endArray("concentratedNarrow");

        _beginArray("concentratedWide");
        _runScenario("concentrated-600", _ladderTicks(600, 1), uint128(1e21));
        _endArray("concentratedWide");

        _beginArray("concentratedLadder");
        // Three nested positions => six initialized ticks => a single large
        // swap crosses several of them.
        _runScenario("ladder-3", _ladderTicks(200, 3), uint128(4e20));
        _endArray("concentratedLadder");

        // (3) A long consecutive sequence against one pool, recording the full
        //     state after every single swap. This is what proves a port tracks
        //     state rather than merely quoting.
        _beginArray("sequence");
        _runSequence(220);
        _endArray("sequence");

        _close();
        assertGt(totalRows, 300, "expected a broad full-swap vector set");
        console.log("full-swap vectors written:", totalRows);
    }

    // ----------------------------------------------------------------------
    // full-swap helpers
    // ----------------------------------------------------------------------

    function _fullRangeLiquidity() private pure returns (uint128) {
        // L = sqrt(100e18 * 10000e18) = 1e21, the liquidity that makes a
        // full-range position hold the benchmark's opening capital.
        return uint128(1e21);
    }

    function _fullRangeTicks() private pure returns (int24[] memory ticks) {
        ticks = new int24[](2);
        ticks[0] = MIN_TICK;
        ticks[1] = MAX_TICK;
    }

    /// `count` nested symmetric positions, the innermost `halfWidth` ticks wide
    /// and each next one twice as wide.
    function _ladderTicks(int24 halfWidth, uint256 count) private pure returns (int24[] memory ticks) {
        ticks = new int24[](count * 2);
        int24 width = halfWidth;
        for (uint256 i; i < count; ++i) {
            ticks[i * 2] = TICK_100 - width;
            ticks[i * 2 + 1] = TICK_100 + width;
            width *= 2;
        }
    }

    /// Mint every `(lower, upper)` pair in `ticks` with the same liquidity and
    /// return the resulting pool.
    function _buildPool(int24[] memory ticks, uint128 liquidity) private returns (Pool memory p) {
        p = _newPool();
        for (uint256 i; i < ticks.length; i += 2) {
            (uint256 a0, uint256 a1) = p.pool.mint(address(this), ticks[i], ticks[i + 1], liquidity, "");
            p.balance0 += a0;
            p.balance1 += a1;
        }
        p.outerLower = ticks[ticks.length - 2];
        p.outerUpper = ticks[ticks.length - 1];
    }

    /// Count how many initialized ticks a swap crossed, by comparing the pool's
    /// tick before and after. Only ticks the pool actually holds are counted.
    function _countCrossings(int24[] memory ticks, int24 before, int24 nowTick)
        private
        pure
        returns (uint256 crossed)
    {
        (int24 lo, int24 hi) = before <= nowTick ? (before, nowTick) : (nowTick, before);
        for (uint256 i; i < ticks.length; ++i) {
            if (ticks[i] > lo && ticks[i] <= hi) {
                crossed += 1;
            }
        }
    }

    /// A price limit 500 ticks beyond the outermost position.
    ///
    /// Upstream would happily walk the price across the entire tick domain when
    /// given `MIN_SQRT_RATIO + 1`, one 256-tick word per loop iteration, which
    /// for the largest trade sizes here costs more gas than a Foundry test has.
    /// Bounding the limit is what a router does anyway, it is recorded in every
    /// vector, and it still lets the swap cross the position's boundary and
    /// leave the range — only the empty walk beyond it is cut short.
    function _boundedLimit(int24[] memory ticks, bool zeroForOne) private view returns (uint160) {
        int24 outer = zeroForOne ? ticks[ticks.length - 2] : ticks[ticks.length - 1];
        int24 target = zeroForOne ? outer - 500 : outer + 500;
        if (target < MIN_TICK) target = MIN_TICK;
        if (target > MAX_TICK) target = MAX_TICK;
        uint160 ratio = tickMath.getSqrtRatioAtTick(target);
        if (zeroForOne && ratio <= MIN_SQRT_RATIO) return MIN_SQRT_RATIO + 1;
        if (!zeroForOne && ratio >= MAX_SQRT_RATIO) return MAX_SQRT_RATIO - 1;
        return ratio;
    }

    /// The trade grid: 0.01%, 0.1%, 1%, 5% and 10% of the pool's balance of the
    /// input token, plus one wei, plus a size large enough to exhaust a narrow
    /// range.
    function _tradeSizes(uint256 balance) private pure returns (uint256[] memory sizes) {
        sizes = new uint256[](7);
        sizes[0] = 1;
        sizes[1] = balance / 10000;
        sizes[2] = balance / 1000;
        sizes[3] = balance / 100;
        sizes[4] = balance / 20;
        sizes[5] = balance / 10;
        sizes[6] = balance; // enough to exhaust a concentrated range
        for (uint256 i; i < sizes.length; ++i) {
            if (sizes[i] == 0) sizes[i] = 1;
        }
    }

    /// Row payload for a swap. Kept as one memory struct so the emitters stay
    /// well under solc 0.7/0.8's stack limit.
    struct SwapRow {
        string label;
        bool zeroForOne;
        bool exactIn;
        bool reverted;
        int256 amountSpecified;
        uint160 limit;
        uint160 sqrtBefore;
        uint160 sqrtAfter;
        int24 tickBefore;
        int24 tickAfter;
        uint128 liquidityBefore;
        uint128 liquidityAfter;
        int256 amount0;
        int256 amount1;
        uint256 crossings;
        int256 step;
        bool hasStep;
    }

    function _emitSwapRow(SwapRow memory r) private {
        string memory body = r.hasStep ? _n("step", r.step) : _s("scenario", r.label);
        body = string.concat(body, ", ", _s("kind", "swap"));
        body = string.concat(body, ", ", _b("zeroForOne", r.zeroForOne));
        body = string.concat(body, ", ", _b("exactIn", r.exactIn));
        body = string.concat(body, ", ", _i("amountSpecified", r.amountSpecified));
        body = string.concat(body, ", ", _u("sqrtPriceLimitX96", uint256(r.limit)));
        body = string.concat(body, ", ", _u("sqrtPriceBeforeX96", uint256(r.sqrtBefore)));
        body = string.concat(body, ", ", _n("tickBefore", r.tickBefore));
        body = string.concat(body, ", ", _u("liquidityBefore", uint256(r.liquidityBefore)));
        body = string.concat(body, ", ", _b("reverted", r.reverted));
        if (!r.reverted) {
            body = string.concat(body, ", ", _i("amount0", r.amount0));
            body = string.concat(body, ", ", _i("amount1", r.amount1));
            body = string.concat(body, ", ", _u("sqrtPriceAfterX96", uint256(r.sqrtAfter)));
            body = string.concat(body, ", ", _n("tickAfter", r.tickAfter));
            body = string.concat(body, ", ", _u("liquidityAfter", uint256(r.liquidityAfter)));
            body = string.concat(body, ", ", _n("crossings", int256(r.crossings)));
        }
        _row(body);
    }

    /// Run one scenario: for every direction, every size, and both exact-input
    /// and exact-output, execute the swap on a fresh pool and record inputs,
    /// pre-state, outputs and post-state.
    /// Deploying a factory, a pool and two tokens costs several million gas, so
    /// the pool is built ONCE per scenario and every swap runs against a state
    /// snapshot of it. Each row is therefore still independent of the last,
    /// without paying for a redeployment per row.
    function _runScenario(string memory label, int24[] memory ticks, uint128 liquidity) private {
        Pool memory p = _buildPool(ticks, liquidity);
        _recordMint(label, ticks, p);

        for (uint256 d; d < 2; ++d) {
            uint256[] memory sizes = _tradeSizes(d == 0 ? p.balance0 : p.balance1);
            for (uint256 s; s < sizes.length; ++s) {
                _recordSwap(label, ticks, p, d == 0, sizes[s], true);
                _recordSwap(label, ticks, p, d == 0, sizes[s], false);
            }
        }
    }

    function _recordMint(string memory label, int24[] memory ticks, Pool memory p) private {
        (uint160 sqrtNow, int24 tickNow,,,,,) = p.pool.slot0();
        string memory body = _s("scenario", label);
        body = string.concat(body, ", ", _s("kind", "mint"));
        body = string.concat(body, ", ", _u("amount0", p.balance0));
        body = string.concat(body, ", ", _u("amount1", p.balance1));
        body = string.concat(body, ", ", _u("liquidity", uint256(p.pool.liquidity())));
        body = string.concat(body, ", ", _u("sqrtPriceX96", uint256(sqrtNow)));
        body = string.concat(body, ", ", _n("tick", tickNow));
        body = string.concat(body, ", ", _n("positions", int256(ticks.length / 2)));
        _row(body);
    }

    /// Execute one swap on a fresh pool so every row is independent of the last.
    function _recordSwap(
        string memory label,
        int24[] memory ticks,
        Pool memory p,
        bool zeroForOne,
        uint256 size,
        bool exactIn
    ) private {
        uint256 snap = vm.snapshotState();
        SwapRow memory r;
        r.label = label;
        r.zeroForOne = zeroForOne;
        r.exactIn = exactIn;
        r.amountSpecified = exactIn ? int256(size) : -int256(size);
        r.limit = _boundedLimit(ticks, zeroForOne);
        (r.sqrtBefore, r.tickBefore,,,,,) = p.pool.slot0();
        r.liquidityBefore = p.pool.liquidity();

        try p.pool.swap(address(this), zeroForOne, r.amountSpecified, r.limit, "") returns (
            int256 amount0, int256 amount1
        ) {
            r.amount0 = amount0;
            r.amount1 = amount1;
            (r.sqrtAfter, r.tickAfter,,,,,) = p.pool.slot0();
            r.liquidityAfter = p.pool.liquidity();
            r.crossings = _countCrossings(ticks, r.tickBefore, r.tickAfter);
        } catch {
            r.reverted = true;
        }
        vm.revertToState(snap);
        _emitSwapRow(r);
    }

    /// A long consecutive sequence against ONE pool, with the full state
    /// recorded after every swap. This is what proves a port tracks state
    /// rather than merely quoting a fresh pool each time.
    function _runSequence(uint256 swaps) private {
        int24[] memory ticks = _ladderTicks(400, 2);
        Pool memory p = _buildPool(ticks, uint128(5e20));
        _recordMint("sequence", ticks, p);
        uint256 seed = 0xA11CE;

        for (uint256 i; i < swaps; ++i) {
            seed = uint256(keccak256(abi.encode(seed, i)));
            SwapRow memory r;
            r.hasStep = true;
            r.step = int256(i);
            r.zeroForOne = seed % 2 == 0;
            r.exactIn = (seed >> 1) % 4 != 0; // mostly exact input
            r.limit = _boundedLimit(ticks, r.zeroForOne);
            (r.sqrtBefore, r.tickBefore,,,,,) = p.pool.slot0();
            r.liquidityBefore = p.pool.liquidity();
            r.amountSpecified = _sequenceAmount(p, seed, r.zeroForOne, r.exactIn);

            try p.pool.swap(address(this), r.zeroForOne, r.amountSpecified, r.limit, "") returns (
                int256 amount0, int256 amount1
            ) {
                r.amount0 = amount0;
                r.amount1 = amount1;
                (r.sqrtAfter, r.tickAfter,,,,,) = p.pool.slot0();
                r.liquidityAfter = p.pool.liquidity();
                r.crossings = _countCrossings(ticks, r.tickBefore, r.tickAfter);
            } catch {
                r.reverted = true;
            }
            _emitSwapRow(r);
        }
    }

    function _sequenceAmount(Pool memory p, uint256 seed, bool zeroForOne, bool exactIn)
        private
        pure
        returns (int256)
    {
        // Sizes from dust to ~0.5% of the pool, so the price wanders without
        // immediately exhausting the range.
        uint256 balance = zeroForOne ? p.balance0 : p.balance1;
        uint256 size = 1 + ((seed >> 8) % (balance / 200 + 1));
        return exactIn ? int256(size) : -int256(size);
    }

    // ----------------------------------------------------------------------

    function _tickArray(int24[] memory src) private pure returns (int24[] memory out) {
        out = src;
    }

    // Deploy tokens, factory and a zero-fee pool at exactly price 100.
    function _newPool() private returns (Pool memory p) {
        GenToken a = new GenToken("Token A", "TKA");
        GenToken b = new GenToken("Token B", "TKB");
        (GenToken t0, GenToken t1) = address(a) < address(b) ? (a, b) : (b, a);

        t0.mint(address(this), TOKEN_SUPPLY);
        t1.mint(address(this), TOKEN_SUPPLY);

        IV3Factory factory = IV3Factory(deployCode("UniswapV3Factory.sol:UniswapV3Factory"));
        factory.enableFeeAmount(0, 1);
        address pool = factory.createPool(address(t0), address(t1), 0);
        IV3Pool(pool).initialize(SQRT_PRICE_100);

        p.pool = IV3Pool(pool);
        p.token0 = t0;
        p.token1 = t1;
        require(p.pool.token0() == address(t0), "token0 ordering");
    }

    function uniswapV3MintCallback(uint256 amount0Owed, uint256 amount1Owed, bytes calldata) external {
        if (amount0Owed > 0) GenToken(IV3Pool(msg.sender).token0()).transfer(msg.sender, amount0Owed);
        if (amount1Owed > 0) GenToken(IV3Pool(msg.sender).token1()).transfer(msg.sender, amount1Owed);
    }

    function uniswapV3SwapCallback(int256 amount0Delta, int256 amount1Delta, bytes calldata) external {
        if (amount0Delta > 0) GenToken(IV3Pool(msg.sender).token0()).transfer(msg.sender, uint256(amount0Delta));
        if (amount1Delta > 0) GenToken(IV3Pool(msg.sender).token1()).transfer(msg.sender, uint256(amount1Delta));
    }
}
