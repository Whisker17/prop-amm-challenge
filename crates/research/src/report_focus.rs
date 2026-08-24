//! The two focused comparison reports.
//!
//! * [`dodo_vs_flashbots`] — the head-to-head between the two oracle-aware
//!   curves, on the pairing table, driven entirely by per-seed paired
//!   differences.
//! * [`vs_baselines`] — every strategy against the passive baselines
//!   (zero-fee Uniswap V2, and Uniswap V3 once it is in the catalogue).
//!
//! They live beside [`crate::report`] rather than inside it so that each has one
//! obvious entry point, and so that a change to one cannot disturb the other.
//!
//! Both refuse to state a ranking from marginal means. Where a paired difference
//! is not significant they say so in those words, and neither is permitted to
//! describe a tie on `netEdge` as the two curves sharing a pricing formula —
//! they do not, and the quote matrix in the same output shows where they part.

use std::fmt::Write as _;

use crate::json::{escape, num};
use crate::metrics::StrategySummary;
use crate::paired::{Attribution, PairedDelta, Univ3SanityGate};
use crate::probe::QuoteRow;
use crate::report::{RunMeta, CURVE_REVERTS_NOTE_ZH, PAIRING_CAVEAT_ZH, SCOPE_NOTE_ZH};

fn fixed(value: f64) -> String {
    if value.is_finite() {
        format!("{value:.4}")
    } else {
        "n/a".to_string()
    }
}

fn signed(value: f64) -> String {
    if value.is_finite() {
        format!("{value:+.5}")
    } else {
        "n/a".to_string()
    }
}

fn pct(value: f64) -> String {
    if value.is_finite() {
        format!("{:.2}%", value * 100.0)
    } else {
        "n/a".to_string()
    }
}

fn verdict_zh(delta: &PairedDelta) -> &'static str {
    if !delta.is_significant() {
        "不显著"
    } else if delta.mean > 0.0 {
        "显著为正"
    } else {
        "显著为负"
    }
}

fn find_summary<'a>(summaries: &'a [StrategySummary], id: &str) -> Option<&'a StrategySummary> {
    summaries.iter().find(|s| s.strategy_id == id)
}

// ===================== DODO vs Flashbots =====================

