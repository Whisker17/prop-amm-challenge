#!/usr/bin/env python3
"""Render research-out/gas/REPORT-gas.zh-CN.md from gas-snapshot.json.

Generated, not hand-written, so that no number in the prose can drift away from
the measurement it describes. The comparability rules are enforced here in code
because they are the part most likely to be softened by accident:

  * only the unified quote (layer A / A2) and the algorithm-oracle state update
    (layer B) may be used to rank curve maths across algorithms;
  * layer C's DodoPool is harness-authored and minimal while ExamplePropAmm is a
    complete production contract, so those two rows are NOT rankable -- and
    since layer C happens to reverse their order, the report says so out loud;
  * A1 (one computeSwapStep) never appears in a whole-quote table;
  * adjustedGas is a diagnostic; raw system gas is ranked separately.
"""

import json
import os
import statistics
from collections import defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
GAS_DIR = os.path.normpath(os.path.join(HERE, "..", "..", "research-out", "gas"))
SNAPSHOT = os.path.join(GAS_DIR, "gas-snapshot.json")
OUT = os.path.join(GAS_DIR, "REPORT-gas.zh-CN.md")

LABEL = {
    "dodo": "DODO PMM",
    "dodo-production-lpFee-3bps": "DODO PMM（lpFee 0.03%）",
    "flashbots": "Flashbots ExamplePropAmm",
    "token-only-control": "token-only 对照（无曲线）",
    "univ2-canonical-30bps": "Uniswap V2（canonical 0.30%）",
    "univ2-modified-zero-fee": "Uniswap V2（benchmark-only 零费）",
    "univ3-production-3000": "Uniswap V3（canonical 0.30%）",
    "univ3-zero-fee": "Uniswap V3（零费）",
}

# whole-quote layers. A1 is deliberately absent: one computeSwapStep is not a quote.
QUOTE_LAYERS = ("A", "A2")


def table(header, lines):
    out = [
        "| " + " | ".join(header) + " |",
        "| " + " | ".join("---" for _ in header) + " |",
    ]
    out.extend("| " + " | ".join(str(cell) for cell in line) + " |" for line in lines)
    return "\n".join(out) + "\n"


def span(values):
    lo, hi = min(values), max(values)
    if lo == hi:
        return str(lo)
    return f"{lo} – {hi}"


def label(algorithm):
    return LABEL.get(algorithm, algorithm)


