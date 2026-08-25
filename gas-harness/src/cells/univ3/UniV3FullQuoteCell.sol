// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity =0.7.6;

import "vendor/univ3/libraries/SwapMath.sol";
import "vendor/univ3/libraries/TickMath.sol";
import "vendor/univ3/libraries/TickBitmap.sol";
import "vendor/univ3/libraries/LiquidityMath.sol";
import "vendor/univ3/libraries/LowGasSafeMath.sol";
import "vendor/univ3/libraries/SafeCast.sol";
import "vendor/univ3/libraries/Tick.sol";
import "vendor/univ3/libraries/Oracle.sol";
import "./UniV3CellStorage.sol";

/// @notice Uniswap V3 algorithm cell -- metric A2, the FULL QUOTE.
///
/// This is the ONLY V3 number comparable with the DODO / Flashbots / Uniswap V2 whole-quote
/// numbers. The single-step primitive (metric A1) lives in {UniV3StepCell} and is NOT
/// comparable with them: it is one iteration of the loop below.
///
/// HARNESS-AUTHORED, and deliberately so. `_swapLoop` is a line-by-line transcription of
///   Uniswap/v3-core @ v1.0.0 (e3589b192d0be27e100cd0daaf6c97204fdb1899)
///   contracts/UniswapV3Pool.sol  sha256 d515775b7f3ffe921dd70aca86b8bad16280fa4c122425d82b4dbea4dc564a7a
///   function `swap`, the `while (state.amountSpecifiedRemaining != 0 && ...)` loop,
/// with these removals, each of which makes the number SMALLER and each of which is a
/// property of *settlement*, not of *quoting*:
///   * `feeGrowthGlobalX128` / protocol-fee accumulation
///   * `ticks.cross(...)` replaced by a direct `_ticks[tickNext].liquidityNet` read
///     (a cross ALSO rewrites the feeGrowthOutside fields; a quote cannot)
///   * the final slot0 / liquidity / balance writes and the token transfers
/// Every arithmetic call -- SwapMath.computeSwapStep, TickMath.getSqrtRatioAtTick,
/// TickMath.getTickAtSqrtRatio, TickBitmap.nextInitializedTickWithinOneWord,
/// LiquidityMath.addDelta, SafeCast, LowGasSafeMath -- is the pinned upstream library,
/// unmodified, compiled at the upstream hardhat configuration (0.7.6, optimizer, runs 800).
///
/// The end-to-end V3 swap on a REAL, unmodified `UniswapV3Pool` is measured in layer C.
contract UniV3FullQuoteCell is UniV3CellStorage {
    using LowGasSafeMath for uint256;
    using LowGasSafeMath for int256;
    using SafeCast for uint256;
    using SafeCast for int256;
    using TickBitmap for mapping(int16 => uint256);
    using Oracle for Oracle.Observation[65535];

    // UniswapV3Pool.SwapState, minus the fee-growth and protocol-fee fields.
    struct SwapState {
        int256 amountSpecifiedRemaining;
        int256 amountCalculated;
        uint160 sqrtPriceX96;
        int24 tick;
        uint128 liquidity;
    }

    // The pool fields the loop needs, gathered into ONE memory struct. Upstream reads them
    // from storage and immutables inside `swap`; passing them flat here overflows the 0.7.6
    // stack, and `via_ir` is off because upstream's own build has it off.
    struct Pool {
        uint160 sqrtPriceX96;
        uint128 liquidity;
        int24 tick;
        int24 tickSpacing;
        uint24 fee;
    }

    // UniswapV3Pool.StepComputations, verbatim.
    struct StepComputations {
        uint160 sqrtPriceStartX96;
        int24 tickNext;
        bool initialized;
        uint160 sqrtPriceNextX96;
        uint256 amountIn;
        uint256 amountOut;
        uint256 feeAmount;
    }

    constructor() {
        (_obsCardinality, _obsCardinalityNext) = Oracle.initialize(_observations, uint32(block.timestamp));
    }

    function quote(bool zeroForOne, uint256 amountIn, uint256[6] calldata state, uint256[2] calldata)
        external
        view
        returns (uint256 amountOut)
    {
        Pool memory p = Pool({
            sqrtPriceX96: uint160(state[0]),
            liquidity: uint128(state[1]),
            tick: int24(int256(state[2])),
            tickSpacing: int24(int256(state[3])),
            fee: uint24(state[4])
        });
        amountOut = _swapLoop(zeroForOne, int256(amountIn), p);
    }

    function seedState(uint256[6] calldata state) external {
        _s[0] = state[0];
        _s[1] = state[1];
        _s[2] = state[2];
        _s[3] = state[3];
        _s[4] = state[4];
        _s[5] = state[5];
    }

    /// @notice Initialises one tick: writes `liquidityNet` and flips the tick bitmap word.
    ///         Present (as a no-op) on every other cell and on every null twin so that the
    ///         selector table has the SAME SHAPE for every algorithm.
    function seedTick(int24 tick, int128 liquidityNet) external {
        _ticks[tick].liquidityGross = uint128(liquidityNet >= 0 ? liquidityNet : -liquidityNet);
        _ticks[tick].liquidityNet = liquidityNet;
        _ticks[tick].initialized = true;
        _tickBitmap.flipTick(tick, int24(int256(_s[3])));
    }

    function swapAlgorithmOracle(bool zeroForOne, uint256 amountIn, uint256[2] calldata)
        external
        returns (uint256 amountOut)
    {
        Pool memory p = Pool({
            sqrtPriceX96: uint160(_s[0]),
            liquidity: uint128(_s[1]),
            tick: int24(int256(_s[2])),
            tickSpacing: int24(int256(_s[3])),
            fee: uint24(_s[4])
        });
        amountOut = _swapLoop(zeroForOne, int256(amountIn), p);
        _s[0] = uint256(p.sqrtPriceX96);
        _s[1] = uint256(p.liquidity);
        _s[2] = uint256(int256(p.tick));
    }

    /// @notice V3's REAL oracle path: an observation written through the pinned
    ///         `Oracle` library, exactly as `UniswapV3Pool.swap` does before it moves the
    ///         price. Observation cardinality is 1 (the state every pool is created with,
    ///         and the state most pools stay in unless someone pays `increaseObservation-
    ///         CardinalityNext`), so `write` overwrites index 0.
    function swapReferenceOracle(bool zeroForOne, uint256 amountIn, bytes calldata)
        external
        returns (uint256 amountOut)
    {
        (uint16 indexUpdated, uint16 cardinalityUpdated) = Oracle.write(
            _observations,
            _obsIndex,
            uint32(block.timestamp),
            int24(int256(_s[2])),
            uint128(_s[1]),
            _obsCardinality,
            _obsCardinalityNext
        );
        _obsIndex = indexUpdated;
        _obsCardinality = cardinalityUpdated;

        Pool memory p = Pool({
            sqrtPriceX96: uint160(_s[0]),
            liquidity: uint128(_s[1]),
            tick: int24(int256(_s[2])),
            tickSpacing: int24(int256(_s[3])),
            fee: uint24(_s[4])
        });
        amountOut = _swapLoop(zeroForOne, int256(amountIn), p);
        _s[0] = uint256(p.sqrtPriceX96);
        _s[1] = uint256(p.liquidity);
        _s[2] = uint256(int256(p.tick));
    }

    function readState() external view returns (uint256[6] memory state) {
        state[0] = _s[0];
        state[1] = _s[1];
        state[2] = _s[2];
        state[3] = _s[3];
        state[4] = _s[4];
        state[5] = _s[5];
    }

    function cellKind() external pure returns (bytes32) {
        return "univ3-full-quote";
    }

    /// @dev UniswapV3Pool.swap's loop. `p` is updated IN PLACE with the post-trade
    ///      sqrtPriceX96 / tick / liquidity, so the caller can write them back.
    function _swapLoop(bool zeroForOne, int256 amountSpecified, Pool memory p)
        internal
        view
        returns (uint256 amountOut)
    {
        uint160 sqrtPriceLimitX96 =
            zeroForOne ? TickMath.MIN_SQRT_RATIO + 1 : TickMath.MAX_SQRT_RATIO - 1;

        SwapState memory state = SwapState({
            amountSpecifiedRemaining: amountSpecified,
            amountCalculated: 0,
            sqrtPriceX96: p.sqrtPriceX96,
            tick: p.tick,
            liquidity: p.liquidity
        });

        while (state.amountSpecifiedRemaining != 0 && state.sqrtPriceX96 != sqrtPriceLimitX96) {
            StepComputations memory step;
            step.sqrtPriceStartX96 = state.sqrtPriceX96;

            (step.tickNext, step.initialized) =
                _tickBitmap.nextInitializedTickWithinOneWord(state.tick, p.tickSpacing, zeroForOne);

            if (step.tickNext < TickMath.MIN_TICK) {
                step.tickNext = TickMath.MIN_TICK;
            } else if (step.tickNext > TickMath.MAX_TICK) {
                step.tickNext = TickMath.MAX_TICK;
            }

            step.sqrtPriceNextX96 = TickMath.getSqrtRatioAtTick(step.tickNext);

            (state.sqrtPriceX96, step.amountIn, step.amountOut, step.feeAmount) = SwapMath.computeSwapStep(
                state.sqrtPriceX96,
                (
                    zeroForOne
                        ? step.sqrtPriceNextX96 < sqrtPriceLimitX96
                        : step.sqrtPriceNextX96 > sqrtPriceLimitX96
                ) ? sqrtPriceLimitX96 : step.sqrtPriceNextX96,
                state.liquidity,
                state.amountSpecifiedRemaining,
                p.fee
            );

            // exactInput only: this harness never quotes an exact-output trade.
            state.amountSpecifiedRemaining -= (step.amountIn + step.feeAmount).toInt256();
            state.amountCalculated = state.amountCalculated.sub(step.amountOut.toInt256());

            if (state.sqrtPriceX96 == step.sqrtPriceNextX96) {
                if (step.initialized) {
                    // upstream calls ticks.cross(), which ALSO rewrites feeGrowthOutside.
                    // A quote cannot write, so only the liquidityNet field is read.
                    int128 liquidityNet = _ticks[step.tickNext].liquidityNet;
                    if (zeroForOne) liquidityNet = -liquidityNet;
                    state.liquidity = LiquidityMath.addDelta(state.liquidity, liquidityNet);
                }
                state.tick = zeroForOne ? step.tickNext - 1 : step.tickNext;
            } else if (state.sqrtPriceX96 != step.sqrtPriceStartX96) {
                state.tick = TickMath.getTickAtSqrtRatio(state.sqrtPriceX96);
            }
        }

        amountOut = uint256(-state.amountCalculated);
        p.sqrtPriceX96 = state.sqrtPriceX96;
        p.tick = state.tick;
        p.liquidity = state.liquidity;
    }
}
