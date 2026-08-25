// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

/// @notice The ONE outer-shell interface. Every algorithm is measured through this and
///         only through this, so the calldata shape, the selector set and the dispatch
///         path are byte-identical across algorithms.
///
/// Layout convention for `state` (6 words, always 6, never fewer, so the calldata cost
/// is identical for every algorithm):
///
/// | algorithm | s0        | s1        | s2        | s3            | s4               | s5          |
/// |-----------|-----------|-----------|-----------|---------------|------------------|-------------|
/// | dodo      | B         | Q         | B0        | Q0            | uint8(RState)    | lpFeeRate   |
/// | flashbots | reserveX  | reserveY  | targetX   | concentration | targetYReference | lockFlag    |
/// | univ2     | reserve0  | reserve1  | 0         | 0             | 0                | 0           |
/// | univ3     | sqrtPX96  | liquidity | int24 tick (two's complement in uint256) | tickSpacing | fee | 0 |
///
/// Layout convention for `oracle` (2 words, always 2):
///
/// | algorithm | o0                       | o1        |
/// |-----------|--------------------------|-----------|
/// | dodo      | i (guide price, WAD)     | K         |
/// | flashbots | multX                    | multY     |
/// | univ2     | ignored                  | ignored   |
/// | univ3     | ignored                  | ignored   |
interface IAdapter {
    /// @notice LAYER A. Pure quote maths. Inputs arrive as CALLDATA, never storage, and the
    ///         function is `view`, so the measured call is a STATICCALL.
    function quote(bool zeroForOne, uint256 amountIn, uint256[6] calldata state, uint256[2] calldata oracle)
        external
        view
        returns (uint256 amountOut);

    /// @notice Writes the cell's storage state. Used to set up layer B and to measure the
    ///         0 -> non-zero first-write case.
    function seedState(uint256[6] calldata state) external;

    /// @notice Initialises one Uniswap V3 tick. A NO-OP on every other algorithm.
    /// @dev It exists on every cell and every null twin ONLY so that the selector table has
    ///      the same shape -- same count, same sorted order -- for every algorithm. Solidity
    ///      dispatches by binary search over that table, so an algorithm with fewer selectors
    ///      would be charged a different (usually smaller) dispatch tax and the raw
    ///      cross-algorithm ranking would silently include it.
    function seedTick(int24 tick, int128 liquidityNet) external;

    /// @notice LAYER B, caliper 1 (algorithm-only). The oracle price is delivered through
    ///         this identical calldata slot for EVERY algorithm, so registry / calldata
    ///         architecture differences are excluded. THIS is the cross-algorithm number.
    function swapAlgorithmOracle(bool zeroForOne, uint256 amountIn, uint256[2] calldata oracle)
        external
        returns (uint256 amountOut);

    /// @notice LAYER B, caliper 2 (reference-system). Each algorithm's REAL oracle path.
    ///         DODO: pricing parameters delivered as an ABI-encoded calldata struct.
    ///         Flashbots: PrioUpdateRegistry.getState().
    ///         Uniswap V2: the price0/1CumulativeLast accumulator in _update().
    ///         Uniswap V3: an observation written through the pinned Oracle library.
    ///         These are DIFFERENT PRODUCTS. This number is a SYSTEM cost and must NOT be
    ///         used to argue that one curve's MATHS is more expensive than another's.
    function swapReferenceOracle(bool zeroForOne, uint256 amountIn, bytes calldata oracleData)
        external
        returns (uint256 amountOut);

    /// @notice Reads the cell's storage state back, for assertions.
    function readState() external view returns (uint256[6] memory state);

    /// @notice Identifies the cell. Present so the selector table has a fixed shape.
    function cellKind() external view returns (bytes32);
}
