// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

import {RState} from "mantle-types/MantlePropAmmTypes.sol";
import {DecimalMath} from "vendor/dodo/DecimalMath.sol";
import {PMMPricing} from "vendor/dodo/PMMPricing.sol";
import {TestToken} from "../TestToken.sol";

/// @notice HARNESS-AUTHORED minimal DODO PMM pool for layer C.
///
/// THIS IS NOT A DEPLOYED DODO CONTRACT. DODO ships DVM / DSP / DPP, each of which wraps the
/// same `PMMPricing` library in a different amount of vault, LP-token, fee-split and
/// permission machinery; the Mantle reference pool (`MantlePropAmmPool`) wraps it in risk
/// caps, pause control and priority-update-registry replay protection. Any of those would
/// have made the layer C number a measurement of THAT WRAPPER rather than of DODO's curve.
///
/// This pool is therefore the thinnest wrapper that still does real work: the pinned
/// `PMMPricing` maths, the Mantle state transition, and two real ERC-20 transfers. It is
/// directly comparable with the UniswapV2 pair in call depth (one external contract, one
/// pull, one push) and it is NOT comparable with a production DODO deployment. Read the
/// layer C table with that caveat attached.
///
/// Pricing parameters arrive in CALLDATA, which is DODO-on-Mantle's real oracle path.
contract DodoPool {
    TestToken public immutable baseToken;
    TestToken public immutable quoteToken;

    uint256 public reserveBase;
    uint256 public reserveQuote;
    uint256 public targetBase;
    uint256 public targetQuote;
    RState public rState;

    constructor(TestToken base_, TestToken quote_) {
        baseToken = base_;
        quoteToken = quote_;
    }

    function seed(uint256 b, uint256 q, uint256 b0, uint256 q0, RState r) external {
        reserveBase = b;
        reserveQuote = q;
        targetBase = b0;
        targetQuote = q0;
        rState = r;
        baseToken.mint(address(this), b);
        quoteToken.mint(address(this), q);
    }

    /// @param sellBase true to pay base and receive quote.
    /// @param i        guide price, WAD, delivered by calldata (the Mantle oracle path).
    /// @param k        PMM curvature, WAD.
    /// @param lpFeeRate WAD; zero for the main comparison.
    function swap(bool sellBase, uint256 amountIn, uint256 i, uint256 k, uint256 lpFeeRate, address to)
        external
        returns (uint256 amountOut)
    {
        RState rBefore = rState;
        PMMPricing.PMMState memory pmm = PMMPricing.PMMState({
            i: i,
            K: k,
            B: reserveBase,
            Q: reserveQuote,
            B0: targetBase,
            Q0: targetQuote,
            R: rBefore
        });
        PMMPricing.adjustedTarget(pmm);

        uint256 gross;
        RState rAfter;
        if (sellBase) {
            (gross, rAfter) = PMMPricing.sellBaseToken(pmm, amountIn);
        } else {
            (gross, rAfter) = PMMPricing.sellQuoteToken(pmm, amountIn);
        }
        amountOut = gross - DecimalMath.mulFloor(gross, lpFeeRate);
        require(amountOut > 0, "DodoPool: zero out");

        if (sellBase) {
            baseToken.transferFrom(msg.sender, address(this), amountIn);
            reserveBase = pmm.B + amountIn;
            reserveQuote = pmm.Q - amountOut;
            if (rAfter != rBefore) targetBase = pmm.B0;
            quoteToken.transfer(to, amountOut);
        } else {
            quoteToken.transferFrom(msg.sender, address(this), amountIn);
            reserveQuote = pmm.Q + amountIn;
            reserveBase = pmm.B - amountOut;
            if (rAfter != rBefore) targetQuote = pmm.Q0;
            baseToken.transfer(to, amountOut);
        }
        if (rAfter != rBefore) rState = rAfter;
    }
}
