// SPDX-License-Identifier: MIT
pragma solidity =0.7.6;

// Build target only; see BuildUniV2Core.sol. `UniswapV3Factory` embeds the
// pool creation code, so importing it also produces the `UniswapV3Pool`
// artifact the end-to-end fixtures deploy.
import "vendor/univ3/UniswapV3Factory.sol";
import "vendor/univ3/UniswapV3Pool.sol";
