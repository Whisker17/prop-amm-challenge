// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

import {Fixtures} from "./Fixtures.sol";
import {IAdapter} from "../src/IAdapter.sol";

contract RecordProbeTest is Fixtures {
    function test_recordAccesses() public {
        (IAdapter dodo,) = deployDodo();
        (, bytes memory ret) = address(dodo).staticcall(abi.encodeWithSignature("cell()"));
        address cell = abi.decode(ret, (address));
        emit log_named_address("cell", cell);

        uint256 snap = vm.snapshotState();
        dodo.seedState(dodoState(false));
        bool ok1 = vm.revertToState(snap);
        bool ok2 = vm.revertToState(snap);
        emit log_named_string("revert #1", ok1 ? "ok" : "FAILED");
        emit log_named_string("revert #2", ok2 ? "ok" : "FAILED");

        dodo.seedState(dodoState(false));
        vm.record();
        dodo.swapAlgorithmOracle(true, X0 / 100, dodoOracle());
        (bytes32[] memory reads, bytes32[] memory writes) = vm.accesses(cell);
        emit log_named_uint("reads", reads.length);
        emit log_named_uint("writes", writes.length);
        assertGt(reads.length + writes.length, 0, "nothing recorded");
    }
}
