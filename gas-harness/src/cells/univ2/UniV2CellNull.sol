// SPDX-License-Identifier: MIT
pragma solidity =0.6.6;

/// @notice NULL TWIN of {UniV2Cell} and {UniV2CellZeroFee}. Both real cells expose exactly
///         the same six selectors and the same storage layout, so one twin type serves both;
///         a separate instance is deployed for each measurement so no storage is shared.
contract UniV2CellNull {
    /// IDENTICAL layout to UniV2Cell / UniV2CellZeroFee: slots 0..5.
    uint256[6] internal _s;

    function quote(bool, uint256, uint256[6] calldata, uint256[2] calldata) external pure returns (uint256) {}

    function seedState(uint256[6] calldata) external {}

    function seedTick(int24, int128) external {}

    function swapAlgorithmOracle(bool, uint256, uint256[2] calldata) external returns (uint256) {}

    function swapReferenceOracle(bool, uint256, bytes calldata) external returns (uint256) {}

    function readState() external view returns (uint256[6] memory) {}

    function cellKind() external pure returns (bytes32) {}
}
