// SPDX-License-Identifier: MIT
pragma solidity =0.5.16;

// Build target only.
//
// `UniswapV2Pair` is deployed by artifact name with `vm.deployCode`, never
// imported by the harness, so forge would not otherwise compile it: `lib/` is
// only built for what `src/` and `test/` reach. Importing it here forces the
// artifact to exist without vendoring a single line of upstream source into
// this repository.
import "vendor/univ2core/contracts/UniswapV2Pair.sol";
import "vendor/univ2core/contracts/UniswapV2Factory.sol";
