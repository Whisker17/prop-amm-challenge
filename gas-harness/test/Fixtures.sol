// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

import {Test} from "forge-std/Test.sol";
import {IAdapter} from "../src/IAdapter.sol";
import {IStepAdapter} from "../src/IStepAdapter.sol";
import {Adapter} from "../src/Adapter.sol";
import {StepAdapter} from "../src/StepAdapter.sol";
import {DodoCell} from "../src/cells/dodo/DodoCell.sol";
import {DodoCellNull} from "../src/cells/dodo/DodoCellNull.sol";
import {FlashbotsCell} from "../src/cells/flashbots/FlashbotsCell.sol";
import {FlashbotsCellNull} from "../src/cells/flashbots/FlashbotsCellNull.sol";
import {PrioUpdateRegistry} from "vendor/flashbots/PrioUpdateRegistry.sol";

/// @notice Shared fixtures: the initial states, the trade-size ladder, and the deployment
///         helpers that cross compiler boundaries.
///
/// Cells compiled by a different solc than this file (Uniswap V2 at 0.6.6, Uniswap V3 at
/// 0.7.6) can never be `import`ed here -- a source file may not import a file with an
/// incompatible pragma. They are deployed by artifact path with `vm.deployCode` and driven
/// through an interface declared locally at THIS file's exact pragma. That is the only
/// cross-version pattern that works.
abstract contract Fixtures is Test {
    uint256 internal constant WAD = 1e18;

    // ---- the unified opening inventory, matching crates/research/README.md ----
    // fair price 100, reserveX 100, reserveY 10 000, zero fee.
    uint256 internal constant X0 = 100e18;
    uint256 internal constant Y0 = 10_000e18;
    uint256 internal constant PRICE_WAD = 100e18;

    // ---- the off-target / inventory-deviated state ----
    // the pool has absorbed 20% more base than its target and paid out quote for it.
    uint256 internal constant XD = 120e18;
    uint256 internal constant YD = 8_000e18;

    // DODO curvature and the Flashbots concentration it is paired with in
    // crates/research/README.md ("K ~= 1 / concentration", local curvature only).
    uint256 internal constant DODO_K = 0.1e18;
    uint256 internal constant FB_CONCENTRATION = 10;

    // RState enum values, from mantle-propamm-contracts src/MantlePropAmmTypes.sol.
    uint256 internal constant R_ONE = 0;
    uint256 internal constant R_ABOVE_ONE = 1;
    uint256 internal constant R_BELOW_ONE = 2;

    // plausible already-running values for the Uniswap V2 TWAP accumulator
    uint256 internal constant TWAP_SEED_0 = 1e30;
    uint256 internal constant TWAP_SEED_1 = 1e26;
    uint256 internal constant TS_SEED = 1_699_999_000;

    // ---- Uniswap V3 ----
    // sqrt(100) * 2**96
    uint160 internal constant SQRT_P_100 = 792281625142643375935439503360;
    uint128 internal constant V3_LIQUIDITY = 1e21;
    int24 internal constant V3_TICK_SPACING = 60;
    uint24 internal constant V3_FEE_ZERO = 0;
    uint24 internal constant V3_FEE_3000 = 3000;

    // trade-size ladder, in basis points of the RESERVE ON THE INPUT SIDE.
    // 0.01%, 0.1%, 1%, 5%, 10%
    function sizeBps() internal pure returns (uint256[5] memory bps) {
        bps = [uint256(1), 10, 100, 500, 1000];
    }

    function sizeLabel(uint256 i) internal pure returns (string memory) {
        string[5] memory labels = ["0.01%", "0.1%", "1%", "5%", "10%"];
        return labels[i];
    }

    // ------------------------------------------------------------------ states

    function dodoState(bool deviated) internal pure returns (uint256[6] memory s) {
        if (deviated) {
            // B > B0 and Q < Q0  =>  RState.BELOW_ONE (see PMMPricing.sellQuoteToken)
            s = [XD, YD, X0, Y0, R_BELOW_ONE, 0];
        } else {
            s = [X0, Y0, X0, Y0, R_ONE, 0];
        }
    }

    function dodoOracle() internal pure returns (uint256[2] memory o) {
        o = [PRICE_WAD, DODO_K];
    }

    /// @dev `targetYReference` (s[4]) is seeded NON-ZERO in both states. `ExamplePropAmm`
    ///      writes that slot on every swap, so leaving it at zero would charge the very first
    ///      swap a 0 -> non-zero SSTORE (20 000 gas) that no later swap ever pays, and the
    ///      reference-system caliper would report a one-off, not a steady state. The one-off
    ///      is measured separately by the `seedState-firstWrite-0-to-nonzero` row.
    function flashbotsState(bool deviated) internal pure returns (uint256[6] memory s) {
        if (deviated) {
            s = [XD, YD, X0, FB_CONCENTRATION, Y0, 0];
        } else {
            s = [X0, Y0, X0, FB_CONCENTRATION, Y0, 0];
        }
    }

    function flashbotsOracle() internal pure returns (uint256[2] memory o) {
        // ExamplePropAmm: multX is the price of X quoted in Y, multY the price of Y.
        o = [PRICE_WAD, WAD];
    }

    /// @dev The price cumulatives (s[2], s[3]) and `blockTimestampLast` (s[4]) are seeded
    ///      NON-ZERO for the same reason as `flashbotsState`: a pair whose TWAP accumulator
    ///      has never been written pays three 0 -> non-zero SSTOREs on its first swap
    ///      (60 000 gas) and never again. Steady state is the comparable number.
    function univ2State(bool deviated) internal pure returns (uint256[6] memory s) {
        if (deviated) {
            s = [XD, YD, TWAP_SEED_0, TWAP_SEED_1, TS_SEED, 0];
        } else {
            s = [X0, Y0, TWAP_SEED_0, TWAP_SEED_1, TS_SEED, 0];
        }
    }

    function univ3State(bool deviated, uint24 fee) internal pure returns (uint256[6] memory s) {
        // A deviated V3 pool is one whose price has already moved; the tick is re-derived by
        // the caller from the sqrt price, so only s[0] changes here.
        uint160 sqrtP = deviated ? uint160((uint256(SQRT_P_100) * 894427190) / 1e9) : SQRT_P_100;
        s = [
            uint256(sqrtP),
            uint256(V3_LIQUIDITY),
            0, // filled in by the caller with the tick for sqrtP
            uint256(int256(V3_TICK_SPACING)),
            uint256(fee),
            0
        ];
    }

    function emptyOracle() internal pure returns (uint256[2] memory o) {
        o = [uint256(0), 0];
    }

    // ------------------------------------------------------------- deployments

    function deployDodo() internal returns (IAdapter real, IAdapter twin) {
        real = IAdapter(address(new Adapter(address(new DodoCell()))));
        twin = IAdapter(address(new Adapter(address(new DodoCellNull()))));
    }

    function deployFlashbots(PrioUpdateRegistry registry, uint256 lane, uint256 maxAge)
        internal
        returns (IAdapter real, IAdapter twin)
    {
        real = IAdapter(address(new Adapter(address(new FlashbotsCell(registry, lane, maxAge)))));
        twin = IAdapter(address(new Adapter(address(new FlashbotsCellNull(registry, lane, maxAge)))));
    }

    /// @dev 0.6.6 artifacts: never importable from a 0.8.28 file.
    function deployUniV2(bool zeroFee) internal returns (IAdapter real, IAdapter twin) {
        address cell = zeroFee
            ? deployCode("UniV2CellZeroFee.sol:UniV2CellZeroFee")
            : deployCode("UniV2Cell.sol:UniV2Cell");
        real = IAdapter(address(new Adapter(cell)));
        twin = IAdapter(address(new Adapter(deployCode("UniV2CellNull.sol:UniV2CellNull"))));
    }

    /// @dev 0.7.6 artifacts.
    function deployUniV3FullQuote() internal returns (IAdapter real, IAdapter twin) {
        real = IAdapter(address(new Adapter(deployCode("UniV3FullQuoteCell.sol:UniV3FullQuoteCell"))));
        twin = IAdapter(address(new Adapter(deployCode("UniV3FullQuoteCellNull.sol:UniV3FullQuoteCellNull"))));
    }

    function deployUniV3Step() internal returns (IStepAdapter real, IStepAdapter twin) {
        real = IStepAdapter(address(new StepAdapter(deployCode("UniV3StepCell.sol:UniV3StepCell"))));
        twin = IStepAdapter(address(new StepAdapter(deployCode("UniV3StepCellNull.sol:UniV3StepCellNull"))));
    }

    // -------------------------------------------------------------- assertions

    /// @notice Asserts a cell and its null twin expose EXACTLY the same selector set.
    ///         A mismatch changes the binary-search dispatch path and silently corrupts
    ///         `adjustedGas`.
    function assertSameSelectors(bytes4[] memory a, bytes4[] memory b) internal pure {
        require(a.length == b.length, "selector COUNT mismatch between cell and null twin");
        for (uint256 i = 0; i < a.length; i++) {
            require(a[i] == b[i], "selector SET mismatch between cell and null twin");
        }
    }

    function uniformSelectors() internal pure returns (bytes4[] memory s) {
        s = new bytes4[](7);
        s[0] = IAdapter.quote.selector;
        s[1] = IAdapter.seedState.selector;
        s[2] = IAdapter.seedTick.selector;
        s[3] = IAdapter.swapAlgorithmOracle.selector;
        s[4] = IAdapter.swapReferenceOracle.selector;
        s[5] = IAdapter.readState.selector;
        s[6] = IAdapter.cellKind.selector;
    }
}
