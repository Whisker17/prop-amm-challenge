// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

/// @notice Outer shell for metric A1 only. {UniV3StepCell} takes the five arguments
///         `SwapMath.computeSwapStep` takes, which do not fit the uniform six-word
///         {IAdapter} state shape. The SAME `StepAdapter` bytecode fronts the real cell and
///         the null twin, so the shell's own cost cancels in `adjustedGas` exactly as it
///         does for {IAdapter}.
interface IStepAdapter {
    function computeStep(
        uint160 sqrtRatioCurrentX96,
        uint160 sqrtRatioTargetX96,
        uint128 liquidity,
        int256 amountRemaining,
        uint24 feePips
    ) external view returns (uint160 sqrtRatioNextX96, uint256 amountIn, uint256 amountOut, uint256 feeAmount);
}