/// `REPORT-dodo-vs-flashbots.zh-CN.md`
pub fn dodo_vs_flashbots(
    meta: &RunMeta,
    summaries: &[StrategySummary],
    flashbots_minus_dodo: &[PairedDelta],
    quote_rows: &[QuoteRow],
) -> String {
    let mut out = String::new();
    out.push_str("# DODO PMM vs Flashbots ExamplePropAmm\n\n");
    out.push_str(
        "两条 oracle-aware 曲线的正面对比。**结论只由逐 seed 配对差给出**，边际均值排名不作为结论。\n\n",
    );

    out.push_str("## 公平性条件\n\n");
    out.push_str("| 条件 | 状态 |\n| --- | --- |\n");
    out.push_str("| DODO `lpFeeRate` | `0` |\n");
    out.push_str("| Flashbots 手续费 | 原生为 0，未加任何 fee wrapper |\n");
    let _ = writeln!(
        out,
        "| 初始资金 | reserveX = {}，reserveY = {}，fairPrice = {} |",
        meta.initial_x, meta.initial_y, meta.initial_price
    );
    out.push_str("| Oracle | 每步量化一次，同一个 `priceWad` 同时发布给 DODO(`i`) 与 Flashbots(`multX`，`multY = 1e18`) |\n");
    out.push_str("| 发布时间 | 零延迟：先发布，后套利，再零售 |\n");
    let _ = writeln!(
        out,
        "| seed | 同一批（起始 {}，步长 {}，共 {} 个），逐 seed 配对 |",
        meta.seed_start, meta.seed_stride, meta.simulations
    );
    out.push_str("| 公式 | 两侧均为锁定 Solidity 的逐行移植，整数运算顺序 / floor / ceil / revert 分支 / 状态转换均未改动 |\n\n");
    let _ = writeln!(
        out,
        "生成结果的 benchmark commit：`{}`{}\n",
        meta.benchmark_commit(),
        if !meta.provenance_match() {
            "（运行时工作区有未提交改动）"
        } else {
            ""
        }
    );

    out.push_str("## 配对差（Flashbots − DODO）\n\n");
    out.push_str(
        "95% CI 为配对差上的正态近似（`均值 ± 1.96 × 标准误`）；paired win rate 为 Flashbots 在多少比例的 seed 上胜出。\n\n",
    );
    for metric in ["netEdge", "retailEdge", "arbitrageEdge"] {
        let _ = writeln!(out, "### {metric}\n");
        out.push_str(
            "| pairing | 均值差 | 95% CI | P5 / P50 / P95 | SE | t | paired win rate | 判定 |\n",
        );
        out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- |\n");
        for delta in flashbots_minus_dodo.iter().filter(|d| d.metric == metric) {
            let _ = writeln!(
                out,
                "| {} | {} | [{}, {}] | {} / {} / {} | {} | {:.2} | {} | {} |",
                delta
                    .pairing_index
                    .map(|i| format!("#{i}"))
                    .unwrap_or_else(|| "-".to_string()),
                signed(delta.mean),
                signed(delta.ci95_low),
                signed(delta.ci95_high),
                signed(delta.distribution.p5),
                signed(delta.distribution.p50),
                signed(delta.distribution.p95),
                fixed(delta.std_error),
                delta.t_stat,
                pct(delta.paired_win_rate),
                verdict_zh(delta)
            );
        }
        out.push('\n');
    }

    let net: Vec<&PairedDelta> = flashbots_minus_dodo
        .iter()
        .filter(|d| d.metric == "netEdge")
        .collect();
    let significant: Vec<&&PairedDelta> = net.iter().filter(|d| d.is_significant()).collect();
    out.push_str("### 判定汇总\n\n");
    let _ = writeln!(
        out,
        "- netEdge 共 {} 个 pairing 行，其中 **{} 行统计显著**，{} 行无法区分高下。",
        net.len(),
        significant.len(),
        net.len() - significant.len()
    );
    for delta in &significant {
        let _ = writeln!(
            out,
            "- 显著行 {}：均值差 {}，95% CI [{}, {}]，t = {:.2}，paired win rate {}。",
            delta
                .pairing_index
                .map(|i| format!("#{i}"))
                .unwrap_or_else(|| "-".to_string()),
            signed(delta.mean),
            signed(delta.ci95_low),
            signed(delta.ci95_high),
            delta.t_stat,
            pct(delta.paired_win_rate)
        );
    }
    out.push_str(
        "\n> netEdge 打平**不等于**两条曲线的报价公式相同。它们是两套不同的公式、不同的状态机、\
不同的整数舍入；本节只说明在这一组参数与这一组订单流下，净边际的差异小到无法与噪声区分。\
下面的报价矩阵展示了它们在不同订单规模上的实际分歧。\n\n",
    );

    out.push_str("## 其它指标（边际值，仅供参考，不用于排名）\n\n");
    out.push_str("| 策略 | 参数 | netEdge>0 占比 | 零售流量份额 | 套利次数 | 套利名义额 | 期末库存偏离 | 最大库存偏离 |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for summary in summaries
        .iter()
        .filter(|s| s.family == "dodo" || s.family == "flashbots")
    {
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} | {} | {} |",
            summary.strategy_id,
            summary.parameter,
            pct(summary.positive_net_rate),
            pct(summary.retail_flow_share.mean),
            fixed(summary.arb_count.mean),
            fixed(summary.arb_notional.mean),
            pct(summary.final_inventory_deviation.mean),
            pct(summary.max_inventory_deviation.mean)
        );
    }

    out.push_str("\n## 不同订单规模的报价差异\n\n");
    out.push_str(
        "在统一初始状态下报价（未成交，不推进状态），以对外部 fair price 的滑点 bps 表示。\
这是两条曲线**形状**差异的直接证据。\n\n",
    );
    for side in ["buy_x", "sell_x"] {
        let _ = writeln!(
            out,
            "### {}",
            if side == "buy_x" {
                "花费 Y 换 X"
            } else {
                "花费 X 换 Y"
            }
        );
        let sizes: Vec<f64> = {
            let mut sizes: Vec<f64> = quote_rows
                .iter()
                .filter(|r| r.side == side)
                .map(|r| r.input)
                .collect();
            sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            sizes.dedup_by(|a, b| (*a - *b).abs() < f64::EPSILON);
            sizes
        };
        out.push_str("\n| 策略 |");
        for size in &sizes {
            let _ = write!(out, " {size} |");
        }
        out.push_str("\n| --- |");
        for _ in &sizes {
            out.push_str(" --- |");
        }
        out.push('\n');
        for summary in summaries
            .iter()
            .filter(|s| s.family == "dodo" || s.family == "flashbots")
        {
            let _ = write!(out, "| `{}` |", summary.strategy_id);
            for size in &sizes {
                let cell = quote_rows.iter().find(|r| {
                    r.strategy_id == summary.strategy_id
                        && r.side == side
                        && (r.input - *size).abs() < f64::EPSILON
                });
                match cell {
                    Some(row) if row.slippage_bps.is_finite() => {
                        let _ = write!(out, " {:.2} |", row.slippage_bps);
                    }
                    _ => out.push_str(" n/a |"),
                }
            }
            out.push('\n');
        }
        out.push('\n');
    }

    let _ = writeln!(out, "> {PAIRING_CAVEAT_ZH}\n");
    out.push_str("## 范围界定\n\n");
    let _ = writeln!(out, "{SCOPE_NOTE_ZH}\n");
    let _ = writeln!(out, "{CURVE_REVERTS_NOTE_ZH}\n");
    out
}

