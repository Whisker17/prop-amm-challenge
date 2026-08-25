// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

import {PrioUpdateRegistry} from "vendor/flashbots/PrioUpdateRegistry.sol";

/// @notice Flashbots `ExamplePropAmm` algorithm cell.
///
/// The quote maths are a LITERAL transcription of
///   flashbots/priority-update-registry @ da53117870c7bec96d71caebe1b3f94370aba3d6
///   src/ExamplePropAmm.sol, `_quoteXtoY` and `_quoteYtoX`
/// with the operation order, the integer division points and the operand order preserved
/// exactly. The only change is that the pair fields arrive from this cell's own `_s` array
/// (or from calldata, for layer A) instead of from a `TradingPair storage` struct, so that
/// every algorithm in this harness presents the SAME six-word state and the SAME calldata.
///
/// The real `ExamplePropAmm` packs `TradingPair` across a different number of slots and is
/// measured, unmodified and in full, in layer C.
///
/// `swapReferenceOracle` is the REAL Flashbots oracle path: an unmodified
/// `PrioUpdateRegistry.getState()` read plus the `_isTargetYLocked` writeback that
/// `ExamplePropAmm.swapXtoY` performs on every swap. That number is a SYSTEM cost.
contract FlashbotsCell {
    /// slot 0..5 -- IDENTICAL layout to FlashbotsCellNull.
    /// s[0]=reserveX s[1]=reserveY s[2]=targetX s[3]=concentration
    /// s[4]=targetYReference s[5]=targetYBasedLock
    uint256[6] internal _s;

    PrioUpdateRegistry internal immutable prioRegistry;
    uint256 internal immutable laneIndex;
    uint256 internal immutable maxParameterAge;

    constructor(PrioUpdateRegistry registry_, uint256 laneIndex_, uint256 maxParameterAge_) {
        prioRegistry = registry_;
        laneIndex = laneIndex_;
        maxParameterAge = maxParameterAge_;
        if (address(registry_) != address(0)) registry_.addUpdater(msg.sender);
    }

    function quote(bool zeroForOne, uint256 amountIn, uint256[6] calldata state, uint256[2] calldata oracle)
        external
        pure
        returns (uint256 amountOut)
    {
        return _quote(zeroForOne, amountIn, state[0], state[2], state[3], oracle[0], oracle[1]);
    }

    function seedState(uint256[6] calldata state) external {
        _s = state;
    }

    /// @notice No-op: this algorithm has no ticks. Present so that every cell in the harness
    ///         exposes the SAME seven selectors and therefore the same dispatch table shape.
    function seedTick(int24, int128) external {}

    function swapAlgorithmOracle(bool zeroForOne, uint256 amountIn, uint256[2] calldata oracle)
        external
        returns (uint256 amountOut)
    {
        return _swap(zeroForOne, amountIn, oracle[0], oracle[1]);
    }

    function swapReferenceOracle(bool zeroForOne, uint256 amountIn, bytes calldata)
        external
        returns (uint256 amountOut)
    {
        // ExamplePropAmm._readParametersFromRegistry, verbatim call shape.
        (, uint256[] memory slots) = prioRegistry.getState(
            laneIndex, uint32(block.timestamp - maxParameterAge), uint32(block.timestamp)
        );
        if (slots.length < 3) revert("ParametersNotSet");
        uint256 concentration = slots[0];
        uint256 multX = slots[1];
        uint256 multY = slots[2];

        // ExamplePropAmm._isTargetYLocked, verbatim, including its storage writeback.
        uint256 targetY = (_s[0] * multX + _s[1] * multY - _s[2] * multX) / multY;
        uint256 maxRef = targetY > _s[4] ? targetY : _s[4];
        _s[4] = maxRef;
        if (((_s[4] - targetY) * 10000) / _s[4] > 500) {
            _s[5] = 1;
        }
        if (_s[5] != 0) revert("PairLocked");

        _s[3] = concentration;
        return _swap(zeroForOne, amountIn, multX, multY);
    }

    function readState() external view returns (uint256[6] memory state) {
        return _s;
    }

    function cellKind() external pure returns (bytes32) {
        return "flashbots";
    }

    /// @dev ExamplePropAmm._quoteXtoY / _quoteYtoX, operation order preserved.
    function _quote(
        bool zeroForOne,
        uint256 amountIn,
        uint256 reserveX,
        uint256 targetX,
        uint256 concentration,
        uint256 multX,
        uint256 multY
    ) private pure returns (uint256 amountOut) {
        uint256 v0 = targetX * concentration;
        uint256 K = (v0 * v0 * multX) / multY;
        uint256 base = v0 + reserveX - targetX;
        if (zeroForOne) {
            amountOut = K / base - K / (base + amountIn);
        } else {
            amountOut = base - K / (K / base + amountIn);
        }
    }

    function _swap(bool zeroForOne, uint256 amountIn, uint256 multX, uint256 multY)
        private
        returns (uint256 amountOut)
    {
        amountOut = _quote(zeroForOne, amountIn, _s[0], _s[2], _s[3], multX, multY);
        if (zeroForOne) {
            _s[0] = _s[0] + amountIn;
            _s[1] = _s[1] - amountOut;
        } else {
            _s[1] = _s[1] + amountIn;
            _s[0] = _s[0] - amountOut;
        }
    }
}
