// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity =0.7.6;

/// @notice NULL TWIN of {UniV3StepCell}: same pragma, same compiler settings, ONE selector
///         against ONE selector, identical signature, empty body, no storage on either side.
contract UniV3StepCellNull {
    function computeStep(uint160, uint160, uint128, int256, uint24)
        external
        pure
        returns (uint160, uint256, uint256, uint256)
    {}
}
