// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity =0.7.6;

import "vendor/univ3/libraries/Tick.sol";
import "vendor/univ3/libraries/Oracle.sol";

/// @notice Storage layout shared by {UniV3FullQuoteCell} and its null twin
///         {UniV3FullQuoteCellNull}. Inheriting the layout from ONE base makes
///         "same storage layout" a compiler-enforced property rather than a promise.
contract UniV3CellStorage {
    // slot 0..5, the uniform six-word state used by every algorithm in this harness.
    // s[0]=sqrtPriceX96 s[1]=liquidity s[2]=tick (int24, two's complement)
    // s[3]=tickSpacing s[4]=fee (pips) s[5]=unused
    uint256[6] internal _s;

    // slot 6
    mapping(int16 => uint256) internal _tickBitmap;
    // slot 7
    mapping(int24 => Tick.Info) internal _ticks;
    // slot 8, packed
    uint16 internal _obsIndex;
    uint16 internal _obsCardinality;
    uint16 internal _obsCardinalityNext;
    // slot 9 .. 9+65534
    Oracle.Observation[65535] internal _observations;
}
