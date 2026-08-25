// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

// The three vendored DODO files under src/vendor/dodo/ are byte-identical copies of
//   mantle-propamm-contracts @ 07f6797  src/vendor/dodo/{DecimalMath,DODOMath,PMMPricing}.sol
// PMMPricing.sol imports `RState` from `../../MantlePropAmmTypes.sol`. This file
// reproduces ONLY that enum, verbatim, so the vendored copies stay byte-identical and
// their sha256 can be checked against the read-only reference checkout.
//
// Source: mantle-propamm-contracts @ 07f6797, src/MantlePropAmmTypes.sol lines 4-9,
// whose full-file sha256 is bee141418f90a91e3f76f291468e47ba39d7865be27fb4ff172fe308149517c8.

/// @notice DODO PMM reserve relation after the most recent state transition.
enum RState {
    ONE,
    ABOVE_ONE,
    BELOW_ONE
}
