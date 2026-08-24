// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

// Reference-vector generator for `FullMath.mulDiv` / `mulDivRoundingUp`,
// `SafeCast.toUint160` / `toInt128` / `toInt256` and `LiquidityMath.addDelta`.
//
// Every expected value in the emitted JSON is produced by EXECUTING the pinned
// Uniswap v3-core Solidity at solc 0.7.6:
//
//   Uniswap/v3-core tag v1.0.0 = commit e3589b192d0be27e100cd0daaf6c97204fdb1899
//     contracts/libraries/FullMath.sol       sha256 54087aee268a6938a85a408d7b14481b5c2c956c21508d5583f1bf48ec6d69ba
//     contracts/libraries/SafeCast.sol       sha256 9aed494b56d3dd16b7d6535583ded2cdfb03dc80aaa919347b13d35fd597e8bf
//     contracts/libraries/LiquidityMath.sol  sha256 84d20a16d5346f6ec4c12dff4df23dda5d46e52d33f18aaaaac2e9e36ce4a072
//
// Nothing here re-implements the maths. The generator only *chooses inputs*;
// the outputs come from `V3MathWrapper` (0.7.6), which is deployed from its
// compiled artifact with `vm.getCode` + `create` because a 0.8.x file may not
// import a 0.7.6-only source.
//
// Reverts are vectors too: every call goes through `try`/`catch`, so a revert
// is recorded as `"result": null, "reverted": true` instead of failing the run.
//
// This file deliberately depends on NOTHING (no forge-std): the cheatcode
// interface is declared locally, so the generator runs in a bare temporary
// forge project with no `lib/` and no network access.

interface Vm {
    function getCode(string calldata artifactPath) external view returns (bytes memory);

    function writeFile(string calldata path, string calldata data) external;

    function writeLine(string calldata path, string calldata data) external;

    function toString(bytes32 value) external pure returns (string memory);

    function toString(uint256 value) external pure returns (string memory);

    function envOr(string calldata name, string calldata defaultValue) external view returns (string memory);
}

interface IV3MathWrapper {
    function mulDiv(uint256 a, uint256 b, uint256 denominator) external pure returns (uint256);

    function mulDivRoundingUp(uint256 a, uint256 b, uint256 denominator) external pure returns (uint256);

    function toUint160(uint256 y) external pure returns (uint160);

    function toInt128(int256 y) external pure returns (int128);

    function toInt256(uint256 y) external pure returns (int256);

    function addDelta(uint128 x, int128 y) external pure returns (uint128);
}

