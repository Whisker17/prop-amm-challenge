// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

// Golden-vector generator for the Flashbots prop-AMM curve.
//
// This file is executed against a pristine copy of
//   flashbots/priority-update-registry @ da53117870c7bec96d71caebe1b3f94370aba3d6
// and it drives the *real* `ExamplePropAmm` contract from that commit: the pair
// is created, liquidity deposited, parameters published through
// `PrioUpdateRegistry`, and the recorded `amountOut` values come from
// `quoteXtoY` / `quoteYtoX` on the deployed contract.
//
// The reserve states that differ from `targetX` are reached by executing real
// swaps (the only way the contract decouples `reserveX` from `targetX`), so the
// vectors also pin the `base = v0 + reserveX - targetX` term away from zero.
//
// Nothing here is derived from the Rust port; the Rust port is tested against
// this output.

// Named imports only: `ExamplePropAmm.sol` declares its own `IERC20Metadata`,
// which collides with the OpenZeppelin symbol under a wildcard import.
import {Test, console} from "forge-std/Test.sol";
import {ExamplePropAmm} from "../src/ExamplePropAmm.sol";
import {PrioUpdateRegistry} from "../src/PrioUpdateRegistry.sol";
import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";

contract GenMockERC20 is ERC20 {
    constructor(string memory name, string memory symbol) ERC20(name, symbol) {}

    function mint(address to, uint256 amount) external {
        _mint(to, amount);
    }
}