/// `dodo-vs-flashbots.csv`
pub fn dodo_vs_flashbots_csv(flashbots_minus_dodo: &[PairedDelta]) -> String {
    let mut out = String::new();
    out.push_str(
        "pairing_index,metric,treatment,baseline,samples,mean,p5,p50,p95,\
std_error,ci95_low,ci95_high,t_stat,paired_win_rate,significant_95\n",
    );
    for d in flashbots_minus_dodo {
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            d.pairing_index
                .map(|i| i.to_string())
                .unwrap_or_else(|| "".to_string()),
            d.metric,
            d.treatment,
            d.baseline,
            d.samples,
            d.mean,
            d.distribution.p5,
            d.distribution.p50,
            d.distribution.p95,
            d.std_error,
            d.ci95_low,
            d.ci95_high,
            d.t_stat,
            d.paired_win_rate,
            d.is_significant()
        );
    }
    out
}

/// `dodo-vs-flashbots.json`
pub fn dodo_vs_flashbots_json(meta: &RunMeta, flashbots_minus_dodo: &[PairedDelta]) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "{{\n  \"benchmarkCommit\": \"{}\",\n  \"workingTreeDirty\": {},\n  \"simulations\": {},\n  \"steps\": {},\n  \"competitor\": \"{}\",\n",
        escape(meta.benchmark_commit()),
        !meta.provenance_match(),
        meta.simulations,
        meta.steps,
        meta.competitor.as_str()
    );
    out.push_str("  \"comparison\": \"flashbots - dodo, per seed, on the K <-> concentration pairing table\",\n");
    out.push_str("  \"method\": \"paired per-seed differences; 95% CI is a normal approximation on the differences\",\n");
    out.push_str("  \"deltas\": [\n");
    let bodies: Vec<String> = flashbots_minus_dodo
        .iter()
        .map(|d| {
            format!(
                "    {{\"pairingIndex\": {}, \"metric\": \"{}\", \"treatment\": \"{}\", \"baseline\": \"{}\", \"samples\": {}, \"mean\": {}, \"p5\": {}, \"p50\": {}, \"p95\": {}, \"stdError\": {}, \"ci95\": [{}, {}], \"tStat\": {}, \"pairedWinRate\": {}, \"significant95\": {}}}",
                d.pairing_index
                    .map(|i| i.to_string())
                    .unwrap_or_else(|| "null".to_string()),
                d.metric,
                escape(&d.treatment),
                escape(&d.baseline),
                d.samples,
                num(d.mean),
                num(d.distribution.p5),
                num(d.distribution.p50),
                num(d.distribution.p95),
                num(d.std_error),
                num(d.ci95_low),
                num(d.ci95_high),
                num(d.t_stat),
                num(d.paired_win_rate),
                d.is_significant()
            )
        })
        .collect();
    out.push_str(&bodies.join(",\n"));
    out.push_str("\n  ]\n}\n");
    out
}

