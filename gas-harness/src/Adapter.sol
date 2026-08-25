// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

import {IAdapter} from "./IAdapter.sol";

/// @notice The ONE outer shell. Identical bytecode is deployed in front of every
///         algorithm cell AND in front of every null twin, so the shell's own dispatch,
///         calldata copy, external-call and return-data cost is identical on both sides
///         of `adjustedGas = rawGas - nullTwinGas` and cancels exactly.
///
/// The cell address is an immutable, i.e. it lives in code and costs no SLOAD, so the
/// shell adds no storage traffic of its own to any measurement.
contract Adapter is IAdapter {
    IAdapter public immutable cell;

    constructor(address cell_) {
        cell = IAdapter(cell_);
    }

    function quote(bool zeroForOne, uint256 amountIn, uint256[6] calldata state, uint256[2] calldata oracle)
        external
        view
        override
        returns (uint256 amountOut)
    {
        return cell.quote(zeroForOne, amountIn, state, oracle);
    }

    function seedState(uint256[6] calldata state) external override {
        cell.seedState(state);
    }

    function seedTick(int24 tick, int128 liquidityNet) external override {
        cell.seedTick(tick, liquidityNet);
    }

    function swapAlgorithmOracle(bool zeroForOne, uint256 amountIn, uint256[2] calldata oracle)
        external
        override
        returns (uint256 amountOut)
    {
        return cell.swapAlgorithmOracle(zeroForOne, amountIn, oracle);
    }

    function swapReferenceOracle(bool zeroForOne, uint256 amountIn, bytes calldata oracleData)
        external
        override
        returns (uint256 amountOut)
    {
        return cell.swapReferenceOracle(zeroForOne, amountIn, oracleData);
    }

    function readState() external view override returns (uint256[6] memory state) {
        return cell.readState();
    }

    function cellKind() external view override returns (bytes32) {
        return cell.cellKind();
    }
}
