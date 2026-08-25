// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

/// @notice NULL TWIN of {DodoCell}.
///
/// Same pragma, same compiler settings, same storage layout, the SAME NUMBER AND SET OF
/// function selectors, empty bodies. Solidity dispatches by binary search over the sorted
/// selector table, so a twin with a different selector COUNT measures a different dispatch
/// path and corrupts `adjustedGas` (an earlier experiment in this workspace turned a true
/// 25 into 91 that way). The selector set is asserted equal in test/NullTwin.t.sol.
contract DodoCellNull {
    /// IDENTICAL layout to DodoCell: slots 0..5.
    uint256[6] internal _s;

    function quote(bool, uint256, uint256[6] calldata, uint256[2] calldata) external pure returns (uint256) {}

    function seedState(uint256[6] calldata) external {}

    function seedTick(int24, int128) external {}

    function swapAlgorithmOracle(bool, uint256, uint256[2] calldata) external returns (uint256) {}

    function swapReferenceOracle(bool, uint256, bytes calldata) external returns (uint256) {}

    function readState() external view returns (uint256[6] memory) {}

    function cellKind() external pure returns (bytes32) {}
}