// ===================== versus the passive baselines =====================

/// `REPORT-vs-baselines.zh-CN.md`
#[allow(clippy::too_many_arguments)]
pub fn vs_baselines(
    meta: &RunMeta,
    summaries: &[StrategySummary],
    versus_univ2: &[PairedDelta],
    versus_univ3: &[PairedDelta],
    univ3_minus_univ2: &[PairedDelta],
    sanity: Option<&Univ3SanityGate>,
    attributions: &[Attribution],
) -> String {
    let mut out = String::new();
    out.push_str("# 相对被动基准（Uniswap V2 / V3）的对比\n\n");

    out.push_str("## 必须先读的口径说明\n\n");
    out.push_str(
        "- DODO 与 Flashbots 使用**同一个零延迟 oracle**：每步先量化发布价格，再套利、再零售。\n",
    );
    out.push_str("- Uniswap V2 与 Uniswap V3 是**被动 AMM，不消费任何外部 oracle**。这是曲线本身的性质，不是本实验的配置选择。\n");
    out.push_str("- 因此「相对 Uni 的差异」**同时包含 oracle 重定价优势与曲率差异**，不能整体归因于曲率。下文的拆分一节给出两者的量级。\n");
    out.push_str("- 下表中的多个显著结果**不是多个独立实验**：它们共用同一批 seed、同一条价格路径、同一份订单流，并且共用同一个 baseline，因此彼此高度相关。不要把「N 个结果显著」当作 N 次独立验证。\n\n");

    let _ = writeln!(
        out,
        "生成结果的 benchmark commit：`{}`{}，模拟 {} 次 × {} 步。\n",
        meta.benchmark_commit(),
        if !meta.provenance_match() {
            "（工作区不干净）"
        } else {
            ""
        },
        meta.simulations,
        meta.steps
    );

    for (title, deltas) in [
        ("## 相对零手续费 Uniswap V2 的配对差", versus_univ2),
        (
            "## 相对零手续费 full-range Uniswap V3 的配对差",
            versus_univ3,
        ),
    ] {
        if deltas.is_empty() {
            continue;
        }
        let _ = writeln!(out, "{title}\n");
        out.push_str("| 策略 | 指标 | 均值差 | 95% CI | t | paired win rate | 判定 |\n");
        out.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
        for delta in deltas.iter().filter(|d| d.metric == "netEdge") {
            let _ = writeln!(
                out,
                "| `{}` | {} | {} | [{}, {}] | {:.2} | {} | {} |",
                delta.treatment,
                delta.metric,
                signed(delta.mean),
                signed(delta.ci95_low),
                signed(delta.ci95_high),
                delta.t_stat,
                pct(delta.paired_win_rate),
                verdict_zh(delta)
            );
        }
        out.push('\n');
    }

    if let Some(gate) = sanity {
        out.push_str("## Sanity check：full-range V3 vs UniV2\n\n");
        out.push_str(
            "两者都是被动、零手续费、同等期初资本，因此逐 seed 的 netEdge 差**应当很小**——\
但**不是 0，本节也不断言它是 0**。已实测的两项真实残差：同一状态下单笔报价最大差 **1693 wei**；\
允许各自独立演化后，400 笔内状态漂移最大 **484 wei**（见 `univ3_vs_univ2_continuous`）。\n\n",
        );

        out.push_str("### 硬性 gate（判定只由这三项决定）\n\n");
        out.push_str(
            "只有可以从构造本身证明的性质才进入判定：\n\n\
* full-range 仓位覆盖整个 tick 域，**不可能**因区间容量不足而拒单；\n\
* canonical capacity probe **不得**走到 revert 分支，否则该订单的容量根本没被测到；\n\
* 移植的报价函数**不得**走到 revert 分支。\n\n",
        );
        let _ = writeln!(
            out,
            "| gate | 值 | 要求 |\n| --- | --- | --- |\n\
| 判定 | **{}** | — |\n\
| full-range 容量不足订单数 | {} | = 0 |\n\
| capacity probe revert 数 | {} | = 0 |\n\
| curve revert 数 | {} | = 0 |\n",
            if gate.passed() { "PASS" } else { "FAIL" },
            gate.full_range_capacity_limited_orders,
            gate.capacity_probe_reverts,
            gate.curve_reverts
        );

        out.push_str("\n### 残差诊断（**不是上界，不进入判定**）\n\n");
        out.push_str(
            "> **下面的 reference scale 是启发式量纲参考，不是数学上界。**\
残差低于它**本身不证明任何东西**。三处构造假设未经证明，其中至少两处在一般情况下不成立：\n\n\
1. `δ²` 形式假设两个搜索都停在光滑目标的**内部驻点**。实际目标既不光滑也非无约束——\
它按 nano 向下取整，是阶梯函数；最优点还经常落在边界上（`α = 0` 或 `1`，\
或套利量被 `MIN_INPUT` 夹住）。边界最优处一阶项不消失，误差是 `O(δ)` 而非 `O(δ²)`。\n\
2. `arb_notional × δ²` 的系数是**断言的，不是推导的**；没有任何论证说明套利目标的曲率\
被自身名义额界住。\n\
3. 量化项用的是 `final_fair_price`（收盘价），**不是**交易实际发生价格的上界；\
一条收在低位的路径会低估它。\n\n\
此外它完全忽略误差传播：两个池一旦分岔就各自独立演化，\
逐笔上界求和不构成对复利轨迹的上界。\n\n\
列出它只是为了让读者不必心算运行规模，就能看出残差是 1e-2 还是 1e2。\
诊断的**判别力**由 `tests/report_artefacts.rs` 里的 mutation 测试给出：\
把报价刻意压低 1 bp，残差会比真实曲线高出若干数量级。\n\n",
        );
        let _ = writeln!(
            out,
            "| 项 | 值 |\n| --- | --- |\n\
| 配对 seed 数 | {} |\n\
| netEdge 残差 均值 | {} |\n\
| 残差 P5 / P50 / P95 | {} / {} / {} |\n\
| 残差 min / max | {} / {} |\n\
| max \\|残差\\| | {:.6e} |\n\
| reference scale（合计，非上界） | {:.6e} |\n\
| ├ 量化项 | {:.6e} |\n\
| ├ 套利搜索项 | {:.6e} |\n\
| └ 路由拆分搜索项 | {:.6e} |\n\
| 残差高于 reference scale 的 seed 数 | {} |\n",
            gate.samples,
            signed(gate.residuals.mean),
            signed(gate.residuals.p5),
            signed(gate.residuals.p50),
            signed(gate.residuals.p95),
            signed(gate.residuals.min),
            signed(gate.residuals.max),
            gate.max_abs_residual,
            gate.reference_scale,
            gate.scale_terms.quantisation,
            gate.scale_terms.arb_search,
            gate.scale_terms.router_search,
            gate.seeds_over_reference_scale
        );
        out.push_str(
            "\n注意 reference scale 的**主项是搜索误差，不是量化误差**——\
第 2、3 项与曲线本身无关，是模拟器搜索精度的性质。\n\n",
        );

        if !univ3_minus_univ2.is_empty() {
            out.push_str("逐指标配对差：\n\n");
            out.push_str("| 指标 | 均值差 | 95% CI | t | paired win rate | 判定 |\n");
            out.push_str("| --- | --- | --- | --- | --- | --- |\n");
            for delta in univ3_minus_univ2 {
                let _ = writeln!(
                    out,
                    "| {} | {} | [{}, {}] | {:.2} | {} | {} |",
                    delta.metric,
                    signed(delta.mean),
                    signed(delta.ci95_low),
                    signed(delta.ci95_high),
                    delta.t_stat,
                    pct(delta.paired_win_rate),
                    verdict_zh(delta)
                );
            }
            out.push('\n');
        }
        out.push_str(
            "> 本节只能声明：**三项硬性 gate 通过，且残差量级已如实报告**。它**不是**\
「V3 与 V2 全局逐 wei 等价」的证明，也**不是**「残差不超过某个推导上界」的证明——\
当前 reference scale 不具备上界资格。两者是不同的实现，V3 的有限 tick 区间会在端点处\
留下结构性偏移。期初状态下的逐笔报价对比见同一目录的 `quote-matrix.csv`。\n\n",
        );
    }

    if !attributions.is_empty() {
        out.push_str("## 优势拆分：oracle 系统性优势 vs 曲线形状差异\n\n");
        out.push_str(
            "| 策略 | 指标 | oracle 系统性优势 | 曲线形状差异 | 合计（vs UniV2） | 锚点 |\n",
        );
        out.push_str("| --- | --- | --- | --- | --- | --- |\n");
        for attribution in attributions.iter().filter(|a| a.metric == "netEdge") {
            let _ = writeln!(
                out,
                "| `{}` | {} | {} | {} | {} | `{}` |",
                attribution.strategy,
                attribution.metric,
                signed(attribution.oracle_system_advantage),
                signed(attribution.curve_shape_effect),
                signed(attribution.total_vs_univ2),
                attribution.anchor
            );
        }
        out.push_str(
            "\n拆分锚定在期初库存，只有均值可拆，分位数不可拆。它不表示 oracle-aware 曲线在其它点上等于恒定乘积。\n\n",
        );
    }

    out.push_str("## 被动基准自身的表现\n\n");
    out.push_str("| 策略 | netEdge 均值 | P5 / P50 / P95 | netEdge>0 占比 | 最大库存偏离 |\n");
    out.push_str("| --- | --- | --- | --- | --- |\n");
    for id in [crate::paired::UNIV2_ID, crate::paired::UNIV3_FULL_RANGE_ID] {
        if let Some(summary) = find_summary(summaries, id) {
            let _ = writeln!(
                out,
                "| `{}` | {} | {} / {} / {} | {} | {} |",
                summary.strategy_id,
                fixed(summary.net_edge.mean),
                fixed(summary.net_edge.p5),
                fixed(summary.net_edge.p50),
                fixed(summary.net_edge.p95),
                pct(summary.positive_net_rate),
                pct(summary.max_inventory_deviation.mean)
            );
        }
    }

    out.push('\n');
    let _ = writeln!(out, "{SCOPE_NOTE_ZH}\n");
    let _ = writeln!(out, "{CURVE_REVERTS_NOTE_ZH}\n");
    out
}

