// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

import {RState} from "mantle-types/MantlePropAmmTypes.sol";
import {DecimalMath} from "vendor/dodo/DecimalMath.sol";
import {PMMPricing} from "vendor/dodo/PMMPricing.sol";

/// @notice DODO V2 PMM algorithm cell.
///
/// `PMMPricing`, `DODOMath` and `DecimalMath` are `internal` libraries: solc INLINES them,
/// there is no DELEGATECALL to measure, so this cell must be compiled by the same compiler
/// that inlines them. That is why the cell is pinned to =0.8.28 with optimizer runs 200,
/// exactly like the vendor copies (mantle-propamm-contracts @ 07f6797 foundry.toml).
///
/// Sources:
///   src/vendor/dodo/PMMPricing.sol   sha256 093b9adab96a57230d240984860c23580a2407e6085a3e57948d820ffe50807e
///   src/vendor/dodo/DODOMath.sol     sha256 90f688a26a7c6ad63b7f84b1c04cd61609540e14269f26810ad2cd80004c448a
///   src/vendor/dodo/DecimalMath.sol  sha256 27d9d19a79982c79bd9faa5ea2c2039be319b256acc898de282179d7bf256352
/// byte-identical copies of the read-only mantle-propamm-contracts @ 07f6797 vendor tree,
/// itself a 0.8.x transcription of DODOEX/contractV2 @ 8da3ee1ec50966fca9a2c80d424040c45c0f785e.
///
/// The quote + state-transition ORDER is transcribed from
///   mantle-propamm-contracts @ 07f6797, src/MantlePropAmmPool.sol lines 459-506:
///   build PMMState -> adjustedTarget -> sellBaseToken/sellQuoteToken -> lp fee ->
///   reserve update -> target update only when the RState changed.
/// Risk caps, pause checks, token transfers and quote-id replay protection are NOT curve
/// maths and are excluded here; token transfers are measured separately in layer C.
contract DodoCell {
    /// slot 0..5 -- IDENTICAL layout to DodoCellNull.
    /// s[0]=B  s[1]=Q  s[2]=B0  s[3]=Q0  s[4]=uint8(RState)  s[5]=lpFeeRate
    uint256[6] internal _s;

    /// @notice The Mantle reference architecture delivers DODO pricing in CALLDATA as a
    ///         struct, published by the market maker and validated against a
    ///         priority-update-registry state hash. This is the calldata shape the
    ///         reference-system caliper decodes.
    /// @dev The Mantle pool ADDITIONALLY re-derives and compares the registry state hash.
    ///      That check is not reproduced here, so the DODO reference-system number below is
    ///      a LOWER BOUND on the real Mantle path, not the whole of it.
    struct PricingStateCalldata {
        uint32 updateTimestamp;
        uint64 quoteId;
        uint256 i;
        uint256 k;
        uint256 lpFeeRate;
        bytes32 stateHash;
    }

    function quote(bool zeroForOne, uint256 amountIn, uint256[6] calldata state, uint256[2] calldata oracle)
        external
        pure
        returns (uint256 amountOut)
    {
        PMMPricing.PMMState memory pmm = PMMPricing.PMMState({
            i: oracle[0],
            K: oracle[1],
            B: state[0],
            Q: state[1],
            B0: state[2],
            Q0: state[3],
            R: RState(uint8(state[4]))
        });
        PMMPricing.adjustedTarget(pmm);
        uint256 gross;
        if (zeroForOne) {
            (gross,) = PMMPricing.sellBaseToken(pmm, amountIn);
        } else {
            (gross,) = PMMPricing.sellQuoteToken(pmm, amountIn);
        }
        return gross - DecimalMath.mulFloor(gross, state[5]);
    }

    function seedState(uint256[6] calldata state) external {
        _s = state;
    }

    /// @notice No-op: this algorithm has no ticks. Present so that every cell in the harness
    ///         exposes the SAME seven selectors and therefore the same dispatch table shape.
    function seedTick(int24, int128) external {}

    function swapAlgorithmOracle(bool zeroForOne, uint256 amountIn, uint256[2] calldata oracle)
        external
        returns (uint256 amountOut)
    {
        return _swap(zeroForOne, amountIn, oracle[0], oracle[1], _s[5]);
    }

    function swapReferenceOracle(bool zeroForOne, uint256 amountIn, bytes calldata oracleData)
        external
        returns (uint256 amountOut)
    {
        PricingStateCalldata memory p = abi.decode(oracleData, (PricingStateCalldata));
        // Freshness bound: the Mantle pool rejects a pricing state published in the future.
        require(p.updateTimestamp <= block.timestamp, "STALE");
        return _swap(zeroForOne, amountIn, p.i, p.k, p.lpFeeRate);
    }

    function readState() external view returns (uint256[6] memory state) {
        return _s;
    }

    function cellKind() external pure returns (bytes32) {
        return "dodo";
    }

    function _swap(bool zeroForOne, uint256 amountIn, uint256 i, uint256 k, uint256 lpFeeRate)
        private
        returns (uint256 amountOut)
    {
        RState rBefore = RState(uint8(_s[4]));
        PMMPricing.PMMState memory pmm =
            PMMPricing.PMMState({i: i, K: k, B: _s[0], Q: _s[1], B0: _s[2], Q0: _s[3], R: rBefore});
        PMMPricing.adjustedTarget(pmm);

        uint256 gross;
        RState rAfter;
        if (zeroForOne) {
            (gross, rAfter) = PMMPricing.sellBaseToken(pmm, amountIn);
        } else {
            (gross, rAfter) = PMMPricing.sellQuoteToken(pmm, amountIn);
        }
        amountOut = gross - DecimalMath.mulFloor(gross, lpFeeRate);

        // `pmm.B` / `pmm.Q` are the pre-trade reserves: adjustedTarget only mutates B0/Q0.
        if (zeroForOne) {
            _s[0] = pmm.B + amountIn;
            _s[1] = pmm.Q - amountOut;
            if (rAfter != rBefore) _s[2] = pmm.B0;
        } else {
            _s[0] = pmm.B - amountOut;
            _s[1] = pmm.Q + amountIn;
            if (rAfter != rBefore) _s[3] = pmm.Q0;
        }
        if (rAfter != rBefore) _s[4] = uint256(uint8(rAfter));
    }
}