contract V3MathVectorGenTest {
    Vm private constant vm = Vm(0x7109709ECfa91a80626fF3989D68f67F5b1DD12D);

    uint256 private constant MAX = type(uint256).max;
    uint256 private constant Q96 = 1 << 96;

    IV3MathWrapper private wrapper;

    string private fullMathOut;
    string private safeCastOut;
    string private path;
    bool private first;
    uint256 private written;
    uint256 private seed;

    function setUp() public {
        wrapper = IV3MathWrapper(_deployWrapper());
        fullMathOut = vm.envOr("FULLMATH_OUT", string("./fullmath-solidity-vectors.json"));
        safeCastOut = vm.envOr("SAFECAST_OUT", string("./safecast-solidity-vectors.json"));
        seed = uint256(keccak256("uniswap-v3-fullmath-vectors"));
    }

    function _deployWrapper() private returns (address addr) {
        bytes memory code = vm.getCode("V3MathWrapper.sol:V3MathWrapper");
        assembly {
            addr := create(0, add(code, 0x20), mload(code))
        }
        require(addr != address(0), "V3MathWrapper deployment failed");
    }

    // ------------------------------------------------------------------
    // FullMath
    // ------------------------------------------------------------------

    function test_generateFullMathVectors() public {
        _begin(fullMathOut, "contracts/libraries/FullMath.sol", "mulDiv, mulDivRoundingUp");

        // --- the exact revert conditions -------------------------------
        // denominator == 0 (both the prod1 == 0 branch and the 512-bit branch)
        _mulDivPair("den-zero-small", 1, 1, 0);
        _mulDivPair("den-zero-zero-product", 0, 0, 0);
        _mulDivPair("den-zero-max", MAX, MAX, 0);
        _mulDivPair("den-zero-one-sided", 0, MAX, 0);
        // prod1 >= denominator  (require(denominator > prod1))
        _mulDivPair("prod1-eq-den", 1 << 128, 1 << 128, 1); // product = 2^256, prod1 = 1
        _mulDivPair("prod1-gt-den", MAX, MAX, 1);
        _mulDivPair("prod1-eq-den-exact", MAX, MAX, MAX - 1); // prod1 == MAX - 1
        _mulDivPair("prod1-just-fits", MAX, MAX, MAX); // prod1 == MAX - 1 < MAX
        _mulDivPair("prod1-den-off-by-one", 1 << 255, 1 << 255, 1 << 254);
        _mulDivPair("prod1-den-boundary", 1 << 255, 1 << 255, (1 << 254) + 1);

        // --- products that genuinely exceed 256 bits and still fit ------
        _mulDivPair("wide-2p200", 1 << 200, 1 << 200, 1 << 200);
        _mulDivPair("wide-2p255", 1 << 255, 1 << 255, 1 << 255);
        _mulDivPair("wide-max-sq-over-max", MAX, MAX, MAX);
        _mulDivPair("wide-1e36-1e36", 1e36, 1e36, 1e36);
        _mulDivPair("wide-1e36-1e36-1e18", 1e36, 1e36, 1e18);
        _mulDivPair("wide-2p200-2p100", 1 << 200, 1 << 100, 1 << 90);
        _mulDivPair("wide-odd-den", (1 << 200) + 12345, (1 << 180) + 6789, (1 << 130) + 1);

        // --- MAX_U256 on every argument --------------------------------
        _mulDivPair("max-a", MAX, 1, 1);
        _mulDivPair("max-b", 1, MAX, 1);
        _mulDivPair("max-den", 1, 1, MAX);
        _mulDivPair("max-a-den", MAX, 1, MAX);
        _mulDivPair("max-b-den", 1, MAX, MAX);
        _mulDivPair("max-ab-den-half", MAX, MAX, 1 << 255);
        _mulDivPair("max-a-b-two", MAX, 2, MAX);
        _mulDivPair("max-a-den-two", MAX, MAX, 2);
        _mulDivPair("max-zero-a", 0, MAX, MAX);
        _mulDivPair("max-zero-b", MAX, 0, MAX);

        // --- a >= denominator and b >= denominator, both ---------------
        _mulDivPair("both-ge-den-small", 10, 10, 3);
        _mulDivPair("both-ge-den-wide", 1 << 200, 1 << 100, 1 << 90);
        _mulDivPair("both-ge-den-eq", 7, 7, 7);
        _mulDivPair("both-ge-den-max", MAX, MAX - 1, 3);
        _mulDivPair("a-ge-den-only", 1 << 200, 3, 1 << 100);
        _mulDivPair("b-ge-den-only", 3, 1 << 200, 1 << 100);

        // --- rounding-up specifics -------------------------------------
        _mulDivPair("round-exact", 6, 4, 3); // mulmod == 0
        _mulDivPair("round-remainder", 5, 5, 3); // mulmod != 0
        _mulDivPair("round-remainder-wide", (1 << 200) + 1, (1 << 200) + 1, (1 << 130) + 7);
        // floor(a*b/d) == type(uint256).max with a non-zero remainder, so
        // mulDivRoundingUp hits `require(result < type(uint256).max)`.
        _mulDivPair("round-overflow-plus-one", MAX - 1, (1 << 255) + 1, 1 << 255);
        _mulDivPair("round-max-exact", MAX, MAX, MAX); // result == MAX, mulmod == 0

        // --- the exact Uniswap V3 call shapes --------------------------
        // mulDiv(a, b, 1 << 96): SqrtPriceMath / SwapMath fee and delta maths.
        _mulDivPair("v3-q96-den-1", uint256(1e18), Q96, Q96);
        _mulDivPair("v3-q96-den-2", uint256(2) * 1e18, 79228162514264337593543950336, Q96);
        _mulDivPair("v3-q96-den-3", 1461446703485210103287273052203988822378723970342, uint256(1e18), Q96);
        _mulDivPair("v3-q96-den-4", 4295128739, uint256(1e18), Q96);
        _mulDivPair("v3-q96-den-5", uint256(1e18) << 96, uint256(3) << 96, Q96);
        _mulDivPair("v3-q96-den-6", MAX, Q96, Q96);
        // mulDiv(x, 1 << 96, y): getNextSqrtPriceFromAmount0RoundingUp shape.
        _mulDivPair("v3-q96-num-1", uint256(1e18), Q96, uint256(2e18));
        _mulDivPair("v3-q96-num-2", 79228162514264337593543950336, Q96, uint256(1e18) + 1);
        _mulDivPair("v3-q96-num-3", uint256(1e18) * 79228162514264337593543950336, Q96, MAX);
        _mulDivPair("v3-q96-num-4", 1461446703485210103287273052203988822378723970342, Q96, 4295128739);
        _mulDivPair("v3-q96-num-5", uint256(5192296858534827628530496329220096), Q96, 1461446703485210103287273052203988822378723970342);
        // liquidity * amount << 96 / sqrtP, the SqrtPriceMath numerator shape.
        _mulDivPair("v3-liq-1", uint256(1e18) << 96, 79228162514264337593543950336, 79228162514264337593543950336 + 1e12);
        _mulDivPair("v3-liq-2", uint256(1e24), 79228162514264337593543950336, 1 << 96);

        // --- zero / identity -------------------------------------------
        _mulDivPair("zero-a", 0, 12345, 7);
        _mulDivPair("zero-b", 12345, 0, 7);
        _mulDivPair("one-one-one", 1, 1, 1);
        _mulDivPair("den-one", 12345, 6789, 1);

        // --- deterministic pseudo-random batches -----------------------
        // (a) fully random triples: most of these revert, which pins the
        //     revert condition over a wide input space.
        for (uint256 i; i < 160; ++i) {
            _mulDivPair(string.concat("rnd-", vm.toString(i)), _rndOperand(), _rndOperand(), _rndOperand());
        }
        // (b) denominators forced above prod1 so the 512-by-256 division path
        //     succeeds. prod1 is computed here only to CHOOSE the denominator.
        for (uint256 i; i < 200; ++i) {
            uint256 a = _rndOperand();
            uint256 b = _rndOperand();
            (, uint256 prod1) = _product(a, b);
            // prod1 <= 2**256 - 2 always, so `prod1 + 1` cannot overflow; the
            // random offset on top of it can, and is clamped back.
            uint256 den;
            unchecked {
                den = prod1 + 1 + (_next() % (prod1 == 0 ? MAX : prod1 + 1));
            }
            if (den <= prod1) den = prod1 + 1; // wrapped: clamp to the boundary
            _mulDivPair(string.concat("wide-ok-", vm.toString(i)), a, b, den);
        }
        // (c) denominators at or below prod1: the overflow revert.
        for (uint256 i; i < 60; ++i) {
            uint256 a = _rndOperand() | (1 << 255);
            uint256 b = _rndOperand() | (1 << 255);
            (, uint256 prod1) = _product(a, b);
            uint256 den = prod1 == 0 ? 0 : (_next() % prod1) + 1;
            if (den > prod1) den = prod1;
            _mulDivPair(string.concat("wide-revert-", vm.toString(i)), a, b, den);
        }
        // (d) small denominators against wide products.
        for (uint256 i; i < 60; ++i) {
            _mulDivPair(string.concat("small-den-", vm.toString(i)), _rndOperand(), _rndOperand(), 1 + (_next() % 1000));
        }

        _end();
    }

    // ------------------------------------------------------------------
    // SafeCast + LiquidityMath
    // ------------------------------------------------------------------

    function test_generateSafeCastVectors() public {
        _begin(
            safeCastOut,
            "contracts/libraries/SafeCast.sol, contracts/libraries/LiquidityMath.sol",
            "toUint160, toInt128, toInt256, addDelta"
        );

        // SafeCast.toUint160: require((z = uint160(y)) == y)
        _toUint160("u160-zero", 0);
        _toUint160("u160-one", 1);
        _toUint160("u160-max", (1 << 160) - 1);
        _toUint160("u160-overflow", 1 << 160);
        _toUint160("u160-overflow-plus", (1 << 160) + 1);
        _toUint160("u160-max-u256", MAX);
        _toUint160("u160-2p161", 1 << 161);
        _toUint160("u160-min-sqrt", 4295128739);
        _toUint160("u160-max-sqrt", 1461446703485210103287273052203988822378723970342);
        for (uint256 i; i < 40; ++i) {
            _toUint160(string.concat("u160-rnd-", vm.toString(i)), _rndOperand());
        }

        // SafeCast.toInt128: require((z = int128(y)) == y)
        _toInt128("i128-zero", 0);
        _toInt128("i128-one", 1);
        _toInt128("i128-minus-one", -1);
        _toInt128("i128-max", int256(type(int128).max));
        _toInt128("i128-max-plus-one", int256(type(int128).max) + 1);
        _toInt128("i128-min", int256(type(int128).min));
        _toInt128("i128-min-minus-one", int256(type(int128).min) - 1);
        _toInt128("i128-int256-max", type(int256).max);
        _toInt128("i128-int256-min", type(int256).min);
        _toInt128("i128-2p128", int256(uint256(1) << 128));
        for (uint256 i; i < 40; ++i) {
            _toInt128(string.concat("i128-rnd-", vm.toString(i)), int256(_rndOperand()));
        }

        // SafeCast.toInt256: require(y < 2**255)
        _toInt256("i256-zero", 0);
        _toInt256("i256-one", 1);
        _toInt256("i256-max-ok", (uint256(1) << 255) - 1);
        _toInt256("i256-boundary", uint256(1) << 255);
        _toInt256("i256-boundary-plus", (uint256(1) << 255) + 1);
        _toInt256("i256-max-u256", MAX);
        _toInt256("i256-wad", 1e18);
        for (uint256 i; i < 40; ++i) {
            _toInt256(string.concat("i256-rnd-", vm.toString(i)), _rndOperand());
        }

        // LiquidityMath.addDelta
        _addDelta("delta-zero", 0, 0);
        _addDelta("delta-add-one", 0, 1);
        _addDelta("delta-sub-one-underflow", 0, -1);
        _addDelta("delta-sub-exact", 5, -5);
        _addDelta("delta-sub-too-much", 5, -6);
        _addDelta("delta-add-max", type(uint128).max, 1);
        _addDelta("delta-add-max-zero", type(uint128).max, 0);
        _addDelta("delta-add-max-int128", type(uint128).max, type(int128).max);
        _addDelta("delta-add-int128-max", 0, type(int128).max);
        _addDelta("delta-sub-int128-min-from-zero", 0, type(int128).min);
        _addDelta("delta-sub-int128-min-from-2p127", uint128(1) << 127, type(int128).min);
        _addDelta("delta-sub-int128-min-from-max", type(uint128).max, type(int128).min);
        _addDelta("delta-add-half", uint128(1) << 127, type(int128).max);
        _addDelta("delta-typical-add", 1e18, 1e17);
        _addDelta("delta-typical-sub", 1e18, -1e17);
        for (uint256 i; i < 60; ++i) {
            uint128 x = uint128(_rndOperand());
            int128 y = int128(uint128(_rndOperand()));
            _addDelta(string.concat("delta-rnd-", vm.toString(i)), x, y);
        }

        _end();
    }

    // ------------------------------------------------------------------
    // call + record
    // ------------------------------------------------------------------

    function _mulDivPair(string memory label, uint256 a, uint256 b, uint256 den) private {
        string memory args = string.concat(
            ", \"a\": \"",
            vm.toString(bytes32(a)),
            "\", \"b\": \"",
            vm.toString(bytes32(b)),
            "\", \"denominator\": \"",
            vm.toString(bytes32(den)),
            "\""
        );

        try wrapper.mulDiv(a, b, den) returns (uint256 result) {
            _write(label, "mulDiv", args, true, result);
        } catch {
            _write(label, "mulDiv", args, false, 0);
        }

        try wrapper.mulDivRoundingUp(a, b, den) returns (uint256 result) {
            _write(label, "mulDivRoundingUp", args, true, result);
        } catch {
            _write(label, "mulDivRoundingUp", args, false, 0);
        }
    }

    function _toUint160(string memory label, uint256 y) private {
        string memory args = string.concat(", \"y\": \"", vm.toString(bytes32(y)), "\"");
        try wrapper.toUint160(y) returns (uint160 z) {
            _write(label, "toUint160", args, true, uint256(z));
        } catch {
            _write(label, "toUint160", args, false, 0);
        }
    }

    function _toInt128(string memory label, int256 y) private {
        string memory args = string.concat(", \"y\": \"", vm.toString(bytes32(uint256(y))), "\"");
        try wrapper.toInt128(y) returns (int128 z) {
            _write(label, "toInt128", args, true, uint256(int256(z)));
        } catch {
            _write(label, "toInt128", args, false, 0);
        }
    }

    function _toInt256(string memory label, uint256 y) private {
        string memory args = string.concat(", \"y\": \"", vm.toString(bytes32(y)), "\"");
        try wrapper.toInt256(y) returns (int256 z) {
            _write(label, "toInt256", args, true, uint256(z));
        } catch {
            _write(label, "toInt256", args, false, 0);
        }
    }

    function _addDelta(string memory label, uint128 x, int128 y) private {
        string memory args = string.concat(
            ", \"x\": \"",
            vm.toString(bytes32(uint256(x))),
            "\", \"y\": \"",
            vm.toString(bytes32(uint256(int256(y)))),
            "\""
        );
        try wrapper.addDelta(x, y) returns (uint128 z) {
            _write(label, "addDelta", args, true, uint256(z));
        } catch {
            _write(label, "addDelta", args, false, 0);
        }
    }

    // ------------------------------------------------------------------
    // input choice helpers (these never compute an expected value)
    // ------------------------------------------------------------------

    function _next() private returns (uint256) {
        seed = uint256(keccak256(abi.encode(seed)));
        return seed;
    }

    /// A pseudo-random uint256 truncated to a pseudo-random bit width, so the
    /// batches cover every limb count rather than only full-width operands.
    function _rndOperand() private returns (uint256) {
        uint256 value = _next();
        uint256 bits = 1 + (_next() % 256);
        return value & (MAX >> (256 - bits));
    }

    /// The 512-bit product `[prod1 prod0] = a * b`, computed exactly as
    /// FullMath does. Used ONLY to pick denominators that land on a chosen
    /// side of `require(denominator > prod1)`.
    function _product(uint256 a, uint256 b) private pure returns (uint256 prod0, uint256 prod1) {
        assembly {
            let mm := mulmod(a, b, not(0))
            prod0 := mul(a, b)
            prod1 := sub(sub(mm, prod0), lt(mm, prod0))
        }
    }

    // ------------------------------------------------------------------
    // JSON emission
    // ------------------------------------------------------------------

    function _begin(string memory out, string memory contracts, string memory functions) private {
        path = out;
        first = true;
        written = 0;
        vm.writeFile(out, "{\n");
        vm.writeLine(out, "  \"source\": {");
        vm.writeLine(out, "    \"repository\": \"https://github.com/Uniswap/v3-core\",");
        vm.writeLine(out, "    \"tag\": \"v1.0.0\",");
        vm.writeLine(out, "    \"commit\": \"e3589b192d0be27e100cd0daaf6c97204fdb1899\",");
        vm.writeLine(out, string.concat("    \"contracts\": \"", contracts, "\","));
        vm.writeLine(out, string.concat("    \"functions\": \"", functions, "\","));
        vm.writeLine(out, "    \"solc\": \"0.7.6\",");
        vm.writeLine(out, "    \"generator\": \"crates/research/solidity/fullmath-vectors/V3MathVectorGen.t.sol\",");
        vm.writeLine(
            out,
            "    \"note\": \"every result is produced by executing the pinned Solidity; reverted calls are recorded with result=null\""
        );
        vm.writeLine(out, "  },");
        vm.writeLine(out, "  \"vectors\": [");
    }

    function _write(string memory label, string memory fn, string memory args, bool ok, uint256 result) private {
        string memory body = string.concat(
            first ? "    {" : "   ,{",
            "\"name\": \"",
            label,
            "\", \"fn\": \"",
            fn,
            "\"",
            args
        );
        first = false;
        body = string.concat(
            body,
            ", \"reverted\": ",
            ok ? "false" : "true",
            ", \"result\": ",
            ok ? string.concat("\"", vm.toString(bytes32(result)), "\"") : "null",
            "}"
        );
        vm.writeLine(path, body);
        written += 1;
    }

    function _end() private {
        vm.writeLine(path, "");
        vm.writeLine(path, "  ],");
        vm.writeLine(path, string.concat("  \"count\": ", vm.toString(written)));
        vm.writeLine(path, "}");
        require(written > 100, "expected a broad vector set");
    }
}