/// `versus-baselines.csv`
pub fn vs_baselines_csv(deltas: &[PairedDelta]) -> String {
    let mut out = String::new();
    out.push_str(
        "treatment,baseline,metric,samples,mean,p5,p50,p95,std_error,\
ci95_low,ci95_high,t_stat,paired_win_rate,significant_95\n",
    );
    for d in deltas {
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            d.treatment,
            d.baseline,
            d.metric,
            d.samples,
            d.mean,
            d.distribution.p5,
            d.distribution.p50,
            d.distribution.p95,
            d.std_error,
            d.ci95_low,
            d.ci95_high,
            d.t_stat,
            d.paired_win_rate,
            d.is_significant()
        );
    }
    out
}

/// `versus-baselines.json`
pub fn vs_baselines_json(
    meta: &RunMeta,
    deltas: &[PairedDelta],
    attributions: &[Attribution],
) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "{{\n  \"benchmarkCommit\": \"{}\",\n  \"workingTreeDirty\": {},\n  \"simulations\": {},\n  \"steps\": {},\n",
        escape(meta.benchmark_commit()),
        !meta.provenance_match(),
        meta.simulations,
        meta.steps
    );
    out.push_str("  \"oracleNote\": \"DODO and Flashbots consume the same zero-latency oracle; Uniswap V2 and V3 are passive and consume none, so a difference against them contains the oracle repricing advantage as well as curve shape\",\n");
    out.push_str("  \"independenceNote\": \"these results share one seed set, one price path, one order flow and one baseline; significant rows are correlated, not independent experiments\",\n");
    out.push_str("  \"deltas\": [\n");
    let bodies: Vec<String> = deltas
        .iter()
        .map(|d| {
            format!(
                "    {{\"treatment\": \"{}\", \"baseline\": \"{}\", \"metric\": \"{}\", \"samples\": {}, \"mean\": {}, \"ci95\": [{}, {}], \"tStat\": {}, \"pairedWinRate\": {}, \"significant95\": {}}}",
                escape(&d.treatment),
                escape(&d.baseline),
                d.metric,
                d.samples,
                num(d.mean),
                num(d.ci95_low),
                num(d.ci95_high),
                num(d.t_stat),
                num(d.paired_win_rate),
                d.is_significant()
            )
        })
        .collect();
    out.push_str(&bodies.join(",\n"));
    out.push_str("\n  ],\n  \"attribution\": [\n");
    let attribution_bodies: Vec<String> = attributions
        .iter()
        .map(|a| {
            format!(
                "    {{\"strategy\": \"{}\", \"metric\": \"{}\", \"anchor\": \"{}\", \"oracleSystemAdvantage\": {}, \"curveShapeEffect\": {}, \"totalVsUniV2\": {}}}",
                escape(&a.strategy),
                a.metric,
                escape(&a.anchor),
                num(a.oracle_system_advantage),
                num(a.curve_shape_effect),
                num(a.total_vs_univ2)
            )
        })
        .collect();
    out.push_str(&attribution_bodies.join(",\n"));
    out.push_str("\n  ]\n}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::experiment::Competitor;
    use crate::json::Json;
    use crate::metrics::{Distribution, RunMetrics};

    fn meta() -> RunMeta {
        RunMeta {
            simulations: 4,
            steps: 100,
            seed_start: 0,
            seed_stride: 1,
            competitor: Competitor::Normalizer,
            workers: 2,
            initial_price: 100.0,
            initial_x: 100.0,
            initial_y: 10_000.0,
            elapsed_seconds: 1.0,
            provenance: crate::provenance::Provenance {
                binary_commit: "a".repeat(40),
                binary_dirty: false,
                run_start: crate::provenance::GitState {
                    commit: "a".repeat(40),
                    dirty: false,
                },
                run_end: crate::provenance::GitState {
                    commit: "a".repeat(40),
                    dirty: false,
                },
            },
            strategy_set: "legacy".to_string(),
        }
    }

    fn delta(metric: &'static str, mean: f64, significant: bool) -> PairedDelta {
        let se = if significant {
            mean.abs() / 10.0
        } else {
            mean.abs() * 10.0 + 1.0
        };
        PairedDelta {
            treatment: "flashbots-c1".into(),
            baseline: "dodo-k1".into(),
            pairing_index: Some(0),
            metric,
            samples: 1_000,
            mean,
            distribution: Distribution::from_samples(&[-1.0, 0.0, 1.0]),
            std_error: se,
            ci95_low: mean - 1.96 * se,
            ci95_high: mean + 1.96 * se,
            t_stat: mean / se,
            paired_win_rate: 0.5,
        }
    }

    fn summary() -> StrategySummary {
        let runs: Vec<RunMetrics> = (0..4)
            .map(|seed| RunMetrics {
                strategy_id: "dodo-k1".into(),
                seed,
                steps: 100,
                retail_edge: 1.0,
                arbitrage_edge: -0.5,
                arbitrage_loss: 0.5,
                net_edge: 0.5,
                retail_notional: 10.0,
                competitor_retail_notional: 10.0,
                retail_flow_share: 0.5,
                retail_trade_count: 1,
                arb_count: 1,
                arb_notional: 1.0,
                final_inventory_deviation: 0.01,
                max_inventory_deviation: 0.02,
                final_reserve_x: 100.0,
                final_reserve_y: 10_000.0,
                final_fair_price: 100.0,
                curve_revert_count: 0,
                univ3: None,
            })
            .collect();
        StrategySummary::from_runs("dodo-k1", "dodo", "K=1e18", &runs)
    }

    #[test]
    fn head_to_head_report_refuses_to_call_a_tie_a_formula_match() {
        let deltas = vec![delta("netEdge", 0.001, false)];
        let markdown = dodo_vs_flashbots(&meta(), &[summary()], &deltas, &[]);
        assert!(markdown.contains("不显著"));
        assert!(markdown.contains("netEdge 打平**不等于**两条曲线的报价公式相同"));
        assert!(markdown.contains("0 行统计显著") || markdown.contains("**0 行统计显著**"));
    }

    #[test]
    fn head_to_head_report_names_significant_rows() {
        let deltas = vec![delta("netEdge", 1.0, true)];
        let markdown = dodo_vs_flashbots(&meta(), &[summary()], &deltas, &[]);
        assert!(markdown.contains("显著为正"));
        assert!(markdown.contains("显著行"));
    }

    #[test]
    fn baselines_report_states_the_oracle_asymmetry_and_non_independence() {
        let deltas = vec![delta("netEdge", 5.0, true)];
        let markdown = vs_baselines(&meta(), &[summary()], &deltas, &[], &[], None, &[]);
        assert!(markdown.contains("不消费任何外部 oracle"));
        assert!(markdown.contains("同时包含 oracle 重定价优势与曲率差异"));
        assert!(markdown.contains("不是多个独立实验"));
    }

    #[test]
    fn sanity_row_is_reported_without_claiming_global_equivalence() {
        let residual = delta("netEdge", 0.0, false);
        let gate = crate::paired::Univ3SanityGate {
            residuals: crate::metrics::Distribution::from_samples(&[1e-7, -2e-7, 3e-8]),
            samples: 3,
            max_abs_residual: 3e-7,
            reference_scale: 1e-5,
            scale_terms: crate::paired::ScaleTerms {
                quantisation: 1e-7,
                arb_search: 9e-6,
                router_search: 9e-7,
            },
            seeds_over_reference_scale: 0,
            full_range_capacity_limited_orders: 0,
            capacity_probe_reverts: 0,
            curve_reverts: 0,
        };
        let markdown = vs_baselines(
            &meta(),
            &[summary()],
            &[],
            &[],
            std::slice::from_ref(&residual),
            Some(&gate),
            &[],
        );
        assert!(markdown.contains("Sanity check"));
        assert!(markdown.contains("PASS"));
        // The tolerance must be shown as derived, not asserted as exact equality.
        assert!(markdown.contains("不是数学上界"));
        assert!(!markdown.contains("期望值是 0"));
        assert!(
            !markdown.contains("全局逐 wei 等价\n"),
            "the report must not assert global equivalence"
        );
        assert!(
            markdown.contains("全局逐 wei 等价"),
            "the disclaimer must still be present"
        );
    }

    #[test]
    fn csv_and_json_round_trip() {
        let deltas = vec![delta("netEdge", 0.5, true), delta("retailEdge", 2.0, true)];
        assert_eq!(dodo_vs_flashbots_csv(&deltas).lines().count(), 3);
        assert_eq!(vs_baselines_csv(&deltas).lines().count(), 3);
        let json = Json::parse(&dodo_vs_flashbots_json(&meta(), &deltas)).expect("valid JSON");
        assert_eq!(json.get("deltas").unwrap().as_array().unwrap().len(), 2);
        let json = Json::parse(&vs_baselines_json(&meta(), &deltas, &[])).expect("valid JSON");
        assert!(json
            .get("oracleNote")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("passive"));
    }
}
