// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity =0.7.6;

import "./UniV3CellStorage.sol";

/// @notice NULL TWIN of {UniV3FullQuoteCell}. It inherits the SAME storage base, so the
///         layout is compiler-enforced identical, and it declares the SAME seven selectors
///         with empty bodies. The constructor is empty: the twin never writes an
///         observation, so no oracle state is created and none is charged for.
contract UniV3FullQuoteCellNull is UniV3CellStorage {
    function quote(bool, uint256, uint256[6] calldata, uint256[2] calldata) external view returns (uint256) {}

    function seedState(uint256[6] calldata) external {}

    function seedTick(int24, int128) external {}

    function swapAlgorithmOracle(bool, uint256, uint256[2] calldata) external returns (uint256) {}

    function swapReferenceOracle(bool, uint256, bytes calldata) external returns (uint256) {}

    function readState() external view returns (uint256[6] memory) {}

    function cellKind() external pure returns (bytes32) {}
}
