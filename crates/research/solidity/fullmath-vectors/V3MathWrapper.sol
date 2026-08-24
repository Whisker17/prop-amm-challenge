// SPDX-License-Identifier: MIT
pragma solidity =0.7.6;

// Thin external wrapper around the *pinned* Uniswap v3-core libraries so that a
// 0.8.x test can reach them through a plain external call and observe both
// their return values and their reverts.
//
// Pinned source: Uniswap/v3-core tag v1.0.0
//                commit e3589b192d0be27e100cd0daaf6c97204fdb1899
//   contracts/libraries/FullMath.sol       sha256 54087aee268a6938a85a408d7b14481b5c2c956c21508d5583f1bf48ec6d69ba
//   contracts/libraries/SafeCast.sol       sha256 9aed494b56d3dd16b7d6535583ded2cdfb03dc80aaa919347b13d35fd597e8bf
//   contracts/libraries/LiquidityMath.sol  sha256 84d20a16d5346f6ec4c12dff4df23dda5d46e52d33f18aaaaac2e9e36ce4a072
//
// The libraries themselves are copied byte-for-byte into `src/v3-core/` by
// `generate.sh`, which verifies those hashes before running `forge`. The exact
// pragma matters: v3-core is a 0.7.6 codebase (FullMath's `-denominator` on a
// uint256 and TickMath's int24 cast do not compile under 0.8.x), so this
// wrapper must be compiled at 0.7.6 and reached by `vm.getCode` + `create`
// rather than by importing it from the 0.8.x test.
//
// Nothing here re-implements anything: every function body is a single call
// into the pinned library.

import {FullMath} from "./v3-core/FullMath.sol";
import {SafeCast} from "./v3-core/SafeCast.sol";
import {LiquidityMath} from "./v3-core/LiquidityMath.sol";

contract V3MathWrapper {
    function mulDiv(
        uint256 a,
        uint256 b,
        uint256 denominator
    ) external pure returns (uint256) {
        return FullMath.mulDiv(a, b, denominator);
    }

    function mulDivRoundingUp(
        uint256 a,
        uint256 b,
        uint256 denominator
    ) external pure returns (uint256) {
        return FullMath.mulDivRoundingUp(a, b, denominator);
    }

    function toUint160(uint256 y) external pure returns (uint160) {
        return SafeCast.toUint160(y);
    }

    function toInt128(int256 y) external pure returns (int128) {
        return SafeCast.toInt128(y);
    }

    function toInt256(uint256 y) external pure returns (int256) {
        return SafeCast.toInt256(y);
    }

    function addDelta(uint128 x, int128 y) external pure returns (uint128) {
        return LiquidityMath.addDelta(x, y);
    }
}
