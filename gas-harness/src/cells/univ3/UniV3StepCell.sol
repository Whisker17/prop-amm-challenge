// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity =0.7.6;

import "vendor/univ3/libraries/SwapMath.sol";

/// @notice Uniswap V3 -- metric A1, `primitiveStepGas`: EXACTLY ONE call to
///         `SwapMath.computeSwapStep`, the pinned upstream library
///         Uniswap/v3-core @ v1.0.0, contracts/libraries/SwapMath.sol
///         sha256 d6cb9a153be4ea9fb2377ef88641ef7979b5cee6933162f1b732d0289e26e1b6
///         compiled at the upstream hardhat configuration (0.7.6, optimizer, runs 800).
///
/// A1 IS NOT COMPARABLE with the DODO / Flashbots / Uniswap V2 whole-quote numbers, and it
/// is not comparable with V3's own A2 either. It is one iteration of A2's loop. It is
/// reported because it is the smallest honest unit of V3 curve maths, and because the ratio
/// A2 / A1 is what tick traversal actually costs.
///
/// `computeSwapStep` is `internal`, so solc INLINES it: there is no DELEGATECALL to measure,
/// which is why this cell must be compiled by the compiler that inlines it.
contract UniV3StepCell {
    function computeStep(
        uint160 sqrtRatioCurrentX96,
        uint160 sqrtRatioTargetX96,
        uint128 liquidity,
        int256 amountRemaining,
        uint24 feePips
    )
        external
        pure
        returns (uint160 sqrtRatioNextX96, uint256 amountIn, uint256 amountOut, uint256 feeAmount)
    {
        return SwapMath.computeSwapStep(
            sqrtRatioCurrentX96, sqrtRatioTargetX96, liquidity, amountRemaining, feePips
        );
    }
}