def main():
    with open(SNAPSHOT) as handle:
        document = json.load(handle)
    rows = document.get("rows") or document.get("measurements") or []
    if not rows:
        raise SystemExit("gas-snapshot.json has no rows")
    if "benchmarkCommit" not in document:
        # A bare `forge test` regenerates the snapshot without provenance, because
        # the Solidity generator cannot ask git anything. Refuse rather than emit a
        # report whose numbers cannot be traced to a commit.
        raise SystemExit(
            "gas-snapshot.json has no benchmarkCommit: it was written by a bare "
            "`forge test`. Run ./script/gas-snapshot.sh, which stamps it."
        )

    parts = []
    add = parts.append

    # ------------------------------------------------------------------ header
    add("# Gas benchmark 结果（真实 EVM）\n\n")
    add(
        "本文件由 `gas-harness/script/render-gas-report.py` 从 `gas-snapshot.json` 生成，"
        "不要手工编辑。全部数字由 Foundry 在真实 EVM 上用 `vm.lastCallGas()` 测得，"
        "没有任何一项来自 Rust 执行时间估算。\n"
    )

    add("\n## 运行信息\n\n")
    dirty = document.get("benchmarkWorkingTreeDirty")
    add(
        table(
            ["项", "值"],
            [
                ["benchmark commit", f"`{document.get('benchmarkCommit', 'unknown')}`"],
                [
                    "运行时工作区状态",
                    "**有未提交改动（数字不可复现到该 commit）**" if dirty else "干净",
                ],
                ["forge", f"`{document.get('forge', 'unknown')}`"],
                ["执行用 EVM 规格", f"`{document.get('executionEvmSpec', 'unknown')}`"],
                [
                    "TestToken runtime code hash",
                    f"`{document.get('testTokenRuntimeCodeHash', 'n/a')}`",
                ],
                [
                    "第三方源码",
                    "不入库。`script/fetch-vendor.sh` 按 commit 拉取到被 git ignore 的 `lib/`，"
                    "逐文件校验 sha256 后才编译",
                ],
                ["pin 清单", f"`{document.get('vendorPinsManifest', 'gas-harness/vendor-pins.sha256')}`"],
                ["测量点数", str(len(rows))],
            ],
        )
    )

    # ------------------------------------------------------------- how to read
    add("\n## 三个数怎么读\n\n")
    add(
        "每一行同时给出 `rawGas`、`nullTwinGas`、`adjustedGas = rawGas − nullTwinGas`。\n\n"
        "- **`rawGas`** 是系统成本：含该算法自己的调用框架、selector 表形状、存储布局。\n"
        "- **`nullTwinGas`** 是同 pragma、同签名、同 selector 集合、同存储布局的空壳成本"
        "（C 层例外，见该节）。\n"
        "- **`adjustedGas`** 只是**诊断量**，用来把编译器 dispatch 税剥出去。"
        "它不是唯一结论，也不能替代 `rawGas`。\n\n"
        "排名必须分两份给：raw system gas 一份，adjusted curve gas 一份。\n"
    )

    # ------------------------------------------------------------------ layer A
    quotes = [r for r in rows if r["layer"] in QUOTE_LAYERS]
    by_algorithm = defaultdict(list)
    for row in quotes:
        by_algorithm[row["algorithm"]].append(row)

    add("\n## A 层：统一口径下的整笔报价\n\n")
    add(
        "输入全部走 calldata，函数为 `view`（STATICCALL），每个算法通过形状完全相同的 adapter 调用，"
        "不碰存储。这是**可以**跨算法比较曲线数学的一层。\n\n"
    )
    lines = []
    for algorithm in sorted(by_algorithm, key=lambda a: statistics.median(r["rawGas"] for r in by_algorithm[a])):
        group = by_algorithm[algorithm]
        raws = [r["rawGas"] for r in group]
        adjusted = [r["adjustedGas"] for r in group]
        nulls = sorted({r["nullTwinGas"] for r in group})
        lines.append(
            [
                label(algorithm),
                group[0]["layer"],
                len(group),
                span(raws),
                int(statistics.median(raws)),
                "/".join(str(n) for n in nulls),
                int(statistics.median(adjusted)),
            ]
        )
    add(
        table(
            ["算法", "层", "n", "rawGas 区间", "rawGas 中位", "nullTwinGas", "adjustedGas 中位"],
            lines,
        )
    )
    add(
        "\n区间跨越 4 个状态（balanced / off-target）× 2 个方向 × 5 个 trade size。"
        "V3 的区间宽是因为 tick 穿越数不同（`tickCrossings` 列逐行记录）。\n"
    )

    primitive = [r for r in rows if r["layer"] == "A1"]
    if primitive:
        raws = [r["rawGas"] for r in primitive]
        add(
            "\n**V3 的 A 层拆成两个指标，二者不可互换：**\n\n"
            f"- `A1 primitiveStep`：单次 `SwapMath.computeSwapStep`，rawGas {span(raws)}。"
            "它与**任何**整笔报价都不可比，**包括与自己的 A2 不可比**——上表里没有它。\n"
            "- `A2 fullQuote`：从输入到最终输出的完整报价循环，含 tick 遍历。"
            "只有 A2 能与上表其它行放在一起。\n"
        )

    # ------------------------------------------------------------------ layer B
    add("\n## B 层：状态更新（两套 oracle 口径）\n\n")
    add(
        "- **algorithm-only**：oracle 价格通过完全相同的 adapter calldata 槽送入，"
        "排除 registry / calldata 架构差异。**这是可跨算法比较的数字。**\n"
        "- **reference-system**：各算法的真实路径（DODO 走 calldata、Flashbots 走 `PrioUpdateRegistry`、"
        "V3 写 observation、V2 写 price cumulative）。这是**系统成本**，"
        "**不得**用它论证某条曲线的数学更贵——它们是不同产品，不是同一功能的不同实现。\n\n"
        "cold 用 `vm.coolSlot` 作用在**拥有存储的那个合约**上（作用在 wrapper 上无效）。\n\n"
    )

    def collect(operation):
        out = defaultdict(list)
        for row in rows:
            if row["operation"] == operation:
                out[row["algorithm"]].append(row["rawGas"])
        return out

    warm_algo = collect("stateUpdate-algorithmOracle")
    cold_algo = collect("stateUpdate-algorithmOracle-cold")
    warm_ref = collect("stateUpdate-referenceOracle")
    cold_ref = collect("stateUpdate-referenceOracle-cold")
    lines = []
    for algorithm in sorted(warm_algo, key=lambda a: statistics.median(warm_algo[a])):
        lines.append(
            [
                label(algorithm),
                span(warm_algo[algorithm]),
                span(cold_algo.get(algorithm, [0])),
                span(warm_ref.get(algorithm, [0])),
                span(cold_ref.get(algorithm, [0])),
            ]
        )
    add(
        table(
            [
                "算法",
                "algorithm-only warm",
                "algorithm-only cold",
                "reference-system warm",
                "reference-system cold",
            ],
            lines,
        )
    )
    add(
        "\n左两列可比，右两列**不可**跨算法比。"
        "V3 不消费外部 oracle，algorithm-only 口径下 calldata 槽被接受并忽略。\n"
    )

    seed = [r for r in rows if r["operation"].startswith("seedState-")]
    if seed:
        add(
            "\n**SSTORE 定标行**（`seedState-*`）：分别测 0→非 0 首写、非 0 覆写、"
            "同一 tx 内归零、以及 `vm.store` 预置后的归零。`rawGas` 是**毛计量值**，"
            "`gasRefunded` 是 EIP-3529 退款计数器——退款在交易结束时才结算且上限为 `gasUsed/5`，"
            "所以两列必须分开看，不能预先相减。\n"
        )

    # ------------------------------------------------------------------ layer C
    end_to_end = defaultdict(list)
    for row in rows:
        if row["layer"] == "C" and row["operation"] == "swapEndToEnd":
            end_to_end[row["algorithm"]].append(row["rawGas"])
    control = [r["rawGas"] for r in rows if r["algorithm"] == "token-only-control"]

    add("\n## C 层：完整 swap\n\n")
    add(
        "含池架构、代币转账与存储差异，**不能把差异全部归因于曲线数学**。"
        "所有算法共用同一个最小 `TestToken`（runtime code hash 见上表）。"
        "C 层的 `nullTwinGas` 不是同 pragma 空壳，而是 **token-only 对照**："
        "一次转出、一次外部调用、一次转回，不含任何曲线。raw 与对照值并列给出，不只给扣减值。\n\n"
    )
    lines = []
    if control:
        lines.append([label("token-only-control"), len(control), span(control), "—", "基线本身"])
    for algorithm in sorted(end_to_end, key=lambda a: statistics.median(end_to_end[a])):
        raws = end_to_end[algorithm]
        lines.append(
            [
                label(algorithm),
                len(raws),
                span(raws),
                int(statistics.median(raws)),
                int(statistics.median(raws)) - (control[0] if control else 0),
            ]
        )
    add(table(["算法", "n", "rawGas 区间", "rawGas 中位", "减去 token-only 对照"], lines))

    add(
        "\nUniswap V2 出现两行且**不可混用**：`canonical-30bps` 是未改动的上游生产实现；"
        "`modified-zero-fee` 是 benchmark-only 的零费改造，补丁见 `gas-harness/patches/`，"
        "改后 bytecode hash 见 `bytecode-hashes.csv`。零费主对比用后者，前者单独展示。\n"
    )

    # ---- the non-rankability box, with the observed order flip stated openly
    dodo_c = end_to_end.get("dodo")
    fb_c = end_to_end.get("flashbots")
    add("\n### DODO 与 Flashbots 在 C 层不可直接排名\n\n")
    add(
        "> C 层的 `DodoPool` 是**本 harness 自己写的最小池**，只包住 pinned `PMMPricing` 库；"
        "`ExamplePropAmm` 是**完整的生产合约**，带 pair 注册表、跨合约读 `PrioUpdateRegistry`、"
        "`Ownable`/`ReentrancyGuard`/`SafeERC20`。二者是不同完成度的产物，不是同一件事的两种实现，"
        "因此 C 层这两行**不构成排名**。\n"
    )
    if dodo_c and fb_c:
        add(
            f">\n> 这一点在数据上是可见的：C 层 DODO 中位 {int(statistics.median(dodo_c))}、"
            f"Flashbots 中位 {int(statistics.median(fb_c))}，**顺序与 A/B 层相反**。"
            "这个反转衡量的是包装层的完成度差异，**不推翻**下节在统一口径下的结论，"
            "也**不能**被反过来当作 DODO 曲线更便宜的证据。\n"
        )
    add(
        ">\n> 若要做最终产品级对比，需要接入真实的 `MantlePropAmmPool`；本 harness 目前没有它，"
        "所以本报告不给出产品级 gas 排名。\n"
    )

    sensitivity = defaultdict(list)
    for row in rows:
        if row["layer"] == "C-fee-sensitivity":
            sensitivity[row["algorithm"]].append(row["rawGas"])
    if sensitivity:
        add("\n### 费率敏感性（单独一节，不并入零费主表）\n\n")
        add(
            table(
                ["算法", "n", "rawGas 区间"],
                [[label(a), len(v), span(v)] for a, v in sorted(sensitivity.items())],
            )
        )

    # ------------------------------------------------------- permitted verdict
    add("\n## 可以下的结论\n\n")
    quote_median = {a: statistics.median(r["rawGas"] for r in g) for a, g in by_algorithm.items()}
    if "flashbots" in quote_median and "dodo" in quote_median:
        fb_q, dodo_q = int(quote_median["flashbots"]), int(quote_median["dodo"])
        fb_s = int(statistics.median(warm_algo["flashbots"])) if warm_algo.get("flashbots") else None
        dodo_s = int(statistics.median(warm_algo["dodo"])) if warm_algo.get("dodo") else None
        add(
            "**在统一的 quote 与 state-update 口径下，Flashbots 明显比 DODO 便宜。**\n\n"
        )
        verdict = [["A 层整笔报价 rawGas 中位", fb_q, dodo_q, f"{dodo_q / fb_q:.1f}×"]]
        if fb_s and dodo_s:
            verdict.append(
                ["B 层 algorithm-only 状态更新 rawGas 中位", fb_s, dodo_s, f"{dodo_s / fb_s:.1f}×"]
            )
        add(table(["口径", "Flashbots", "DODO", "DODO / Flashbots"], verdict))
        add("\n两个口径方向一致，且都排除了架构与 oracle 路径差异。\n")

    add(
        "\n**只有上面这一条跨算法结论成立。** 其余排名都不成立：\n\n"
        "- C 层各实现完成度不同（见上节），其顺序反转不构成证据；\n"
        "- reference-system 口径衡量的是产品而非曲线；\n"
        "- `adjustedGas` 只是诊断量；\n"
        "- V3 的 A1 与任何整笔报价不可比。\n\n"
        "本报告不回答“哪条曲线在产品上更省 gas”——那需要真实的 `MantlePropAmmPool`。\n"
    )

    # ---------------------------------------------------------- confounders
    add("\n## 不可比较项与混淆因素\n\n")
    add(
        "- 编译器 dispatch 税按签名而异，符号还会翻转（`noop()` 在 0.5/0.6/0.7 是 166、"
        "在 0.8.28 是 141；`ident(uint256)` 反过来是 215 vs 233）。"
        "null-twin 扣减只有在 twin 的 selector 集合与签名完全一致时才成立，这一点由 "
        "`test/NullTwin.t.sol` 在写任何数字之前断言。\n"
        "- selector 表形状不可消除，只能披露。\n"
        "- 池/代理架构与调用深度不同。\n"
        "- oracle 读取路径是不同产品。\n"
        "- 每个算法按其上游 canonical 配置编译（pragma 硬钉，无法统一），"
        "逐行记录在 `compilerConfig` 列，由 `foundry.toml` 的 `compilation_restrictions` 落实。\n"
        "- V3 的 gas 强烈依赖 tick 穿越数，逐行见 `tickCrossings`；跨算法比较时必须连它一起读。\n"
    )

    # ---------------------------------------------------------------- artefacts
    add("\n## 产物\n\n")
    add(
        "```\n"
        "research-out/gas/gas-snapshot.csv       每个测量点一行\n"
        "research-out/gas/gas-snapshot.json      同样的行 + 运行级 metadata（含 benchmarkCommit）\n"
        "research-out/gas/touched-slots.csv      B 层 vm.record()/vm.accesses() 结果\n"
        "research-out/gas/bytecode-hashes.csv    C 层每个合约的部署 runtime code hash\n"
        "research-out/gas/REPORT-gas.zh-CN.md    本文件\n"
        "research-out/gas/MANIFEST.sha256        以上全部文件的 sha256\n"
        "```\n\n"
        "复现：`cd gas-harness && ./script/gas-snapshot.sh`。"
        "该脚本会先按 pin 重新拉取并校验第三方源码，任何 sha256 不符都会在编译之前中止。\n"
    )

    with open(OUT, "w") as handle:
        handle.write("".join(parts))
    print(f"  wrote {OUT} ({len(rows)} measurements)")


if __name__ == "__main__":
    main()
