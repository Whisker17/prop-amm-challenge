// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

import {LayerCFixtures} from "./LayerCFixtures.sol";
import {IAdapter} from "../src/IAdapter.sol";
import {IStepAdapter} from "../src/IStepAdapter.sol";
import {IExamplePropAmmMinimal} from "../src/pools/EndToEndRunner.sol";
import {PrioUpdateRegistry} from "vendor/flashbots/PrioUpdateRegistry.sol";
import {DodoCell} from "../src/cells/dodo/DodoCell.sol";

/// @notice Produces research-out/gas/gas-snapshot.{csv,json} and touched-slots.csv.
///
/// EVERY measured operation is reported as three numbers, always:
///   rawGas       what the EVM actually charged for the call as made
///   nullTwinGas  the same call against a same-pragma, same-selector-set, same-storage-layout
///                twin with empty bodies
///   adjustedGas  rawGas - nullTwinGas
/// `adjustedGas` is a DIAGNOSTIC. It is what remains after dispatch, calldata-copy and
/// return-encoding overhead is removed, and it is meaningful only WITHIN one algorithm's own
/// compiler configuration. Raw system gas and adjusted curve gas are ranked separately in
/// README.md and neither is presented as the sole conclusion.
///
/// Every helper here is deliberately small and the row is assembled in a storage scratch
/// struct: solc 0.8.28 without `via_ir` runs out of stack slots long before this sweep runs
/// out of dimensions, and turning `via_ir` on would move the outer shell's codegen away from
/// the configuration the shell is pinned to. The scratch struct is only ever written AFTER a
/// measurement has been taken, so it cannot perturb one.
contract GasSnapshotTest is LayerCFixtures {
    string internal constant CSV_PATH = "../research-out/gas/gas-snapshot.csv";
    string internal constant JSON_PATH = "../research-out/gas/gas-snapshot.json";
    string internal constant SLOTS_PATH = "../research-out/gas/touched-slots.csv";

    string internal constant CFG_SHELL = "solc=0.8.28;opt=on;runs=200;viaIR=off;evmTarget=prague";
    string internal constant CFG_DODO =
        "solc=0.8.28;opt=on;runs=200;viaIR=off;evmTarget=prague;bytecodeHash=none";
    string internal constant CFG_FB =
        "solc=0.8.28;opt=on;runs=200;viaIR=off;evmTarget=prague;bytecodeHash=none";
    string internal constant CFG_V2Q =
        "solc=0.6.6;opt=on;runs=999999;viaIR=off;evmTarget=istanbul;bytecodeHash=none";
    string internal constant CFG_V2CORE =
        "solc=0.5.16;opt=on;runs=999999;viaIR=off;evmTarget=istanbul;bytecodeHash=none";
    string internal constant CFG_V3 =
        "solc=0.7.6;opt=on;runs=800;viaIR=off;evmTarget=istanbul;bytecodeHash=none";

    string internal constant SRC_DODO =
        "DODOEX/contractV2@8da3ee1ec50966fca9a2c80d424040c45c0f785e via mantle-propamm-contracts@07f6797";
    string internal constant SRC_FB =
        "flashbots/priority-update-registry@da53117870c7bec96d71caebe1b3f94370aba3d6";
    string internal constant SRC_V2PERIPH = "Uniswap/v2-periphery@ed24991304291297c3b4a52818d02f46a17aa9a2";
    string internal constant SRC_V2CORE = "Uniswap/v2-core@4dd59067c76dea4a0e8e4bfdda41877a6b16dedc(v1.0.1)";
    string internal constant SRC_V2CORE_PATCHED =
        "Uniswap/v2-core@4dd59067c76dea4a0e8e4bfdda41877a6b16dedc+patches/uniswap-v2-pair-zero-fee.patch";
    string internal constant SRC_V3CORE = "Uniswap/v3-core@e3589b192d0be27e100cd0daaf6c97204fdb1899(v1.0.0)";
    string internal constant SRC_HARNESS = "gas-harness(harness-authored; see README)";

    struct R {
        string algorithm;
        string layer;
        string operation;
        string direction;
        string state;
        string tradeSize;
        uint256 tickCrossings;
        uint256 coldGas;
        uint256 warmGas;
        uint256 rawGas;
        uint256 nullTwinGas;
        int256 gasRefunded;
        string compilerConfig;
        string sourceCommit;
        string note;
    }

    /// @notice One algorithm's identity, carried as a single memory pointer so the sweep loops
    ///         keep their stack shallow.
    struct Algo {
        string name;
        IAdapter real;
        IAdapter twin;
        uint256 family; // 0=dodo 1=flashbots 2=univ2-30bps 3=univ2-zero-fee
        string cfg;
        string src;
    }

    struct BMeasure {
        uint256 warmRaw;
        uint256 warmNull;
        uint256 coldRaw;
        uint256 coldNull;
        // vm.record()/vm.accesses() output, carried in MEMORY. `slotLines` is test-contract
        // STORAGE, and `vm.revertToState` rolls the test contract's storage back too, so
        // anything pushed to storage before the final revert would be silently erased.
        bytes32[] reads;
        bytes32[] writes;
    }

    /// @notice Raw gas of the token-only control at each trade-size index. Layer C has no
    ///         same-pragma empty-body twin -- there is no such thing as an "empty pool" that
    ///         still moves tokens -- so the control (one transfer out, one external call, one
    ///         transfer back, NO curve) is the declared baseline for the `nullTwinGas` column
    ///         on every layer C row. It is ALSO published as its own row, so the raw number is
    ///         never only available in subtracted form.
    uint256[5] internal _controlGas;

    string[] internal csvLines;
    string[] internal jsonObjs;
    string[] internal slotLines;
    R internal _row;

    function setUp() public {
        _setUpLayerC();
    }

    function test_generateSnapshot() public {
        csvLines.push(
            "algorithm,layer,operation,direction,state,tradeSize,tickCrossings,coldGas,warmGas,rawGas,nullTwinGas,adjustedGas,gasRefunded,compilerConfig,sourceCommit,note"
        );
        slotLines.push("algorithm,operation,accessKind,slot");

        _layerA();
        _layerB();
        _layerC();
        _write();

        assertGt(csvLines.length, 100, "snapshot suspiciously small");
        assertGt(slotLines.length, 1, "no touched slots recorded");
    }

    // ---------------------------------------------------------------- algo table

    function _algos() internal returns (Algo[] memory a) {
        PrioUpdateRegistry reg = new PrioUpdateRegistry(3600, 60);
        a = new Algo[](4);

        (IAdapter r0, IAdapter t0) = deployDodo();
        a[0] = Algo("dodo", r0, t0, 0, CFG_DODO, SRC_DODO);

        (IAdapter r1, IAdapter t1) = deployFlashbots(reg, 7, FB_MAX_PARAM_AGE);
        a[1] = Algo("flashbots", r1, t1, 1, CFG_FB, SRC_FB);

        (IAdapter r2, IAdapter t2) = deployUniV2(false);
        a[2] = Algo("univ2-canonical-30bps", r2, t2, 2, CFG_V2Q, SRC_V2PERIPH);

        (IAdapter r3, IAdapter t3) = deployUniV2(true);
        a[3] = Algo("univ2-modified-zero-fee", r3, t3, 3, CFG_V2Q, SRC_V2PERIPH);

        // the Flashbots reference caliper must read a REAL registry lane
        uint256[] memory slots = new uint256[](3);
        slots[0] = FB_CONCENTRATION;
        slots[1] = PRICE_WAD;
        slots[2] = WAD;
        reg.updateState(_cellOf(r1), 7, uint32(block.timestamp), slots);
    }

    function _stateFor(uint256 family, bool dev) internal pure returns (uint256[6] memory) {
        if (family == 0) return dodoState(dev);
        if (family == 1) return flashbotsState(dev);
        return univ2State(dev);
    }

    function _oracleFor(uint256 family) internal pure returns (uint256[2] memory) {
        if (family == 0) return dodoOracle();
        if (family == 1) return flashbotsOracle();
        return emptyOracle();
    }

    function _refDataFor(uint256 family) internal view returns (bytes memory) {
        if (family != 0) return bytes("");
        return abi.encode(
            DodoCell.PricingStateCalldata({
                updateTimestamp: uint32(block.timestamp),
                quoteId: 1,
                i: PRICE_WAD,
                k: DODO_K,
                lpFeeRate: 0,
                stateHash: keccak256("prio-state")
            })
        );
    }

    function _cellOf(IAdapter adapter) internal view returns (address) {
        (, bytes memory ret) = address(adapter).staticcall(abi.encodeWithSignature("cell()"));
        return abi.decode(ret, (address));
    }

    function _inputReserve(bool sellBase, bool dev) internal pure returns (uint256) {
        if (sellBase) return dev ? XD : X0;
        return dev ? YD : Y0;
    }

    // ================================================================== LAYER A

    function _layerA() internal {
        Algo[] memory a = _algos();
        for (uint256 k = 0; k < a.length; k++) {
            _quoteSweep(a[k]);
        }
        _v3QuoteSweep();
        _v3StepSweep();
    }

    function _quoteSweep(Algo memory a) internal {
        for (uint256 d = 0; d < 2; d++) {
            for (uint256 st = 0; st < 2; st++) {
                for (uint256 i = 0; i < 5; i++) {
                    _quoteOne(a, d == 0, st == 1, i);
                }
            }
        }
    }

    function _quoteOne(Algo memory a, bool sellBase, bool dev, uint256 i) internal {
        uint256 amountIn = (_inputReserve(sellBase, dev) * sizeBps()[i]) / 10_000;
        uint256 raw;
        uint256 nul;
        {
            uint256[6] memory s = _stateFor(a.family, dev);
            uint256[2] memory o = _oracleFor(a.family);
            // warm the accounts first, so the cold-account surcharge is not charged to a curve
            a.real.quote(sellBase, amountIn, s, o);
            a.twin.quote(sellBase, amountIn, s, o);
            a.real.quote(sellBase, amountIn, s, o);
            raw = vm.lastCallGas().gasTotalUsed;
            a.twin.quote(sellBase, amountIn, s, o);
            nul = vm.lastCallGas().gasTotalUsed;
        }
        _row.algorithm = a.name;
        _row.layer = "A";
        _row.operation = "quote";
        _row.direction = sellBase ? "sell-base" : "buy-base";
        _row.state = dev ? "off-target" : "balanced";
        _row.tradeSize = sizeLabel(i);
        _row.tickCrossings = 0;
        _row.coldGas = 0;
        _row.warmGas = 0;
        _row.rawGas = raw;
        _row.nullTwinGas = nul;
        _row.gasRefunded = 0;
        _row.compilerConfig = a.cfg;
        _row.sourceCommit = a.src;
        _row.note = "STATICCALL; state+oracle arrive as CALLDATA, never storage";
        _emit();
    }

    // ------------------------------------------------------------ V3 metric A2

    function _seedV3Cell(IAdapter cell, bool dev) internal returns (uint256[6] memory s) {
        (uint160 sp, int24 tk,,,,,) = v3Pool.slot0();
        s = univ3State(false, V3_FEE_ZERO);
        s[0] = uint256(sp);
        s[2] = uint256(int256(tk));
        if (dev) s[1] = uint256(V3_LIQUIDITY) / 2;
        cell.seedState(s);
        // mirrors the layer C ladder: adjacent positions share a boundary, so interior ladder
        // ticks carry liquidityNet == 0 and only the outermost carry +/- 1e18.
        cell.seedTick(v3BaseTick + V3_TICK_SPACING, int128(int256(1e18)));
        cell.seedTick(v3BaseTick - V3_TICK_SPACING, -int128(int256(1e18)));
        for (uint256 i = 2; i <= 10; i++) {
            cell.seedTick(v3BaseTick + int24(uint24(i)) * V3_TICK_SPACING, 0);
            cell.seedTick(v3BaseTick - int24(uint24(i)) * V3_TICK_SPACING, 0);
        }
        cell.seedTick(v3BaseTick + 11 * V3_TICK_SPACING, -int128(int256(1e18)));
        cell.seedTick(v3BaseTick - 11 * V3_TICK_SPACING, int128(int256(1e18)));
    }

    function _v3QuoteSweep() internal {
        (IAdapter real, IAdapter twin) = deployUniV3FullQuote();
        for (uint256 st = 0; st < 2; st++) {
            uint256[6] memory s = _seedV3Cell(real, st == 1);
            _seedV3Cell(twin, st == 1);
            for (uint256 d = 0; d < 2; d++) {
                for (uint256 i = 0; i < 5; i++) {
                    _v3QuoteOne(real, twin, s, d == 0, st == 1, i);
                }
            }
        }
    }

    function _v3QuoteOne(IAdapter real, IAdapter twin, uint256[6] memory s, bool sellBase, bool dev, uint256 i)
        internal
    {
        bool zeroForOne = xIsToken0 ? sellBase : !sellBase;
        uint256 amountIn = ((sellBase ? X0 : Y0) * sizeBps()[i]) / 10_000;
        uint256 crossings = _v3CellCrossings(real, zeroForOne, amountIn, s);
        uint256 raw;
        uint256 nul;
        {
            real.quote(zeroForOne, amountIn, s, emptyOracle());
            twin.quote(zeroForOne, amountIn, s, emptyOracle());
            real.quote(zeroForOne, amountIn, s, emptyOracle());
            raw = vm.lastCallGas().gasTotalUsed;
            twin.quote(zeroForOne, amountIn, s, emptyOracle());
            nul = vm.lastCallGas().gasTotalUsed;
        }
        _row.algorithm = "univ3-zero-fee";
        _row.layer = "A2";
        _row.operation = "fullQuote";
        _row.direction = sellBase ? "sell-base" : "buy-base";
        _row.state = dev ? "off-target" : "balanced";
        _row.tradeSize = sizeLabel(i);
        _row.tickCrossings = crossings;
        _row.coldGas = 0;
        _row.warmGas = 0;
        _row.rawGas = raw;
        _row.nullTwinGas = nul;
        _row.gasRefunded = 0;
        _row.compilerConfig = CFG_V3;
        _row.sourceCommit = SRC_V3CORE;
        _row.note = "A2: whole quote incl. tick traversal. ONLY A2 compares with other whole-quote rows";
        _emit();
    }

    /// @dev Runs the swap on a state snapshot to learn how many initialized ticks the quote
    ///      traverses, then rolls the state back so the measurement is unaffected.
    function _v3CellCrossings(IAdapter cell, bool zeroForOne, uint256 amountIn, uint256[6] memory s)
        internal
        returns (uint256)
    {
        uint256 snap = vm.snapshotState();
        cell.seedState(s);
        cell.swapAlgorithmOracle(zeroForOne, amountIn, emptyOracle());
        int24 tickAfter = int24(int256(cell.readState()[2]));
        vm.revertToState(snap);
        return v3Crossings(int24(int256(s[2])), tickAfter);
    }

    // ------------------------------------------------------------ V3 metric A1

    function _v3StepSweep() internal {
        (IStepAdapter real, IStepAdapter twin) = deployUniV3Step();
        uint160 target = uint160((uint256(SQRT_P_100) * 999) / 1000);
        for (uint256 i = 0; i < 5; i++) {
            int256 rem = int256((X0 * sizeBps()[i]) / 10_000);
            real.computeStep(SQRT_P_100, target, V3_LIQUIDITY, rem, 0);
            twin.computeStep(SQRT_P_100, target, V3_LIQUIDITY, rem, 0);
            real.computeStep(SQRT_P_100, target, V3_LIQUIDITY, rem, 0);
            uint256 raw = vm.lastCallGas().gasTotalUsed;
            twin.computeStep(SQRT_P_100, target, V3_LIQUIDITY, rem, 0);
            uint256 nul = vm.lastCallGas().gasTotalUsed;

            _row.algorithm = "univ3-zero-fee";
            _row.layer = "A1";
            _row.operation = "primitiveStep";
            _row.direction = "sell-base";
            _row.state = "balanced";
            _row.tradeSize = sizeLabel(i);
            _row.tickCrossings = 0;
            _row.coldGas = 0;
            _row.warmGas = 0;
            _row.rawGas = raw;
            _row.nullTwinGas = nul;
            _row.gasRefunded = 0;
            _row.compilerConfig = CFG_V3;
            _row.sourceCommit = SRC_V3CORE;
            _row.note = "A1: ONE SwapMath.computeSwapStep. NOT comparable with any whole-quote row, incl. V3 A2";
            _emit();
        }
    }

    // ================================================================== LAYER B

    function _layerB() internal {
        Algo[] memory a = _algos();
        for (uint256 k = 0; k < a.length; k++) {
            _stateUpdateSweep(a[k]);
        }
        _v3StateUpdateSweep();
        _firstWriteAndZeroing();
    }

    function _stateUpdateSweep(Algo memory a) internal {
        for (uint256 cal = 0; cal < 2; cal++) {
            for (uint256 d = 0; d < 2; d++) {
                for (uint256 st = 0; st < 2; st++) {
                    _stateUpdateOne(a, cal == 1, d == 0, st == 1);
                }
            }
        }
    }

    function _stateUpdateOne(Algo memory a, bool isRef, bool sellBase, bool dev) internal {
        BMeasure memory m = _measureB(a, isRef, sellBase, dev, _inputReserve(sellBase, dev) / 100);
        _pushSlots(a.name, isRef ? "stateUpdate-referenceOracle" : "stateUpdate-algorithmOracle", m);

        _row.algorithm = a.name;
        _row.layer = "B";
        _row.operation = isRef ? "stateUpdate-referenceOracle" : "stateUpdate-algorithmOracle";
        _row.direction = sellBase ? "sell-base" : "buy-base";
        _row.state = dev ? "off-target" : "balanced";
        _row.tradeSize = "1%";
        _row.tickCrossings = 0;
        _row.coldGas = m.coldRaw;
        _row.warmGas = m.warmRaw;
        _row.rawGas = m.warmRaw;
        _row.nullTwinGas = m.warmNull;
        _row.gasRefunded = 0;
        _row.compilerConfig = a.cfg;
        _row.sourceCommit = a.src;
        _row.note = isRef
            ? "SYSTEM cost: this algorithm's REAL oracle path. These are DIFFERENT PRODUCTS; MUST NOT be used to argue one curve's MATHS is dearer"
            : "CROSS-ALGORITHM number: oracle delivered through the identical adapter calldata slot for every algorithm";
        _emit();

        _row.operation = isRef ? "stateUpdate-referenceOracle-cold" : "stateUpdate-algorithmOracle-cold";
        _row.rawGas = m.coldRaw;
        _row.nullTwinGas = m.coldNull;
        _row.note = "vm.coolSlot on slots 0..5 of the CELL, the contract that OWNS them; cooling the Adapter wrapper does nothing";
        _emit();
    }

    function _measureB(Algo memory a, bool isRef, bool sellBase, bool dev, uint256 amountIn)
        internal
        returns (BMeasure memory m)
    {
        uint256[6] memory s0 = _stateFor(a.family, dev);
        uint256[2] memory o = _oracleFor(a.family);
        bytes memory refData = _refDataFor(a.family);
        uint256 snap = vm.snapshotState();

        // WARM: seed, run once to warm accounts and slots, revert, seed, measure.
        a.real.seedState(s0);
        a.twin.seedState(s0);
        _runB(a.real, isRef, sellBase, amountIn, o, refData);
        _runB(a.twin, isRef, sellBase, amountIn, o, refData);
        vm.revertToState(snap);

        a.real.seedState(s0);
        _runB(a.real, isRef, sellBase, amountIn, o, refData);
        m.warmRaw = vm.lastCallGas().gasTotalUsed;
        vm.revertToState(snap);

        a.twin.seedState(s0);
        _runB(a.twin, isRef, sellBase, amountIn, o, refData);
        m.warmNull = vm.lastCallGas().gasTotalUsed;
        vm.revertToState(snap);

        // COLD
        a.real.seedState(s0);
        _coolCell(_cellOf(a.real));
        _runB(a.real, isRef, sellBase, amountIn, o, refData);
        m.coldRaw = vm.lastCallGas().gasTotalUsed;
        vm.revertToState(snap);

        a.twin.seedState(s0);
        _coolCell(_cellOf(a.twin));
        _runB(a.twin, isRef, sellBase, amountIn, o, refData);
        m.coldNull = vm.lastCallGas().gasTotalUsed;
        vm.revertToState(snap);

        // touched slots
        a.real.seedState(s0);
        vm.record();
        _runB(a.real, isRef, sellBase, amountIn, o, refData);
        (m.reads, m.writes) = vm.accesses(_cellOf(a.real));
        vm.revertToState(snap);
    }

    function _runB(
        IAdapter target,
        bool isRef,
        bool sellBase,
        uint256 amountIn,
        uint256[2] memory o,
        bytes memory refData
    ) internal {
        if (isRef) {
            target.swapReferenceOracle(sellBase, amountIn, refData);
        } else {
            target.swapAlgorithmOracle(sellBase, amountIn, o);
        }
    }

    function _coolCell(address cell) internal {
        for (uint256 i = 0; i < 6; i++) {
            vm.coolSlot(cell, bytes32(i));
        }
    }

    /// @dev Must be called only AFTER the last `vm.revertToState`, because `slotLines` lives
    ///      in this test contract's storage and a state revert rolls that back as well.
    function _pushSlots(string memory algo, string memory op, BMeasure memory m) internal {
        for (uint256 i = 0; i < m.reads.length; i++) {
            slotLines.push(string.concat(algo, ",", op, ",read,", vm.toString(uint256(m.reads[i]))));
        }
        for (uint256 i = 0; i < m.writes.length; i++) {
            slotLines.push(string.concat(algo, ",", op, ",write,", vm.toString(uint256(m.writes[i]))));
        }
    }

    function _v3StateUpdateSweep() internal {
        (IAdapter real, IAdapter twin) = deployUniV3FullQuote();
        for (uint256 cal = 0; cal < 2; cal++) {
            for (uint256 d = 0; d < 2; d++) {
                _v3StateUpdateOne(real, twin, cal == 1, d == 0);
            }
        }
    }

    function _v3StateUpdateOne(IAdapter real, IAdapter twin, bool isRef, bool sellBase) internal {
        bool zeroForOne = xIsToken0 ? sellBase : !sellBase;
        uint256 amountIn = (sellBase ? X0 : Y0) / 100;
        BMeasure memory m;
        uint256 crossings;
        uint256 snap = vm.snapshotState();

        {
            uint256[6] memory s = _seedV3Cell(real, false);
            _seedV3Cell(twin, false);
            crossings = _v3CellCrossings(real, zeroForOne, amountIn, s);
            _runB(real, isRef, zeroForOne, amountIn, emptyOracle(), bytes(""));
            _runB(twin, isRef, zeroForOne, amountIn, emptyOracle(), bytes(""));
            vm.revertToState(snap);
        }

        _seedV3Cell(real, false);
        _runB(real, isRef, zeroForOne, amountIn, emptyOracle(), bytes(""));
        m.warmRaw = vm.lastCallGas().gasTotalUsed;
        vm.revertToState(snap);

        _seedV3Cell(twin, false);
        _runB(twin, isRef, zeroForOne, amountIn, emptyOracle(), bytes(""));
        m.warmNull = vm.lastCallGas().gasTotalUsed;
        vm.revertToState(snap);

        _seedV3Cell(real, false);
        _coolCell(_cellOf(real));
        _runB(real, isRef, zeroForOne, amountIn, emptyOracle(), bytes(""));
        m.coldRaw = vm.lastCallGas().gasTotalUsed;
        vm.revertToState(snap);

        _seedV3Cell(twin, false);
        _coolCell(_cellOf(twin));
        _runB(twin, isRef, zeroForOne, amountIn, emptyOracle(), bytes(""));
        m.coldNull = vm.lastCallGas().gasTotalUsed;
        vm.revertToState(snap);

        _seedV3Cell(real, false);
        vm.record();
        _runB(real, isRef, zeroForOne, amountIn, emptyOracle(), bytes(""));
        (m.reads, m.writes) = vm.accesses(_cellOf(real));
        vm.revertToState(snap);
        _pushSlots("univ3-zero-fee", isRef ? "stateUpdate-referenceOracle" : "stateUpdate-algorithmOracle", m);

        _row.algorithm = "univ3-zero-fee";
        _row.layer = "B";
        _row.operation = isRef ? "stateUpdate-referenceOracle" : "stateUpdate-algorithmOracle";
        _row.direction = sellBase ? "sell-base" : "buy-base";
        _row.state = "balanced";
        _row.tradeSize = "1%";
        _row.tickCrossings = crossings;
        _row.coldGas = m.coldRaw;
        _row.warmGas = m.warmRaw;
        _row.rawGas = m.warmRaw;
        _row.nullTwinGas = m.warmNull;
        _row.gasRefunded = 0;
        _row.compilerConfig = CFG_V3;
        _row.sourceCommit = SRC_V3CORE;
        _row.note = isRef
            ? "SYSTEM cost: V3's observation array at cardinality 1, written through the pinned Oracle library"
            : "CROSS-ALGORITHM number; V3 consumes no external oracle, the calldata slot is accepted and ignored";
        _emit();

        _row.operation = isRef ? "stateUpdate-referenceOracle-cold" : "stateUpdate-algorithmOracle-cold";
        _row.rawGas = m.coldRaw;
        _row.nullTwinGas = m.coldNull;
        _row.note = "vm.coolSlot on slots 0..5 of the CELL";
        _emit();
    }

    /// @notice 0 -> non-zero first write, non-zero overwrite, and the gross/refund split when
    ///         slots are zeroed. Measured on the DODO cell, whose six-word layout every cell
    ///         in this harness shares, so it is the harness-wide storage baseline.
    /// @notice 0 -> non-zero first write, non-zero overwrite, and the gross/refund split when
    ///         slots are zeroed. Measured on the DODO cell, whose six-word layout every cell
    ///         in this harness shares, so it is the harness-wide storage baseline.
    /// @dev All three rows are emitted only AFTER the last `vm.revertToState`: `csvLines` is
    ///      this contract's STORAGE and a state revert rolls it back too.
    function _firstWriteAndZeroing() internal {
        uint256[6] memory s = dodoState(false);
        uint256[6] memory zeros;
        uint256[6] memory firstPair;
        uint256[6] memory overPair;
        uint256[6] memory zeroPair;
        int256 refund;
        uint256 snap = vm.snapshotState();

        {
            (IAdapter fresh, IAdapter freshNull) = deployDodo();
            fresh.seedState(s);
            firstPair[0] = vm.lastCallGas().gasTotalUsed;
            freshNull.seedState(s);
            firstPair[1] = vm.lastCallGas().gasTotalUsed;
            vm.revertToState(snap);
        }
        {
            (IAdapter real, IAdapter twin) = deployDodo();
            real.seedState(s);
            real.seedState(s);
            overPair[0] = vm.lastCallGas().gasTotalUsed;
            twin.seedState(s);
            twin.seedState(s);
            overPair[1] = vm.lastCallGas().gasTotalUsed;
            vm.revertToState(snap);
        }
        {
            (IAdapter real, IAdapter twin) = deployDodo();
            real.seedState(s);
            real.seedState(zeros);
            zeroPair[0] = vm.lastCallGas().gasTotalUsed;
            refund = vm.lastCallGas().gasRefunded;
            twin.seedState(s);
            twin.seedState(zeros);
            zeroPair[1] = vm.lastCallGas().gasTotalUsed;
            vm.revertToState(snap);
        }

        // The `-sameTx` row above zeroes slots whose TRANSACTION-ORIGINAL value was 0, because
        // the seeding write happened in the same transaction (a forge test IS one transaction).
        // EIP-2200 then charges the dirty-slot price (100/slot) and hands back the full
        // 19 900/slot. The `-vmStoreSeeded` row installs the non-zero values with `vm.store`
        // instead. Both are published: they are different questions, and the harness cannot
        // reach a genuinely cross-transaction original from inside a single test.
        uint256[6] memory storePair;
        {
            (IAdapter real2, IAdapter twin2) = deployDodo();
            _storeState(_cellOf(real2), s);
            _storeState(_cellOf(twin2), s);
            real2.seedState(zeros);
            storePair[0] = vm.lastCallGas().gasTotalUsed;
            storePair[1] = uint256(int256(vm.lastCallGas().gasRefunded));
            twin2.seedState(zeros);
            storePair[2] = vm.lastCallGas().gasTotalUsed;
            vm.revertToState(snap);
        }

        // EVERY row is emitted only here, after the LAST revert: `csvLines` is this contract's
        // storage and `vm.revertToState` rolls that back too.
        _storageRow("seedState-firstWrite-0-to-nonzero", "fresh", firstPair[0], 0, firstPair[0], firstPair[1], 0);
        _storageRow("seedState-overwrite-nonzero", "warm", 0, overPair[0], overPair[0], overPair[1], 0);
        _storageRow("seedState-zeroing-nonzero-to-0-sameTx", "warm", 0, zeroPair[0], zeroPair[0], zeroPair[1], refund);
        _storageRow(
            "seedState-zeroing-nonzero-to-0-vmStoreSeeded",
            "warm",
            0,
            storePair[0],
            storePair[0],
            storePair[2],
            int256(storePair[1])
        );
    }

    function _storeState(address cell, uint256[6] memory s) internal {
        for (uint256 i = 0; i < 6; i++) {
            vm.store(cell, bytes32(i), bytes32(s[i]));
        }
    }

    function _storageRow(
        string memory op,
        string memory st,
        uint256 cold,
        uint256 warm,
        uint256 raw,
        uint256 nul,
        int256 refund
    ) internal {
        _row.algorithm = "dodo";
        _row.layer = "B";
        _row.operation = op;
        _row.direction = "n/a";
        _row.state = st;
        _row.tradeSize = "n/a";
        _row.tickCrossings = 0;
        _row.coldGas = cold;
        _row.warmGas = warm;
        _row.rawGas = raw;
        _row.nullTwinGas = nul;
        _row.gasRefunded = refund;
        _row.compilerConfig = CFG_DODO;
        _row.sourceCommit = SRC_HARNESS;
        _row.note =
            "rawGas is GROSS as metered; gasRefunded is the EIP-3529 refund counter, applied at transaction end and capped at gasUsed/5. Six-slot layout shared by every cell";
        _emit();
    }

    // ================================================================== LAYER C

    function _layerC() internal {
        for (uint256 i = 0; i < 5; i++) {
            _controlOne(i);
        }
        _feeSensitivity();
        for (uint256 d = 0; d < 2; d++) {
            for (uint256 i = 0; i < 5; i++) {
                _v2EndToEndOne(false, d == 0, i);
                _v2EndToEndOne(true, d == 0, i);
                _v3EndToEndOne(d == 0, i);
                _dodoEndToEndOne(d == 0, i);
                _flashbotsEndToEndOne(d == 0, i);
            }
        }
    }

    /// @notice SEPARATE production-fee sensitivity table. The main comparison is zero-fee;
    ///         these rows exist so the fee's own gas cost is visible and are tagged
    ///         `C-fee-sensitivity` so no reader can accidentally mix them into the main table.
    function _feeSensitivity() internal {
        for (uint256 d = 0; d < 2; d++) {
            bool sellBase = d == 0;
            uint256 amountIn = (sellBase ? X0 : Y0) / 100;

            // Uniswap V3 at the canonical 0.30% tier (fee 3000, tickSpacing 60)
            {
                bool zeroForOne = xIsToken0 ? sellBase : !sellBase;
                uint256 snap = vm.snapshotState();
                runner.swapUniV3(v3Pool3000, zeroForOne, amountIn);
                vm.revertToState(snap);
                runner.swapUniV3(v3Pool3000, zeroForOne, amountIn);
                uint256 raw = vm.lastCallGas().gasTotalUsed;
                vm.revertToState(snap);
                _sensitivityRow("univ3-production-3000", sellBase, raw, CFG_V3, SRC_V3CORE,
                    "SEPARATE sensitivity row: REAL UniswapV3Pool at the canonical 0.30% tier. Do NOT mix with the zero-fee main table");
            }
            // DODO at a production lpFeeRate of 3 bps
            {
                uint256 snap = vm.snapshotState();
                runner.swapDodo(dodoPool, sellBase, amountIn, PRICE_WAD, DODO_K, 3e14);
                vm.revertToState(snap);
                runner.swapDodo(dodoPool, sellBase, amountIn, PRICE_WAD, DODO_K, 3e14);
                uint256 raw = vm.lastCallGas().gasTotalUsed;
                vm.revertToState(snap);
                _sensitivityRow("dodo-production-lpFee-3bps", sellBase, raw, CFG_DODO, SRC_DODO,
                    "SEPARATE sensitivity row: lpFeeRate 3e14 (0.03%) on the harness-authored DODO pool. Do NOT mix with the zero-fee main table");
            }
        }
    }

    function _sensitivityRow(
        string memory algo,
        bool sellBase,
        uint256 raw,
        string memory cfg,
        string memory src,
        string memory note
    ) internal {
        _row.algorithm = algo;
        _row.layer = "C-fee-sensitivity";
        _row.operation = "swapEndToEnd";
        _row.direction = sellBase ? "sell-base" : "buy-base";
        _row.state = "balanced";
        _row.tradeSize = "1%";
        _row.tickCrossings = 0;
        _row.coldGas = 0;
        _row.warmGas = raw;
        _row.rawGas = raw;
        _row.nullTwinGas = _controlGas[2]; // the 1% token-only control
        _row.gasRefunded = 0;
        _row.compilerConfig = cfg;
        _row.sourceCommit = src;
        _row.note = note;
        _emit();
    }

    function _controlOne(uint256 i) internal {
        uint256 amountIn = (X0 * sizeBps()[i]) / 10_000;
        uint256 snap = vm.snapshotState();
        runner.controlTransferPair(tokenX, tokenY, amountIn, amountIn * 100);
        vm.revertToState(snap);
        runner.controlTransferPair(tokenX, tokenY, amountIn, amountIn * 100);
        uint256 raw = vm.lastCallGas().gasTotalUsed;
        vm.revertToState(snap);
        _controlGas[i] = raw;

        _row.algorithm = "token-only-control";
        _row.layer = "C";
        _row.operation = "transferPair";
        _row.direction = "sell-base";
        _row.state = "balanced";
        _row.tradeSize = sizeLabel(i);
        _row.tickCrossings = 0;
        _row.coldGas = 0;
        _row.warmGas = raw;
        _row.rawGas = raw;
        _row.nullTwinGas = 0;
        _row.gasRefunded = 0;
        _row.compilerConfig = CFG_SHELL;
        _row.sourceCommit = SRC_HARNESS;
        _row.note =
            "NO curve: one TestToken transfer out, one external call, one transfer back. THIS is the layer C baseline, so its own nullTwinGas is 0. Reported alongside every raw number, never only in subtracted form";
        _emit();
    }

    function _v2EndToEndOne(bool zeroFee, bool sellBase, uint256 i) internal {
        uint256 amountIn = ((sellBase ? X0 : Y0) * sizeBps()[i]) / 10_000;
        uint256 out = v2AmountOut(zeroFee, sellBase, amountIn);
        if (out == 0) return;
        uint256 raw;
        {
            bool dir = sellBase ? address(tokenX) < address(tokenY) : address(tokenY) < address(tokenX);
            uint256 snap = vm.snapshotState();
            runner.swapUniV2(
                zeroFee ? v2ZeroFee : v2Canonical, sellBase ? tokenX : tokenY, amountIn, dir, out
            );
            vm.revertToState(snap);
            runner.swapUniV2(
                zeroFee ? v2ZeroFee : v2Canonical, sellBase ? tokenX : tokenY, amountIn, dir, out
            );
            raw = vm.lastCallGas().gasTotalUsed;
            vm.revertToState(snap);
        }
        _row.algorithm = zeroFee ? "univ2-modified-zero-fee" : "univ2-canonical-30bps";
        _row.layer = "C";
        _row.operation = "swapEndToEnd";
        _row.direction = sellBase ? "sell-base" : "buy-base";
        _row.state = "balanced";
        _row.tradeSize = sizeLabel(i);
        _row.tickCrossings = 0;
        _row.coldGas = 0;
        _row.warmGas = raw;
        _row.rawGas = raw;
        _row.nullTwinGas = _controlGas[i];
        _row.gasRefunded = 0;
        _row.compilerConfig = CFG_V2CORE;
        _row.sourceCommit = zeroFee ? SRC_V2CORE_PATCHED : SRC_V2CORE;
        _row.note = zeroFee
            ? "BENCHMARK-ONLY zero-fee pair, deployed DIRECTLY (pairFor's init-code hash never used). NEVER mix with the canonical row. nullTwinGas here is the TOKEN-ONLY CONTROL"
            : "PRODUCTION REALITY: unmodified upstream pair at the pinned commit, 30 bps. nullTwinGas here is the TOKEN-ONLY CONTROL (no curve), not a same-pragma empty-body twin";
        _emit();
    }

    function _v3EndToEndOne(bool sellBase, uint256 i) internal {
        bool zeroForOne = xIsToken0 ? sellBase : !sellBase;
        uint256 amountIn = ((sellBase ? X0 : Y0) * sizeBps()[i]) / 10_000;
        uint256 raw;
        uint256 crossings;
        {
            uint256 snap = vm.snapshotState();
            (, int24 tickBefore,,,,,) = v3Pool.slot0();
            runner.swapUniV3(v3Pool, zeroForOne, amountIn);
            (, int24 tickAfter,,,,,) = v3Pool.slot0();
            crossings = v3Crossings(tickBefore, tickAfter);
            vm.revertToState(snap);
            runner.swapUniV3(v3Pool, zeroForOne, amountIn);
            vm.revertToState(snap);
            runner.swapUniV3(v3Pool, zeroForOne, amountIn);
            raw = vm.lastCallGas().gasTotalUsed;
            vm.revertToState(snap);
        }
        _row.algorithm = "univ3-zero-fee";
        _row.layer = "C";
        _row.operation = "swapEndToEnd";
        _row.direction = sellBase ? "sell-base" : "buy-base";
        _row.state = "balanced";
        _row.tradeSize = sizeLabel(i);
        _row.tickCrossings = crossings;
        _row.coldGas = 0;
        _row.warmGas = raw;
        _row.rawGas = raw;
        _row.nullTwinGas = _controlGas[i];
        _row.gasRefunded = 0;
        _row.compilerConfig = CFG_V3;
        _row.sourceCommit = SRC_V3CORE;
        _row.note =
            "REAL unmodified UniswapV3Pool; fee 0 enabled with factory.enableFeeAmount(0,60), which needs NO upstream change. nullTwinGas here is the TOKEN-ONLY CONTROL";
        _emit();
    }

    function _dodoEndToEndOne(bool sellBase, uint256 i) internal {
        uint256 amountIn = ((sellBase ? X0 : Y0) * sizeBps()[i]) / 10_000;
        uint256 snap = vm.snapshotState();
        runner.swapDodo(dodoPool, sellBase, amountIn, PRICE_WAD, DODO_K, 0);
        vm.revertToState(snap);
        runner.swapDodo(dodoPool, sellBase, amountIn, PRICE_WAD, DODO_K, 0);
        uint256 raw = vm.lastCallGas().gasTotalUsed;
        vm.revertToState(snap);

        _row.algorithm = "dodo";
        _row.layer = "C";
        _row.operation = "swapEndToEnd";
        _row.direction = sellBase ? "sell-base" : "buy-base";
        _row.state = "balanced";
        _row.tradeSize = sizeLabel(i);
        _row.tickCrossings = 0;
        _row.coldGas = 0;
        _row.warmGas = raw;
        _row.rawGas = raw;
        _row.nullTwinGas = _controlGas[i];
        _row.gasRefunded = 0;
        _row.compilerConfig = CFG_DODO;
        _row.sourceCommit = SRC_DODO;
        _row.note =
            "HARNESS-AUTHORED minimal pool around the pinned PMMPricing library. NOT a deployed DODO contract and NOT the Mantle pool. nullTwinGas here is the TOKEN-ONLY CONTROL. See README";
        _emit();
    }

    function _flashbotsEndToEndOne(bool sellBase, uint256 i) internal {
        uint256 amountIn = ((sellBase ? X0 : Y0) * sizeBps()[i]) / 10_000;
        uint256 snap = vm.snapshotState();
        _fbSwap(sellBase, amountIn);
        vm.revertToState(snap);
        _fbSwap(sellBase, amountIn);
        uint256 raw = vm.lastCallGas().gasTotalUsed;
        vm.revertToState(snap);

        _row.algorithm = "flashbots";
        _row.layer = "C";
        _row.operation = "swapEndToEnd";
        _row.direction = sellBase ? "sell-base" : "buy-base";
        _row.state = "balanced";
        _row.tradeSize = sizeLabel(i);
        _row.tickCrossings = 0;
        _row.coldGas = 0;
        _row.warmGas = raw;
        _row.rawGas = raw;
        _row.nullTwinGas = _controlGas[i];
        _row.gasRefunded = 0;
        _row.compilerConfig = CFG_FB;
        _row.sourceCommit = SRC_FB;
        _row.note =
            "REAL unmodified ExamplePropAmm + PrioUpdateRegistry, including Ownable/ReentrancyGuard/SafeERC20 and the registry read. nullTwinGas here is the TOKEN-ONLY CONTROL";
        _emit();
    }

    function _fbSwap(bool sellBase, uint256 amountIn) internal {
        if (sellBase) {
            runner.swapFlashbotsXtoY(IExamplePropAmmMinimal(address(fbAmm)), fbPairId, amountIn);
        } else {
            runner.swapFlashbotsYtoX(IExamplePropAmmMinimal(address(fbAmm)), fbPairId, amountIn);
        }
    }

    // ================================================================== output

    function _emit() internal {
        int256 adjusted = int256(_row.rawGas) - int256(_row.nullTwinGas);
        csvLines.push(
            string.concat(
                string.concat(
                    _row.algorithm, ",", _row.layer, ",", _row.operation, ",", _row.direction, ",", _row.state
                ),
                string.concat(
                    ",",
                    _row.tradeSize,
                    ",",
                    vm.toString(_row.tickCrossings),
                    ",",
                    vm.toString(_row.coldGas),
                    ",",
                    vm.toString(_row.warmGas)
                ),
                string.concat(
                    ",",
                    vm.toString(_row.rawGas),
                    ",",
                    vm.toString(_row.nullTwinGas),
                    ",",
                    vm.toString(adjusted),
                    ",",
                    vm.toString(_row.gasRefunded)
                ),
                string.concat(",", _row.compilerConfig, ",", _row.sourceCommit, ',"', _row.note, '"')
            )
        );
        jsonObjs.push(
            string.concat(
                string.concat(
                    '{"algorithm":"',
                    _row.algorithm,
                    '","layer":"',
                    _row.layer,
                    '","operation":"',
                    _row.operation,
                    '","direction":"',
                    _row.direction,
                    '","state":"',
                    _row.state,
                    '","tradeSize":"',
                    _row.tradeSize
                ),
                string.concat(
                    '","tickCrossings":',
                    vm.toString(_row.tickCrossings),
                    ',"coldGas":',
                    vm.toString(_row.coldGas),
                    ',"warmGas":',
                    vm.toString(_row.warmGas),
                    ',"rawGas":',
                    vm.toString(_row.rawGas)
                ),
                string.concat(
                    ',"nullTwinGas":',
                    vm.toString(_row.nullTwinGas),
                    ',"adjustedGas":',
                    vm.toString(adjusted),
                    ',"gasRefunded":',
                    vm.toString(_row.gasRefunded)
                ),
                string.concat(
                    ',"compilerConfig":"',
                    _row.compilerConfig,
                    '","sourceCommit":"',
                    _row.sourceCommit,
                    '","note":"',
                    _row.note,
                    '"}'
                )
            )
        );
    }

    function _write() internal {
        vm.writeFile(CSV_PATH, "");
        for (uint256 i = 0; i < csvLines.length; i++) {
            vm.writeLine(CSV_PATH, csvLines[i]);
        }

        vm.writeFile(SLOTS_PATH, "");
        for (uint256 i = 0; i < slotLines.length; i++) {
            vm.writeLine(SLOTS_PATH, slotLines[i]);
        }

        vm.writeFile(JSON_PATH, "");
        vm.writeLine(JSON_PATH, "{");
        vm.writeLine(JSON_PATH, '  "executionEvmSpec": "prague",');
        vm.writeLine(JSON_PATH, '  "forge": "1.4.1-v1.4.1",');
        vm.writeLine(
            JSON_PATH, string.concat('  "testTokenRuntimeCodeHash": "', vm.toString(address(tokenX).codehash), '",')
        );
        vm.writeLine(JSON_PATH, '  "rows": [');
        for (uint256 i = 0; i < jsonObjs.length; i++) {
            vm.writeLine(JSON_PATH, string.concat("    ", jsonObjs[i], i + 1 == jsonObjs.length ? "" : ","));
        }
        vm.writeLine(JSON_PATH, "  ]");
        vm.writeLine(JSON_PATH, "}");

        emit log_named_uint("rows written", csvLines.length - 1);
        emit log_named_uint("touched-slot records written", slotLines.length - 1);
    }
}
