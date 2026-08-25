// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

import {IStepAdapter} from "./IStepAdapter.sol";

/// @notice The A1 outer shell. Identical bytecode in front of {UniV3StepCell} and
///         {UniV3StepCellNull}.
contract StepAdapter is IStepAdapter {
    IStepAdapter public immutable cell;

    constructor(address cell_) {
        cell = IStepAdapter(cell_);
    }

    function computeStep(
        uint160 sqrtRatioCurrentX96,
        uint160 sqrtRatioTargetX96,
        uint128 liquidity,
        int256 amountRemaining,
        uint24 feePips
    ) external view override returns (uint160, uint256, uint256, uint256) {
        return cell.computeStep(sqrtRatioCurrentX96, sqrtRatioTargetX96, liquidity, amountRemaining, feePips);
    }
}
