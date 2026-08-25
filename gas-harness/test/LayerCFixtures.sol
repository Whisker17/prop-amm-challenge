// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

import {Fixtures} from "./Fixtures.sol";
import {IAdapter} from "../src/IAdapter.sol";
import {TestToken} from "../src/TestToken.sol";
import {DodoPool} from "../src/pools/DodoPool.sol";
import {RState} from "mantle-types/MantlePropAmmTypes.sol";
import {
    EndToEndRunner,
    IUniV2PairMinimal,
    IUniV3FactoryMinimal,
    IUniV3PoolMinimal,
    IExamplePropAmmMinimal
} from "../src/pools/EndToEndRunner.sol";
import {PrioUpdateRegistry} from "vendor/flashbots/PrioUpdateRegistry.sol";
import {ExamplePropAmm} from "vendor/flashbots/ExamplePropAmm.sol";

/// @notice Layer C: real pools, real tokens, real transfers.
abstract contract LayerCFixtures is Fixtures {
    TestToken internal tokenX;
    TestToken internal tokenY;
    EndToEndRunner internal runner;

    // uniswap v2
    IUniV2PairMinimal internal v2Canonical;
    IUniV2PairMinimal internal v2ZeroFee;
    IAdapter internal v2QuoteCanonical;
    IAdapter internal v2QuoteZeroFee;

    // uniswap v3
    IUniV3FactoryMinimal internal v3Factory;
    IUniV3PoolMinimal internal v3Pool;
    /// @notice the canonical 0.30% tier, for the SEPARATE production-fee sensitivity table.
    IUniV3PoolMinimal internal v3Pool3000;
    TestToken internal v3Token0;
    TestToken internal v3Token1;
    bool internal xIsToken0;
    int24 internal v3BaseTick;

    // dodo
    DodoPool internal dodoPool;

    // flashbots
    PrioUpdateRegistry internal fbRegistry;
    ExamplePropAmm internal fbAmm;
    bytes32 internal fbPairId;
    address internal marketMaker = address(uint160(uint256(keccak256("marketMaker"))));

    uint256 internal constant MINT_EACH = 1e30;
    uint256 internal constant FB_MAX_PARAM_AGE = 300;

    /// @notice `UniswapV2Pair` calls `IUniswapV2Factory(factory).feeTo()` inside `_mintFee`,
    ///         and `factory` is whoever deployed the pair -- here, this test contract, because
    ///         both pairs are deployed DIRECTLY rather than through `UniswapV2Factory`'s
    ///         CREATE2 (which is what lets the zero-fee variant exist without touching
    ///         `pairFor`'s hard-coded init-code hash). Returning `address(0)` is exactly what
    ///         the real factory returns when the protocol fee is off, which is its mainnet
    ///         state, so the measured path is the mainnet path.
    function feeTo() external pure returns (address) {
        return address(0);
    }

    function _setUpLayerC() internal {
        vm.warp(1_700_000_000);

        tokenX = new TestToken("Base", "X");
        tokenY = new TestToken("Quote", "Y");
        runner = new EndToEndRunner();

        tokenX.mint(address(runner), MINT_EACH);
        tokenY.mint(address(runner), MINT_EACH);
        // the control's counterparty needs an inventory, exactly as a pool does
        tokenX.mint(address(runner.sink()), MINT_EACH);
        tokenY.mint(address(runner.sink()), MINT_EACH);

        _setUpUniV2();
        _setUpUniV3();
        _setUpDodo();
        _setUpFlashbots();
    }

    // ------------------------------------------------------------------ uniswap v2

    function _setUpUniV2() private {
        // Deployed DIRECTLY, not through UniswapV2Factory's CREATE2, so that neither variant
        // depends on `UniswapV2Library.pairFor`'s hard-coded init-code hash. The deployer
        // becomes `factory` and is therefore allowed to call `initialize`.
        v2Canonical = IUniV2PairMinimal(deployCode("UniswapV2Pair.sol:UniswapV2Pair"));
        v2ZeroFee = IUniV2PairMinimal(deployCode("UniswapV2PairZeroFee.sol:UniswapV2PairZeroFee"));

        (address t0, address t1) = address(tokenX) < address(tokenY)
            ? (address(tokenX), address(tokenY))
            : (address(tokenY), address(tokenX));
        v2Canonical.initialize(t0, t1);
        v2ZeroFee.initialize(t0, t1);

        tokenX.mint(address(v2Canonical), X0);
        tokenY.mint(address(v2Canonical), Y0);
        v2Canonical.mint(address(this));

        tokenX.mint(address(v2ZeroFee), X0);
        tokenY.mint(address(v2ZeroFee), Y0);
        v2ZeroFee.mint(address(this));

        (v2QuoteCanonical,) = deployUniV2(false);
        (v2QuoteZeroFee,) = deployUniV2(true);
    }

    /// @dev Uses the PINNED library (through the layer A cell) rather than re-deriving the
    ///      constant-product formula in the test.
    function v2AmountOut(bool zeroFee, bool sellX, uint256 amountIn) internal view returns (uint256) {
        uint256[6] memory s = univ2State(false);
        IAdapter q = zeroFee ? v2QuoteZeroFee : v2QuoteCanonical;
        return q.quote(sellX, amountIn, s, emptyOracle());
    }

    // ------------------------------------------------------------------ uniswap v3

    function _setUpUniV3() private {
        v3Factory = IUniV3FactoryMinimal(deployCode("UniswapV3Factory.sol:UniswapV3Factory"));
        // Zero fee needs NO upstream modification: `enableFeeAmount` has no lower bound.
        v3Factory.enableFeeAmount(V3_FEE_ZERO, V3_TICK_SPACING);

        xIsToken0 = address(tokenX) < address(tokenY);
        v3Token0 = xIsToken0 ? tokenX : tokenY;
        v3Token1 = xIsToken0 ? tokenY : tokenX;

        v3Pool = IUniV3PoolMinimal(
            v3Factory.createPool(address(v3Token0), address(v3Token1), V3_FEE_ZERO)
        );
        // token1/token0 = 100 when X is token0, else 1/100.
        v3Pool.initialize(xIsToken0 ? SQRT_P_100 : uint160((uint256(1) << 96) / 10));

        runner.setV3Tokens(v3Token0, v3Token1);

        (, int24 tick,,,,,) = v3Pool.slot0();
        v3BaseTick = (tick / V3_TICK_SPACING) * V3_TICK_SPACING;
        if (tick < 0 && tick % V3_TICK_SPACING != 0) v3BaseTick -= V3_TICK_SPACING;

        // The canonical 0.30% tier (fee 3000 / tickSpacing 60) is enabled by the factory's own
        // constructor, so this pool needs no configuration at all. Same price, same ladder.
        v3Pool3000 =
            IUniV3PoolMinimal(v3Factory.createPool(address(v3Token0), address(v3Token1), V3_FEE_3000));
        v3Pool3000.initialize(xIsToken0 ? SQRT_P_100 : uint160((uint256(1) << 96) / 10));

        // one broad position that carries the depth ...
        runner.mintUniV3(v3Pool, v3BaseTick - 6000, v3BaseTick + 6000, V3_LIQUIDITY);
        // ... and a ladder of narrow positions whose boundaries are the INITIALIZED ticks the
        // swap loop will have to traverse. Ten on each side of the opening tick.
        for (uint256 i = 1; i <= 10; i++) {
            int24 lo = v3BaseTick + int24(uint24(i)) * V3_TICK_SPACING;
            runner.mintUniV3(v3Pool, lo, lo + V3_TICK_SPACING, 1e18);
            int24 lo2 = v3BaseTick - int24(uint24(i + 1)) * V3_TICK_SPACING;
            runner.mintUniV3(v3Pool, lo2, lo2 + V3_TICK_SPACING, 1e18);
        }
        // identical liquidity shape in the 0.30% pool
        runner.mintUniV3(v3Pool3000, v3BaseTick - 6000, v3BaseTick + 6000, V3_LIQUIDITY);
        for (uint256 i = 1; i <= 10; i++) {
            int24 lo = v3BaseTick + int24(uint24(i)) * V3_TICK_SPACING;
            runner.mintUniV3(v3Pool3000, lo, lo + V3_TICK_SPACING, 1e18);
            int24 lo2 = v3BaseTick - int24(uint24(i + 1)) * V3_TICK_SPACING;
            runner.mintUniV3(v3Pool3000, lo2, lo2 + V3_TICK_SPACING, 1e18);
        }
    }

    /// @notice Counts how many of the ladder's initialized ticks lie strictly between the
    ///         opening tick and `tickAfter`. This is the `tickCrossings` column.
    function v3Crossings(int24 tickBefore, int24 tickAfter) internal view returns (uint256 n) {
        int24 lo = tickBefore < tickAfter ? tickBefore : tickAfter;
        int24 hi = tickBefore < tickAfter ? tickAfter : tickBefore;
        for (int24 t = v3BaseTick - 11 * V3_TICK_SPACING; t <= v3BaseTick + 11 * V3_TICK_SPACING; t += V3_TICK_SPACING) {
            if (t > lo && t <= hi) n++;
        }
    }

    // ----------------------------------------------------------------------- dodo

    function _setUpDodo() private {
        dodoPool = new DodoPool(tokenX, tokenY);
        dodoPool.seed(X0, Y0, X0, Y0, RState.ONE);
        vm.prank(address(runner));
        tokenX.approve(address(dodoPool), type(uint256).max);
        vm.prank(address(runner));
        tokenY.approve(address(dodoPool), type(uint256).max);
    }

    // ------------------------------------------------------------------ flashbots

    function _setUpFlashbots() private {
        fbRegistry = new PrioUpdateRegistry(3600, 60);
        fbAmm = new ExamplePropAmm(marketMaker, fbRegistry, FB_MAX_PARAM_AGE);

        vm.prank(marketMaker);
        fbPairId = fbAmm.createPair(address(tokenX), address(tokenY), FB_CONCENTRATION, 0, 0);

        // The market maker publishes the real multipliers top-of-block, directly to the
        // registry -- the production path, not a helper on the AMM.
        uint256[] memory slots = new uint256[](3);
        slots[0] = FB_CONCENTRATION;
        slots[1] = PRICE_WAD;
        slots[2] = WAD;
        vm.prank(marketMaker);
        fbRegistry.updateState(address(fbAmm), uint256(fbPairId), uint32(block.timestamp), slots);

        tokenX.mint(marketMaker, X0);
        tokenY.mint(marketMaker, Y0);
        vm.startPrank(marketMaker);
        tokenX.approve(address(fbAmm), type(uint256).max);
        tokenY.approve(address(fbAmm), type(uint256).max);
        fbAmm.deposit(fbPairId, X0, Y0);
        vm.stopPrank();

        vm.prank(address(runner));
        tokenX.approve(address(fbAmm), type(uint256).max);
        vm.prank(address(runner));
        tokenY.approve(address(fbAmm), type(uint256).max);
    }
}
