// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

import {PrioUpdateRegistry} from "vendor/flashbots/PrioUpdateRegistry.sol";

/// @notice NULL TWIN of {FlashbotsCell}: same pragma, same compiler settings, same storage
///         layout, the same six selectors, empty bodies. Constructor arguments match so the
///         deployment script is uniform; constructors have no selector and do not affect
///         runtime dispatch.
contract FlashbotsCellNull {
    /// IDENTICAL layout to FlashbotsCell: slots 0..5.
    uint256[6] internal _s;

    PrioUpdateRegistry internal immutable prioRegistry;
    uint256 internal immutable laneIndex;
    uint256 internal immutable maxParameterAge;

    constructor(PrioUpdateRegistry registry_, uint256 laneIndex_, uint256 maxParameterAge_) {
        prioRegistry = registry_;
        laneIndex = laneIndex_;
        maxParameterAge = maxParameterAge_;
    }

    function quote(bool, uint256, uint256[6] calldata, uint256[2] calldata) external pure returns (uint256) {}

    function seedState(uint256[6] calldata) external {}

    function seedTick(int24, int128) external {}

    function swapAlgorithmOracle(bool, uint256, uint256[2] calldata) external returns (uint256) {}

    function swapReferenceOracle(bool, uint256, bytes calldata) external returns (uint256) {}

    function readState() external view returns (uint256[6] memory) {}

    function cellKind() external pure returns (bytes32) {}
}
