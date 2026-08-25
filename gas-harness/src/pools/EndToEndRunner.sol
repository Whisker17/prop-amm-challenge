// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

import {TestToken} from "../TestToken.sol";
import {DodoPool} from "./DodoPool.sol";

interface IUniV2PairMinimal {
    function initialize(address, address) external;
    function mint(address to) external returns (uint256 liquidity);
    function swap(uint256 amount0Out, uint256 amount1Out, address to, bytes calldata data) external;
    function getReserves() external view returns (uint112, uint112, uint32);
    function token0() external view returns (address);
}

interface IUniV3FactoryMinimal {
    function enableFeeAmount(uint24 fee, int24 tickSpacing) external;
    function createPool(address tokenA, address tokenB, uint24 fee) external returns (address pool);
    function owner() external view returns (address);
}

interface IUniV3PoolMinimal {
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
    function slot0()
        external
        view
        returns (uint160 sqrtPriceX96, int24 tick, uint16, uint16, uint16, uint8, bool);
    function liquidity() external view returns (uint128);
    function tickSpacing() external view returns (int24);
}

interface IExamplePropAmmMinimal {
    function createPair(address tokenX, address tokenY, uint256 c, uint8 xd, uint8 yd) external returns (bytes32);
    function deposit(bytes32 pairId, uint256 amountX, uint256 amountY) external;
    function swapXtoY(bytes32 pairId, uint256 amountXIn, uint256 minAmountYOut) external returns (uint256);
    function swapYtoX(bytes32 pairId, uint256 amountYIn, uint256 minAmountXOut) external returns (uint256);
}

/// @notice Counterparty for the token-only control. It exists so the control has the same
///         call SHAPE as a Uniswap V2 swap -- one token transfer out to a contract, one
///         external call into that contract, one token transfer back -- with the curve, the
///         reserve reads and the reserve writes removed.
contract TokenSink {
    function push(TestToken token, address to, uint256 amount) external {
        token.transfer(to, amount);
    }
}

/// @notice The layer C trader. One contract, one entry point per algorithm, so the call
///         depth from the measured call to the pool is 1 in every case.
///
/// The entry points CANNOT have a single shape: a Uniswap V2 swap is "transfer then call",
/// a Uniswap V3 swap is "call then pay in a callback", a DODO or ExamplePropAmm swap is
/// "approve then call". That difference is pool ARCHITECTURE, it is real, it is a confounder,
/// and it is listed as one in README.md. It is not something the harness can normalise away
/// without ceasing to measure the real contracts.
contract EndToEndRunner {
    TokenSink public immutable sink;

    constructor() {
        sink = new TokenSink();
    }

    // ------------------------------------------------------------- token control

    /// @notice Token-only control: NO curve, NO reserve arithmetic. One transfer out, one
    ///         external call, one transfer back. Reported ALONGSIDE raw end-to-end gas,
    ///         never silently subtracted from it.
    function controlTransferPair(TestToken tokenIn, TestToken tokenOut, uint256 amountIn, uint256 amountOut)
        external
    {
        tokenIn.transfer(address(sink), amountIn);
        sink.push(tokenOut, address(this), amountOut);
    }

    // ------------------------------------------------------------------ uniswap v2

    /// @dev Works for BOTH the canonical 30 bps pair and the benchmark-only zero-fee pair:
    ///      the patch does not change the ABI.
    function swapUniV2(IUniV2PairMinimal pair, TestToken tokenIn, uint256 amountIn, bool zeroForOne, uint256 amountOut)
        external
    {
        tokenIn.transfer(address(pair), amountIn);
        if (zeroForOne) {
            pair.swap(0, amountOut, address(this), "");
        } else {
            pair.swap(amountOut, 0, address(this), "");
        }
    }

    // ------------------------------------------------------------------ uniswap v3

    uint160 internal constant MIN_SQRT_RATIO_PLUS_ONE = 4295128740;
    uint160 internal constant MAX_SQRT_RATIO_MINUS_ONE =
        1461446703485210103287273052203988822378723970341;

    TestToken internal _v3Token0;
    TestToken internal _v3Token1;

    function setV3Tokens(TestToken token0_, TestToken token1_) external {
        _v3Token0 = token0_;
        _v3Token1 = token1_;
    }

    function mintUniV3(IUniV3PoolMinimal pool, int24 tickLower, int24 tickUpper, uint128 amount) external {
        pool.mint(address(this), tickLower, tickUpper, amount, "");
    }

    function swapUniV3(IUniV3PoolMinimal pool, bool zeroForOne, uint256 amountIn) external {
        pool.swap(
            address(this),
            zeroForOne,
            int256(amountIn),
            zeroForOne ? MIN_SQRT_RATIO_PLUS_ONE : MAX_SQRT_RATIO_MINUS_ONE,
            ""
        );
    }

    function uniswapV3MintCallback(uint256 amount0Owed, uint256 amount1Owed, bytes calldata) external {
        if (amount0Owed > 0) _v3Token0.transfer(msg.sender, amount0Owed);
        if (amount1Owed > 0) _v3Token1.transfer(msg.sender, amount1Owed);
    }

    function uniswapV3SwapCallback(int256 amount0Delta, int256 amount1Delta, bytes calldata) external {
        if (amount0Delta > 0) _v3Token0.transfer(msg.sender, uint256(amount0Delta));
        if (amount1Delta > 0) _v3Token1.transfer(msg.sender, uint256(amount1Delta));
    }

    // ----------------------------------------------------------------------- dodo

    function swapDodo(DodoPool pool, bool sellBase, uint256 amountIn, uint256 i, uint256 k, uint256 lpFeeRate)
        external
    {
        pool.swap(sellBase, amountIn, i, k, lpFeeRate, address(this));
    }

    // ------------------------------------------------------------------ flashbots

    function swapFlashbotsXtoY(IExamplePropAmmMinimal amm, bytes32 pairId, uint256 amountIn) external {
        amm.swapXtoY(pairId, amountIn, 0);
    }

    function swapFlashbotsYtoX(IExamplePropAmmMinimal amm, bytes32 pairId, uint256 amountIn) external {
        amm.swapYtoX(pairId, amountIn, 0);
    }

    // -------------------------------------------------------------------- helpers

    function approve(TestToken token, address spender, uint256 amount) external {
        token.approve(spender, amount);
    }

    function transfer(TestToken token, address to, uint256 amount) external {
        token.transfer(to, amount);
    }
}
