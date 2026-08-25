// SPDX-License-Identifier: GPL-3.0-or-later
pragma solidity =0.6.6;

import "../../patched/UniswapV2LibraryZeroFee.sol";
/// @dev Transcription of v2-core @ v1.0.1 contracts/libraries/UQ112x112.sol, which carries
///      an EXACT `pragma solidity =0.5.16` and therefore cannot be imported into a 0.6.6
///      compilation unit. Body, constant and operation order are unchanged.
library UQ112x112Local {
    uint224 constant Q112 = 2 ** 112;

    function encode(uint112 y) internal pure returns (uint224 z) {
        z = uint224(y) * Q112; // never overflows
    }

    function uqdiv(uint224 x, uint112 y) internal pure returns (uint224 z) {
        z = x / uint224(y);
    }
}

/// @notice Uniswap V2 algorithm cell, benchmark-only ZERO fee.
///
/// THIS IS NOT PRODUCTION UNISWAP V2. It exists only so that V2 sits on the same zero-fee
/// footing as DODO, Flashbots and (zero-fee-pool) V3 in the main comparison. The canonical
/// 30 bps cell {UniV2Cell} is measured separately and the two are never mixed.
///
/// `UniswapV2LibraryZeroFee.getAmountOut` is the pinned upstream quote:
///   Uniswap/v2-periphery @ ed24991304291297c3b4a52818d02f46a17aa9a2
///   contracts/libraries/UniswapV2Library.sol
///   sha256 4f83e9334f833568fa47b36e9ceca435f6c2962760a0596b043c4e538d0fd9f2
/// with the MINIMAL patch recorded in patches/uniswap-v2-library-zero-fee.patch
/// (line 46, `997` -> `1000`, plus the library rename and import-path rewrite forced by
/// moving the file). `pairFor` and its hard-coded init-code hash are NOT patched and are
/// NEVER called by this harness -- every pair is deployed directly and its address kept.
/// Compiled at the upstream .waffle.json configuration: solc 0.6.6, optimizer on,
/// runs 999999, evmVersion istanbul. `getAmountOut` is `internal`, so solc INLINES it and
/// there is no DELEGATECALL to measure.
///
/// Uniswap V2 consumes NO oracle, by construction: the `oracle` calldata words are accepted
/// (so the calldata shape is identical to every other algorithm) and ignored. That is the
/// point of including V2 -- it is the no-oracle baseline.
///
/// `swapReferenceOracle` measures V2's own "oracle": the `price0CumulativeLast` /
/// `price1CumulativeLast` TWAP accumulator that `UniswapV2Pair._update` maintains, using the
/// `UQ112x112` maths from v2-core @ v1.0.1 (transcribed above; see the note there).
contract UniV2CellZeroFee {
    using UQ112x112Local for uint224;

    /// slot 0..5 -- IDENTICAL layout to UniV2CellNull.
    /// s[0]=reserve0 s[1]=reserve1 s[2]=price0CumulativeLast s[3]=price1CumulativeLast
    /// s[4]=blockTimestampLast s[5]=unused
    /// The real pair packs reserve0/reserve1/blockTimestampLast into ONE slot; layer C
    /// measures the real pair. This cell keeps the uniform six-word layout so that layer A
    /// and layer B compare like with like across algorithms.
    uint256[6] internal _s;

    function quote(bool zeroForOne, uint256 amountIn, uint256[6] calldata state, uint256[2] calldata)
        external
        pure
        returns (uint256 amountOut)
    {
        if (zeroForOne) {
            return UniswapV2LibraryZeroFee.getAmountOut(amountIn, state[0], state[1]);
        }
        return UniswapV2LibraryZeroFee.getAmountOut(amountIn, state[1], state[0]);
    }

    function seedState(uint256[6] calldata state) external {
        _s[0] = state[0];
        _s[1] = state[1];
        _s[2] = state[2];
        _s[3] = state[3];
        _s[4] = state[4];
        _s[5] = state[5];
    }

    /// @notice No-op: this algorithm has no ticks. Present so that every cell in the harness
    ///         exposes the SAME seven selectors and therefore the same dispatch table shape.
    function seedTick(int24, int128) external {}

    function swapAlgorithmOracle(bool zeroForOne, uint256 amountIn, uint256[2] calldata)
        external
        returns (uint256 amountOut)
    {
        return _swap(zeroForOne, amountIn);
    }

    function swapReferenceOracle(bool zeroForOne, uint256 amountIn, bytes calldata)
        external
        returns (uint256 amountOut)
    {
        uint112 r0 = uint112(_s[0]);
        uint112 r1 = uint112(_s[1]);
        // UniswapV2Pair._update, transcribed (v2-core @ v1.0.1, contracts/UniswapV2Pair.sol).
        uint32 blockTimestamp = uint32(block.timestamp % 2 ** 32);
        uint32 timeElapsed = blockTimestamp - uint32(_s[4]);
        if (timeElapsed > 0 && r0 != 0 && r1 != 0) {
            _s[2] += uint256(UQ112x112Local.encode(r1).uqdiv(r0)) * timeElapsed;
            _s[3] += uint256(UQ112x112Local.encode(r0).uqdiv(r1)) * timeElapsed;
        }
        _s[4] = blockTimestamp;
        return _swap(zeroForOne, amountIn);
    }

    function readState() external view returns (uint256[6] memory state) {
        state[0] = _s[0];
        state[1] = _s[1];
        state[2] = _s[2];
        state[3] = _s[3];
        state[4] = _s[4];
        state[5] = _s[5];
    }

    function cellKind() external pure returns (bytes32) {
        return "univ2-zero-fee";
    }

    function _swap(bool zeroForOne, uint256 amountIn) private returns (uint256 amountOut) {
        if (zeroForOne) {
            amountOut = UniswapV2LibraryZeroFee.getAmountOut(amountIn, _s[0], _s[1]);
            _s[0] = _s[0] + amountIn;
            _s[1] = _s[1] - amountOut;
        } else {
            amountOut = UniswapV2LibraryZeroFee.getAmountOut(amountIn, _s[1], _s[0]);
            _s[1] = _s[1] + amountIn;
            _s[0] = _s[0] - amountOut;
        }
    }
}