contract FlashbotsGoldenGenTest is Test {
    uint256 private constant WAD = 1e18;
    uint256 private constant MAX_UPDATE_AGE = 1 hours;
    uint256 private constant MAX_UPDATE_LEAD_TIME = 1 hours;
    uint256 private constant MAX_PARAMETER_AGE = 12;

    // Unified benchmark initial state.
    uint256 private constant INITIAL_X = 100 * WAD;
    uint256 private constant INITIAL_Y = 10_000 * WAD;

    address private marketMaker = address(0xA11CE);
    address private trader = address(0xB0B);

    string private outPath;
    uint256 private written;
    bool private firstVector = true;

    function setUp() public {
        vm.warp(1_700_000_000);
        outPath = vm.envOr("VECTOR_OUT", string("./flashbots-golden-vectors.json"));
    }

    function test_generateGoldenVectors() public {
        uint256[8] memory concentrations = [uint256(1), 2, 5, 10, 20, 50, 100, 1000];
        // multX values are guide prices in WAD; multY is always 1e18 in this
        // benchmark, so multX/multY is the mid price of X in Y.
        uint256[4] memory prices = [uint256(100 * WAD), 80 * WAD, 125_500_000_000_000_000_000, 99_370_000_000_000_000_000];

        _begin();

        for (uint256 c; c < concentrations.length; ++c) {
            for (uint256 p; p < prices.length; ++p) {
                _emitParameterSet(concentrations[c], prices[p], 1e18);
            }
        }

        _end();
        console.log("vectors written:", written);
        assertGt(written, 200, "expected a broad vector set");
    }

    function _emitParameterSet(uint256 concentration, uint256 multX, uint256 multY) private {
        (ExamplePropAmm amm, PrioUpdateRegistry registry, GenMockERC20 tokenX, GenMockERC20 tokenY) = _deploy();

        vm.prank(marketMaker);
        bytes32 pairId = amm.createPair(address(tokenX), address(tokenY), concentration, 0, 0);

        vm.startPrank(marketMaker);
        tokenX.approve(address(amm), type(uint256).max);
        tokenY.approve(address(amm), type(uint256).max);
        amm.deposit(pairId, INITIAL_X, INITIAL_Y);
        vm.stopPrank();

        _publish(amm, registry, pairId, concentration, multX, multY);

        // State 0: reserveX == targetX.
        _recordState(amm, pairId, concentration, multX, multY, "initial");

        // Push reserveX above targetX with X->Y swaps.
        if (_trySwapXtoY(amm, pairId, 2 * WAD)) {
            _recordState(amm, pairId, concentration, multX, multY, "after-x-in-2");
        }
        if (_trySwapXtoY(amm, pairId, 8 * WAD)) {
            _recordState(amm, pairId, concentration, multX, multY, "after-x-in-10");
        }
        // Pull reserveX back below targetX with Y->X swaps.
        if (_trySwapYtoX(amm, pairId, 1_200 * WAD)) {
            _recordState(amm, pairId, concentration, multX, multY, "after-y-in-1200");
        }
        if (_trySwapYtoX(amm, pairId, 1_200 * WAD)) {
            _recordState(amm, pairId, concentration, multX, multY, "after-y-in-2400");
        }
    }

    function _deploy()
        private
        returns (ExamplePropAmm amm, PrioUpdateRegistry registry, GenMockERC20 tokenX, GenMockERC20 tokenY)
    {
        tokenX = new GenMockERC20("Token X", "TKX");
        tokenY = new GenMockERC20("Token Y", "TKY");
        registry = new PrioUpdateRegistry(MAX_UPDATE_AGE, MAX_UPDATE_LEAD_TIME);
        amm = new ExamplePropAmm(marketMaker, registry, MAX_PARAMETER_AGE);

        tokenX.mint(marketMaker, INITIAL_X);
        tokenY.mint(marketMaker, INITIAL_Y);
        tokenX.mint(trader, 1_000_000 * WAD);
        tokenY.mint(trader, 100_000_000 * WAD);

        vm.startPrank(trader);
        tokenX.approve(address(amm), type(uint256).max);
        tokenY.approve(address(amm), type(uint256).max);
        vm.stopPrank();
    }

    function _publish(
        ExamplePropAmm amm,
        PrioUpdateRegistry registry,
        bytes32 pairId,
        uint256 concentration,
        uint256 multX,
        uint256 multY
    ) private {
        uint256[] memory slots = amm.encodeParameterSlots(concentration, multX, multY);
        vm.prank(marketMaker);
        registry.updateState(address(amm), uint256(pairId), uint32(block.timestamp), slots);
    }

    function _trySwapXtoY(ExamplePropAmm amm, bytes32 pairId, uint256 amountIn) private returns (bool) {
        vm.prank(trader);
        try amm.swapXtoY(pairId, amountIn, 0) {
            return true;
        } catch {
            return false;
        }
    }

    function _trySwapYtoX(ExamplePropAmm amm, bytes32 pairId, uint256 amountIn) private returns (bool) {
        vm.prank(trader);
        try amm.swapYtoX(pairId, amountIn, 0) {
            return true;
        } catch {
            return false;
        }
    }

    struct Vector {
        string label;
        string direction;
        uint256 concentration;
        uint256 multX;
        uint256 multY;
        uint256 targetX;
        uint256 reserveX;
        uint256 reserveY;
        uint256 amountIn;
        uint256 amountOut;
    }

    function _recordState(
        ExamplePropAmm amm,
        bytes32 pairId,
        uint256 concentration,
        uint256 multX,
        uint256 multY,
        string memory label
    ) private {
        ExamplePropAmm.TradingPair memory pair = amm.getPair(pairId);

        Vector memory vector;
        vector.label = label;
        vector.concentration = concentration;
        vector.multX = multX;
        vector.multY = multY;
        vector.targetX = pair.targetX;
        vector.reserveX = pair.reserveX;
        vector.reserveY = pair.reserveY;

        uint256[8] memory xIn = [uint256(1), 1e9, 1e15, 1e18 / 1000, 1e17, WAD, 5 * WAD, 20 * WAD];
        vector.direction = "XtoY";
        for (uint256 i; i < xIn.length; ++i) {
            vector.amountIn = xIn[i];
            vector.amountOut = amm.quoteXtoY(pairId, xIn[i]);
            _writeVector(vector);
        }

        uint256[8] memory yIn = [uint256(1), 1e9, 1e15, 1e17, WAD, 100 * WAD, 1_000 * WAD, 5_000 * WAD];
        vector.direction = "YtoX";
        for (uint256 i; i < yIn.length; ++i) {
            vector.amountIn = yIn[i];
            vector.amountOut = amm.quoteYtoX(pairId, yIn[i]);
            _writeVector(vector);
        }
    }

    function _begin() private {
        vm.writeFile(outPath, "{\n");
        vm.writeLine(outPath, "  \"source\": {");
        vm.writeLine(outPath, "    \"repository\": \"https://github.com/flashbots/priority-update-registry\",");
        vm.writeLine(outPath, "    \"commit\": \"da53117870c7bec96d71caebe1b3f94370aba3d6\",");
        vm.writeLine(outPath, "    \"contract\": \"src/ExamplePropAmm.sol\",");
        vm.writeLine(outPath, "    \"functions\": [\"quoteXtoY\", \"quoteYtoX\"],");
        vm.writeLine(outPath, "    \"generator\": \"tools/research/solidity/flashbots-golden/FlashbotsGoldenGen.t.sol\",");
        vm.writeLine(outPath, "    \"note\": \"amountOut values are produced by executing the pinned Solidity contract\"");
        vm.writeLine(outPath, "  },");
        vm.writeLine(outPath, "  \"vectors\": [");
    }

    function _end() private {
        vm.writeLine(outPath, "");
        vm.writeLine(outPath, "  ],");
        vm.writeLine(outPath, string.concat("  \"count\": ", vm.toString(written)));
        vm.writeLine(outPath, "}");
    }

    function _writeVector(Vector memory vector) private {
        string memory body = string.concat(
            firstVector ? "    {" : "   ,{",
            "\"name\": \"c",
            vm.toString(vector.concentration),
            "-p",
            vm.toString(vector.multX),
            "-",
            vector.label,
            "-",
            vector.direction,
            "-",
            vm.toString(vector.amountIn),
            "\""
        );
        firstVector = false;
        body = string.concat(
            body,
            ", \"direction\": \"",
            vector.direction,
            "\", \"concentration\": ",
            vm.toString(vector.concentration),
            ", \"multX\": ",
            vm.toString(vector.multX),
            ", \"multY\": ",
            vm.toString(vector.multY)
        );
        body = string.concat(
            body,
            ", \"targetX\": ",
            vm.toString(vector.targetX),
            ", \"reserveX\": ",
            vm.toString(vector.reserveX),
            ", \"reserveY\": ",
            vm.toString(vector.reserveY)
        );
        body = string.concat(
            body,
            ", \"amountIn\": ",
            vm.toString(vector.amountIn),
            ", \"amountOut\": ",
            vm.toString(vector.amountOut),
            "}"
        );
        // One vector per line; the leading comma on every line after the first
        // keeps the JSON array well formed.
        vm.writeLine(outPath, body);
        written += 1;
    }
}
