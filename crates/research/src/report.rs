//! Machine-readable (JSON + CSV) and human-readable (Chinese Markdown) output.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::experiment::{BatchConfig, Competitor};
use crate::json::{escape, num};
use crate::metrics::{Distribution, RunMetrics, StrategySummary};
use crate::paired::{self, Attribution, PairedDelta};
use crate::probe::QuoteRow;
use crate::u256::U256;

/// Provenance and configuration recorded alongside every result set.
#[derive(Debug, Clone)]
pub struct RunMeta {
    pub simulations: u32,
    pub steps: u32,
    pub seed_start: u64,
    pub seed_stride: u64,
    pub competitor: Competitor,
    pub workers: usize,
    pub initial_price: f64,
    pub initial_x: f64,
    pub initial_y: f64,
    pub elapsed_seconds: f64,
    /// Commit of *this* repository that produced the result set.
    pub benchmark_commit: String,
    /// Whether the working tree had uncommitted changes at run time.
    pub benchmark_dirty: bool,
}

impl RunMeta {
    pub fn from_batch(batch: &BatchConfig, elapsed_seconds: f64) -> RunMeta {
        let (benchmark_commit, benchmark_dirty) = git_metadata();
        RunMeta {
            benchmark_commit,
            benchmark_dirty,
            simulations: batch.simulations,
            steps: batch.steps,
            seed_start: batch.seed_start,
            seed_stride: batch.seed_stride,
            competitor: batch.competitor,
            // Record the resolved worker count; `0` means "auto" on the CLI.
            workers: if batch.workers == 0 {
                rayon::current_num_threads()
            } else {
                batch.workers
            },
            initial_price: prop_amm_shared::config::INITIAL_PRICE,
            initial_x: prop_amm_shared::config::INITIAL_X,
            initial_y: prop_amm_shared::config::INITIAL_Y,
            elapsed_seconds,
        }
    }
}

pub const DODO_COMMIT: &str = "8da3ee1ec50966fca9a2c80d424040c45c0f785e";
pub const FLASHBOTS_COMMIT: &str = "da53117870c7bec96d71caebe1b3f94370aba3d6";

/// Mantle reference repository, read-only, from which the DODO vendor copies,
/// the golden vectors and the state-persistence rule were taken.
pub const MANTLE_COMMIT: &str = "07f6797";
pub const MANTLE_BRANCH: &str = "feature/v2-dodo-curve";
/// `src/MantlePropAmmPool.sol` — the source of the target / `RState`
/// persistence rule. Last modified there by commit `5e4071b`.
pub const MANTLE_POOL_SHA256: &str =
    "28bd3d2fa70481ffa0ab61b2d0ff1a7914406fdeeb5814643eb5d8b12e9530c4";
/// `src/MantlePropAmmTypes.sol` — the canonical `RState` enum.
pub const MANTLE_TYPES_SHA256: &str =
    "bee141418f90a91e3f76f291468e47ba39d7865be27fb4ff172fe308149517c8";
/// Mantle design document.
pub const MANTLE_DESIGN_DOC: &str = "docs/[TD] mantle PropAmm 合约设计.md";
pub const MANTLE_DESIGN_DOC_SHA256: &str =
    "c395caa6eb5c68595274bf1c1af29882c23c1228fdf7a2dda019961fb7e52b3a";
/// Vendored DODO sources as read (sha256 of the Mantle vendor copies).
pub const MANTLE_VENDOR_DECIMAL_MATH_SHA256: &str =
    "27d9d19a79982c79bd9faa5ea2c2039be319b256acc898de282179d7bf256352";
pub const MANTLE_VENDOR_DODO_MATH_SHA256: &str =
    "90f688a26a7c6ad63b7f84b1c04cd61609540e14269f26810ad2cd80004c448a";
pub const MANTLE_VENDOR_PMM_PRICING_SHA256: &str =
    "093b9adab96a57230d240984860c23580a2407e6085a3e57948d820ffe50807e";

pub const PAIRING_CAVEAT_ZH: &str = "K ≈ 1 / concentration 仅用于匹配平衡点附近的局部曲率；它不修改、不替换任何原始公式，也不表示两条全局曲线等价。";

/// What this benchmark does and does not model.
pub const SCOPE_NOTE_ZH: &str = "本 benchmark 只对比曲线报价本身（curve-only）。池级别的风控与准入不在范围内，也未被实现：Flashbots 的 targetY emergency lock（`_isTargetYLocked`，仅作用于 swap 路径，upstream 的 quoteXtoY / quoteYtoX 本身也不经过它）、Mantle 的 notional / reserve / inventory deviation 上限、以及余额与授权检查，全部不参与。";

pub const CURVE_REVERTS_NOTE_ZH: &str = "`curve_reverts` 只统计被移植的**报价函数**在非退化输入下走到 revert 分支（溢出 / 下溢 / 除零）的次数。它为 0 表示定价数学在整个运行中没有触发 revert，**不等于**一笔完整的链上 swap 会成功——池级别的风控、余额与授权检查不在本 benchmark 范围内。";

/// `(commit, dirty)` of the benchmark repository, so a result set can be traced
/// to the code that produced it. Falls back to `"unknown"` outside a checkout.
pub fn git_metadata() -> (String, bool) {
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|commit| !commit.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| !String::from_utf8_lossy(&output.stdout).trim().is_empty())
        .unwrap_or(false);
    (commit, dirty)
}

fn distribution_json(name: &str, d: &Distribution) -> String {
    format!(
        "\"{name}\": {{\"mean\": {}, \"p5\": {}, \"p50\": {}, \"p95\": {}, \"min\": {}, \"max\": {}}}",
        num(d.mean),
        num(d.p5),
        num(d.p50),
        num(d.p95),
        num(d.min),
        num(d.max)
    )
}

fn summary_json(summary: &StrategySummary) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "{{\"strategy\": \"{}\", \"family\": \"{}\", \"parameter\": \"{}\", \"simulations\": {}, \"steps\": {}, \"positiveNetRate\": {}, \"totalNetEdge\": {}, \"totalCurveReverts\": {}, ",
        escape(&summary.strategy_id),
        escape(&summary.family),
        escape(&summary.parameter),
        summary.simulations,
        summary.steps,
        num(summary.positive_net_rate),
        num(summary.total_net_edge),
        summary.total_curve_reverts
    );
    let fields = [
        ("netEdge", &summary.net_edge),
        ("retailEdge", &summary.retail_edge),
        ("arbitrageEdge", &summary.arbitrage_edge),
        ("arbitrageLoss", &summary.arbitrage_loss),
        ("retailFlowShare", &summary.retail_flow_share),
        ("retailNotional", &summary.retail_notional),
        ("arbCount", &summary.arb_count),
        ("arbNotional", &summary.arb_notional),
        (
            "finalInventoryDeviation",
            &summary.final_inventory_deviation,
        ),
        ("maxInventoryDeviation", &summary.max_inventory_deviation),
    ];
    let rendered: Vec<String> = fields
        .iter()
        .map(|(name, d)| distribution_json(name, d))
        .collect();
    out.push_str(&rendered.join(", "));
    out.push('}');
    out
}

pub fn summary_json_document(meta: &RunMeta, summaries: &[StrategySummary]) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    let _ = write!(
        out,
        "  \"provenance\": {{\n    \"benchmarkCommit\": \"{}\",\n    \"benchmarkWorkingTreeDirty\": {},\n    \"dodoUpstream\": {{\"repository\": \"https://github.com/DODOEX/contractV2\", \"commit\": \"{DODO_COMMIT}\"}},\n    \"flashbotsUpstream\": {{\"repository\": \"https://github.com/flashbots/priority-update-registry\", \"commit\": \"{FLASHBOTS_COMMIT}\"}},\n    \"univ2\": \"UniswapV2Library.getAmountOut with the fee numerator set to 1000/1000 (zero fee)\",\n    \"mantleReference\": {{\"repository\": \"mantle-propamm-contracts\", \"branch\": \"{MANTLE_BRANCH}\", \"commit\": \"{MANTLE_COMMIT}\", \"readOnly\": true, \"poolSha256\": \"{MANTLE_POOL_SHA256}\", \"typesSha256\": \"{MANTLE_TYPES_SHA256}\", \"designDoc\": \"{}\", \"designDocSha256\": \"{MANTLE_DESIGN_DOC_SHA256}\", \"vendorDecimalMathSha256\": \"{MANTLE_VENDOR_DECIMAL_MATH_SHA256}\", \"vendorDodoMathSha256\": \"{MANTLE_VENDOR_DODO_MATH_SHA256}\", \"vendorPmmPricingSha256\": \"{MANTLE_VENDOR_PMM_PRICING_SHA256}\"}},\n    \"pairingCaveat\": \"{}\",\n    \"scopeNote\": \"{}\",\n    \"curveRevertsNote\": \"{}\"\n  }},\n",
        escape(&meta.benchmark_commit),
        meta.benchmark_dirty,
        escape(MANTLE_DESIGN_DOC),
        escape(PAIRING_CAVEAT_ZH),
        escape(SCOPE_NOTE_ZH),
        escape(CURVE_REVERTS_NOTE_ZH)
    );
    let _ = writeln!(
        out,
        "  \"config\": {{\"simulations\": {}, \"steps\": {}, \"seedStart\": {}, \"seedStride\": {}, \"competitor\": \"{}\", \"workers\": {}, \"initialPrice\": {}, \"initialX\": {}, \"initialY\": {}, \"lpFeeRate\": 0, \"elapsedSeconds\": {}}},",
        meta.simulations,
        meta.steps,
        meta.seed_start,
        meta.seed_stride,
        meta.competitor.as_str(),
        meta.workers,
        num(meta.initial_price),
        num(meta.initial_x),
        num(meta.initial_y),
        num(meta.elapsed_seconds)
    );
    out.push_str("  \"strategies\": [\n");
    let bodies: Vec<String> = summaries
        .iter()
        .map(|s| format!("    {}", summary_json(s)))
        .collect();
    out.push_str(&bodies.join(",\n"));
    out.push_str("\n  ]\n}\n");
    out
}

pub fn summary_csv(summaries: &[StrategySummary]) -> String {
    let mut out = String::new();
    out.push_str(
        "strategy,family,parameter,simulations,steps,positive_net_rate,total_net_edge,\
net_edge_mean,net_edge_p5,net_edge_p50,net_edge_p95,\
retail_edge_mean,arbitrage_edge_mean,arbitrage_loss_mean,\
retail_flow_share_mean,retail_notional_mean,arb_count_mean,arb_notional_mean,\
final_inventory_deviation_mean,max_inventory_deviation_mean,max_inventory_deviation_p95,curve_reverts\n",
    );
    for s in summaries {
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            s.strategy_id,
            s.family,
            s.parameter,
            s.simulations,
            s.steps,
            s.positive_net_rate,
            s.total_net_edge,
            s.net_edge.mean,
            s.net_edge.p5,
            s.net_edge.p50,
            s.net_edge.p95,
            s.retail_edge.mean,
            s.arbitrage_edge.mean,
            s.arbitrage_loss.mean,
            s.retail_flow_share.mean,
            s.retail_notional.mean,
            s.arb_count.mean,
            s.arb_notional.mean,
            s.final_inventory_deviation.mean,
            s.max_inventory_deviation.mean,
            s.max_inventory_deviation.p95,
            s.total_curve_reverts
        );
    }
    out
}

pub fn runs_csv(runs: &[RunMetrics]) -> String {
    let mut out = String::new();
    out.push_str(
        "strategy,seed,steps,retail_edge,arbitrage_edge,arbitrage_loss,net_edge,\
retail_notional,competitor_retail_notional,retail_flow_share,retail_trade_count,\
arb_count,arb_notional,final_inventory_deviation,max_inventory_deviation,\
final_reserve_x,final_reserve_y,final_fair_price,curve_revert_count\n",
    );
    for r in runs {
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            r.strategy_id,
            r.seed,
            r.steps,
            r.retail_edge,
            r.arbitrage_edge,
            r.arbitrage_loss,
            r.net_edge,
            r.retail_notional,
            r.competitor_retail_notional,
            r.retail_flow_share,
            r.retail_trade_count,
            r.arb_count,
            r.arb_notional,
            r.final_inventory_deviation,
            r.max_inventory_deviation,
            r.final_reserve_x,
            r.final_reserve_y,
            r.final_fair_price,
            r.curve_revert_count
        );
    }
    out
}

pub fn paired_deltas_csv(deltas: &[PairedDelta]) -> String {
    let mut out = String::new();
    out.push_str(
        "treatment,baseline,pairing_index,metric,samples,mean,p5,p50,p95,\
std_error,ci95_low,ci95_high,t_stat,paired_win_rate,significant_95\n",
    );
    for d in deltas {
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            d.treatment,
            d.baseline,
            d.pairing_index
                .map(|i| i.to_string())
                .unwrap_or_else(|| "".to_string()),
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

pub fn attribution_csv(attributions: &[Attribution]) -> String {
    let mut out = String::new();
    out.push_str(
        "strategy,family,parameter,anchor,metric,\
oracle_system_advantage,curve_shape_effect,total_vs_univ2\n",
    );
    for a in attributions {
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{}",
            a.strategy,
            a.family,
            a.parameter,
            a.anchor,
            a.metric,
            a.oracle_system_advantage,
            a.curve_shape_effect,
            a.total_vs_univ2
        );
    }
    out
}

fn paired_delta_json(d: &PairedDelta) -> String {
    format!(
        "{{\"treatment\": \"{}\", \"baseline\": \"{}\", \"pairingIndex\": {}, \"metric\": \"{}\", \"samples\": {}, \"mean\": {}, \"p5\": {}, \"p50\": {}, \"p95\": {}, \"stdError\": {}, \"ci95\": [{}, {}], \"tStat\": {}, \"pairedWinRate\": {}, \"significant95\": {}}}",
        escape(&d.treatment),
        escape(&d.baseline),
        d.pairing_index
            .map(|i| i.to_string())
            .unwrap_or_else(|| "null".to_string()),
        d.metric,
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
}

/// Paired statistics document: `Flashbots - DODO` per pairing row, every
/// strategy against zero-fee Uniswap V2, and the oracle / curve-shape split.
pub fn paired_json_document(
    meta: &RunMeta,
    flashbots_minus_dodo: &[PairedDelta],
    versus_univ2: &[PairedDelta],
    attributions: &[Attribution],
) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    let _ = write!(
        out,
        "  \"benchmarkCommit\": \"{}\",\n  \"competitor\": \"{}\",\n  \"simulations\": {},\n  \"steps\": {},\n",
        escape(&meta.benchmark_commit),
        meta.competitor.as_str(),
        meta.simulations,
        meta.steps
    );
    out.push_str("  \"method\": \"per-seed paired differences; CI is a normal approximation (mean +/- 1.96 * SE) on the differences\",\n");
    let _ = writeln!(
        out,
        "  \"attributionAnchors\": {{\"dodo\": \"{}\", \"flashbots\": \"{}\", \"baseline\": \"{}\"}},",
        paired::DODO_ANCHOR_ID,
        paired::FLASHBOTS_ANCHOR_ID,
        paired::UNIV2_ID
    );

    let render = |deltas: &[PairedDelta]| -> String {
        deltas
            .iter()
            .map(|d| format!("    {}", paired_delta_json(d)))
            .collect::<Vec<_>>()
            .join(",\n")
    };
    let _ = write!(
        out,
        "  \"flashbotsMinusDodo\": [\n{}\n  ],\n",
        render(flashbots_minus_dodo)
    );
    let _ = write!(
        out,
        "  \"versusUniV2\": [\n{}\n  ],\n",
        render(versus_univ2)
    );

    let attribution_bodies: Vec<String> = attributions
        .iter()
        .map(|a| {
            format!(
                "    {{\"strategy\": \"{}\", \"family\": \"{}\", \"parameter\": \"{}\", \"anchor\": \"{}\", \"metric\": \"{}\", \"oracleSystemAdvantage\": {}, \"curveShapeEffect\": {}, \"totalVsUniV2\": {}}}",
                escape(&a.strategy),
                escape(&a.family),
                escape(&a.parameter),
                escape(&a.anchor),
                a.metric,
                num(a.oracle_system_advantage),
                num(a.curve_shape_effect),
                num(a.total_vs_univ2)
            )
        })
        .collect();
    let _ = write!(
        out,
        "  \"attribution\": [\n{}\n  ]\n}}\n",
        attribution_bodies.join(",\n")
    );
    out
}

pub fn quote_matrix_csv(rows: &[QuoteRow]) -> String {
    let mut out = String::new();
    out.push_str("strategy,family,parameter,side,input,output,average_price,slippage_bps\n");
    for row in rows {
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{}",
            row.strategy_id,
            row.family,
            row.parameter,
            row.side,
            row.input,
            row.output,
            row.average_price,
            row.slippage_bps
        );
    }
    out
}

pub fn quote_matrix_json(rows: &[QuoteRow], price: f64, reserve_x: f64, reserve_y: f64) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "{{\n  \"fairPrice\": {}, \"reserveX\": {}, \"reserveY\": {}, \"count\": {},\n  \"rows\": [\n",
        num(price),
        num(reserve_x),
        num(reserve_y),
        rows.len()
    );
    let bodies: Vec<String> = rows
        .iter()
        .map(|row| {
            format!(
                "    {{\"strategy\": \"{}\", \"family\": \"{}\", \"parameter\": \"{}\", \"side\": \"{}\", \"input\": {}, \"output\": {}, \"averagePrice\": {}, \"slippageBps\": {}}}",
                escape(&row.strategy_id),
                escape(&row.family),
                escape(&row.parameter),
                row.side,
                num(row.input),
                num(row.output),
                num(row.average_price),
                num(row.slippage_bps)
            )
        })
        .collect();
    out.push_str(&bodies.join(",\n"));
    out.push_str("\n  ]\n}\n");
    out
}

/// Mid-price table (integer WAD values) for the equilibrium check.
pub fn mid_price_json(rows: &[(String, U256)], expected: U256) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "{{\n  \"expectedMidPriceWad\": \"{expected}\",\n  \"allEqual\": {},\n  \"rows\": [\n",
        rows.iter().all(|(_, value)| *value == expected)
    );
    let bodies: Vec<String> = rows
        .iter()
        .map(|(id, value)| {
            format!(
                "    {{\"strategy\": \"{}\", \"midPriceWad\": \"{value}\"}}",
                escape(id)
            )
        })
        .collect();
    out.push_str(&bodies.join(",\n"));
    out.push_str("\n  ]\n}\n");
    out
}

fn pct(value: f64) -> String {
    if value.is_finite() {
        format!("{:.2}%", value * 100.0)
    } else {
        "n/a".to_string()
    }
}

fn fixed(value: f64) -> String {
    if value.is_finite() {
        format!("{value:.4}")
    } else {
        "n/a".to_string()
    }
}

fn signed(value: f64) -> String {
    if value.is_finite() {
        format!("{value:+.4}")
    } else {
        "n/a".to_string()
    }
}

fn verdict(delta: &PairedDelta) -> &'static str {
    if !delta.is_significant() {
        "不显著"
    } else if delta.mean > 0.0 {
        "显著为正"
    } else {
        "显著为负"
    }
}

fn paired_section_zh(
    out: &mut String,
    flashbots_minus_dodo: &[PairedDelta],
    versus_univ2: &[PairedDelta],
    attributions: &[Attribution],
) {
    out.push_str("\n## 配对统计（逐 seed 相减）\n\n");
    out.push_str(
        "所有策略跑在同一批 seed 上，因此正确的比较是**逐 seed 相减**：配对差抵消了共享的价格路径与订单流，\
才能给出可用的置信区间。下表的 95% CI 为配对差上的正态近似（`均值 ± 1.96 × 标准误`），\
paired win rate 是「treatment 在多少比例的 seed 上胜过 baseline」。\n\n",
    );

    out.push_str("### Flashbots − DODO（按 pairing 行）\n\n");
    out.push_str(
        "| pairing | 指标 | 均值差 | 95% CI | P5 / P50 / P95 | t | paired win rate | 判定 |\n",
    );
    out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for delta in flashbots_minus_dodo {
        let _ = writeln!(
            out,
            "| {} | {} | {} | [{}, {}] | {} / {} / {} | {:.2} | {} | {} |",
            delta
                .pairing_index
                .map(|i| format!("#{i}"))
                .unwrap_or_else(|| "-".to_string()),
            delta.metric,
            signed(delta.mean),
            signed(delta.ci95_low),
            signed(delta.ci95_high),
            signed(delta.distribution.p5),
            signed(delta.distribution.p50),
            signed(delta.distribution.p95),
            delta.t_stat,
            pct(delta.paired_win_rate),
            verdict(delta)
        );
    }
    let significant = flashbots_minus_dodo
        .iter()
        .filter(|d| d.metric == "netEdge" && d.is_significant())
        .count();
    let total = flashbots_minus_dodo
        .iter()
        .filter(|d| d.metric == "netEdge")
        .count();
    let _ = writeln!(
        out,
        "\n netEdge 上共 {total} 个 pairing 行，其中 {significant} 行在 95% 水平上显著。\
不显著的行意味着两条曲线在该参数下**分不出高下**，边际均值的排名不能当作结论。\n"
    );

    out.push_str("### 相对零手续费 Uni V2 的配对差\n\n");
    out.push_str("| 策略 | 指标 | 均值差 | 95% CI | t | paired win rate | 判定 |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
    for delta in versus_univ2.iter().filter(|d| d.metric == "netEdge") {
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
            verdict(delta)
        );
    }

    out.push_str("\n### 优势拆分：oracle 系统性优势 vs 曲线形状差异\n\n");
    out.push_str(
        "锚点取「曲线形状在期初库存处与零手续费恒定乘积重合」的两条 oracle-aware 策略：\
DODO `K = 1e18`（PMM 的恒定乘积特例）与 Flashbots `concentration = 1`（`reserveX == targetX` 时虚拟报价储备等于真实储备）。\
`quote-matrix` 显示它们在该点与 Uni V2 报价逐位一致。于是：\n\n",
    );
    out.push_str("- **oracle 系统性优势** = `均值(锚点 − UniV2)`：曲线形状在期初相同，剩下的就是「按 oracle 重新定价」这件事本身带来的差异。\n");
    out.push_str(
        "- **曲线形状差异** = `均值(策略 − 锚点)`：oracle 相同，剩下的就是曲率参数带来的差异。\n",
    );
    out.push_str(
        "- 两者之和恒等于 `均值(策略 − UniV2)`（同一批 seed 上配对差的均值是线性的）。\n\n",
    );
    out.push_str(
        "> 该拆分锚定在**期初库存**。它不表示 oracle-aware 曲线在其它点上等于恒定乘积——价格一动，锚点会重定价而被动恒定乘积不会。\
只有均值可拆，分位数不可拆，因此不做拆分。\n\n",
    );
    out.push_str("| 策略 | 指标 | oracle 系统性优势 | 曲线形状差异 | 合计（vs UniV2） | 锚点 |\n");
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
}

/// Chinese Markdown summary.
pub fn markdown_zh(
    meta: &RunMeta,
    summaries: &[StrategySummary],
    flashbots_minus_dodo: &[PairedDelta],
    versus_univ2: &[PairedDelta],
    attributions: &[Attribution],
) -> String {
    let mut out = String::new();
    out.push_str("# Oracle-aware AMM 曲线对比 benchmark 结果\n\n");
    out.push_str("## 实验设置\n\n");
    let _ = writeln!(out, "- 模拟次数：{}", meta.simulations);
    let _ = writeln!(out, "- 每次步数：{}", meta.steps);
    let _ = writeln!(
        out,
        "- seed 范围：起始 {}，步长 {}（所有策略共用同一批 seed，逐 seed 配对对比）",
        meta.seed_start, meta.seed_stride
    );
    let _ = writeln!(
        out,
        "- 对手方：{}",
        match meta.competitor {
            Competitor::Normalizer => "challenge normalizer（恒定乘积 + 手续费，参数按 seed 生成）",
            Competitor::None => "无（solo 模式，全部零售流量进入被测曲线）",
        }
    );
    let _ = writeln!(
        out,
        "- 统一初始状态：fair price = {}，reserveX = {}，reserveY = {}",
        meta.initial_price, meta.initial_x, meta.initial_y
    );
    out.push_str("- 手续费：全部被测曲线为零手续费（DODO `lpFeeRate = 0`，Flashbots 原生无手续费，Uni V2 fee = 0）\n");
    out.push_str("- Oracle：每步 `fair_price = price.step()` 后量化一次为 WAD，同一个 `priceWad` 发布给 DODO（`i`）与 Flashbots（`multX`，`multY = 1e18`）\n");
    let _ = writeln!(out, "- 并行 worker：{}", meta.workers);
    let _ = writeln!(out, "- 耗时：{:.1} 秒", meta.elapsed_seconds);
    let _ = writeln!(
        out,
        "- 生成结果的 benchmark commit：`{}`{}\n",
        meta.benchmark_commit,
        if meta.benchmark_dirty {
            "（运行时工作区有未提交改动）"
        } else {
            ""
        }
    );

    out.push_str("## 算法来源（锁定 commit）\n\n");
    let _ = writeln!(
        out,
        "- DODO V2 PMM：`DODOEX/contractV2@{DODO_COMMIT}`（`DecimalMath.sol` / `DODOMath.sol` / `PMMPricing.sol`）"
    );
    let _ = writeln!(
        out,
        "- Flashbots ExamplePropAmm：`flashbots/priority-update-registry@{FLASHBOTS_COMMIT}`"
    );
    out.push_str("- Uniswap V2：`UniswapV2Library.getAmountOut`，仅把手续费分子由 997 改为 1000（零手续费），其余整数运算顺序保持不变\n");
    let _ = writeln!(
        out,
        "- Mantle 参考实现（只读）：`mantle-propamm-contracts@{MANTLE_COMMIT}`（分支 `{MANTLE_BRANCH}`）"
    );
    let _ = writeln!(
        out,
        "  - `src/MantlePropAmmPool.sol` sha256 `{MANTLE_POOL_SHA256}`（target / RState 持久化规则来源）"
    );
    let _ = writeln!(
        out,
        "  - `src/MantlePropAmmTypes.sol` sha256 `{MANTLE_TYPES_SHA256}`（RState 枚举）"
    );
    let _ = writeln!(
        out,
        "  - 设计文档 `{MANTLE_DESIGN_DOC}` sha256 `{MANTLE_DESIGN_DOC_SHA256}`"
    );
    let _ = writeln!(
        out,
        "  - vendor DODO sha256：DecimalMath `{MANTLE_VENDOR_DECIMAL_MATH_SHA256}`，DODOMath `{MANTLE_VENDOR_DODO_MATH_SHA256}`，PMMPricing `{MANTLE_VENDOR_PMM_PRICING_SHA256}`\n"
    );
    let _ = writeln!(out, "> {PAIRING_CAVEAT_ZH}\n");

    out.push_str("## 范围界定\n\n");
    let _ = writeln!(out, "{SCOPE_NOTE_ZH}\n");

    out.push_str("## 运行健康检查\n\n");
    let total_reverts: u64 = summaries.iter().map(|s| s.total_curve_reverts).sum();
    if total_reverts == 0 {
        out.push_str("- 曲线 revert 次数：0（报价数学在整个运行中没有触发 revert 分支）\n");
        let _ = writeln!(out, "- {CURVE_REVERTS_NOTE_ZH}\n");
    } else {
        let _ = writeln!(
            out,
            "- **曲线 revert 次数：{total_reverts}（非零！模拟账本与曲线整数状态出现不一致，结果不可直接采信，请检查 `summary.csv` 的 `curve_reverts` 列）**\n"
        );
    }

    out.push_str("## 净边际（netEdge）排名\n\n");
    let mut ranked: Vec<&StrategySummary> = summaries.iter().collect();
    ranked.sort_by(|a, b| {
        b.net_edge
            .mean
            .partial_cmp(&a.net_edge.mean)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.push_str("| 排名 | 策略 | 参数 | netEdge 均值 | P5 | P50 | P95 | netEdge>0 占比 |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for (rank, s) in ranked.iter().enumerate() {
        let _ = writeln!(
            out,
            "| {} | `{}` | {} | {} | {} | {} | {} | {} |",
            rank + 1,
            s.strategy_id,
            s.parameter,
            fixed(s.net_edge.mean),
            fixed(s.net_edge.p5),
            fixed(s.net_edge.p50),
            fixed(s.net_edge.p95),
            pct(s.positive_net_rate)
        );
    }

    out.push_str("\n## 收益拆分\n\n");
    out.push_str("| 策略 | 参数 | retailEdge 均值 | arbitrageEdge 均值 | arbitrageLoss 均值 | netEdge 均值 | 零售流量份额 |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
    for s in summaries {
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} | {} |",
            s.strategy_id,
            s.parameter,
            fixed(s.retail_edge.mean),
            fixed(s.arbitrage_edge.mean),
            fixed(s.arbitrage_loss.mean),
            fixed(s.net_edge.mean),
            pct(s.retail_flow_share.mean)
        );
    }

    out.push_str("\n## 流量与套利\n\n");
    out.push_str("| 策略 | 零售名义额均值 | 套利次数均值 | 套利名义额均值 |\n");
    out.push_str("| --- | --- | --- | --- |\n");
    for s in summaries {
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} |",
            s.strategy_id,
            fixed(s.retail_notional.mean),
            fixed(s.arb_count.mean),
            fixed(s.arb_notional.mean)
        );
    }

    out.push_str("\n## 库存偏离\n\n");
    out.push_str("库存偏离定义为 `(reserveX - initialX) / initialX`。\n\n");
    out.push_str(
        "| 策略 | 期末偏离均值 | 期末偏离 P5 | 期末偏离 P95 | 最大偏离均值 | 最大偏离 P95 |\n",
    );
    out.push_str("| --- | --- | --- | --- | --- | --- |\n");
    for s in summaries {
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} |",
            s.strategy_id,
            pct(s.final_inventory_deviation.mean),
            pct(s.final_inventory_deviation.p5),
            pct(s.final_inventory_deviation.p95),
            pct(s.max_inventory_deviation.mean),
            pct(s.max_inventory_deviation.p95)
        );
    }

    out.push_str("\n## 完整分位数（netEdge / retailEdge / arbitrageEdge）\n\n");
    out.push_str(
        "| 策略 | netEdge P5/P50/P95 | retailEdge P5/P50/P95 | arbitrageEdge P5/P50/P95 |\n",
    );
    out.push_str("| --- | --- | --- | --- |\n");
    for s in summaries {
        let _ = writeln!(
            out,
            "| `{}` | {} / {} / {} | {} / {} / {} | {} / {} / {} |",
            s.strategy_id,
            fixed(s.net_edge.p5),
            fixed(s.net_edge.p50),
            fixed(s.net_edge.p95),
            fixed(s.retail_edge.p5),
            fixed(s.retail_edge.p50),
            fixed(s.retail_edge.p95),
            fixed(s.arbitrage_edge.p5),
            fixed(s.arbitrage_edge.p50),
            fixed(s.arbitrage_edge.p95)
        );
    }

    paired_section_zh(&mut out, flashbots_minus_dodo, versus_univ2, attributions);

    out.push_str("\n## 说明\n\n");
    out.push_str("- edge 一律从 AMM 角度、按外部 fair price 计价：正值表示 AMM 获利。\n");
    out.push_str("- `arbitrageEdge` 通常为负（AMM 向套利者付出），`arbitrageLoss = max(0, -arbitrageEdge)`。\n");
    out.push_str("- `netEdge = retailEdge + arbitrageEdge`。\n");
    out.push_str(
        "- 曲线内部全部使用整数定点数（uint256）运算；`f64` 只出现在 oracle 量化与模拟账本边界。\n",
    );
    let _ = writeln!(out, "- {CURVE_REVERTS_NOTE_ZH}");
    out.push_str("- Uni V2 不消费 oracle 价格，这是该曲线本身的性质，作为无 oracle 基准列出。\n");
    out.push_str("- `positiveNetRate` 是该策略自身 netEdge 为正的 seed 占比，**不是**与另一条曲线的比较；跨曲线比较一律看配对统计一节的 paired win rate。\n");
    out
}

/// Write every artefact into `dir`.
pub fn write_all(
    dir: &Path,
    meta: &RunMeta,
    summaries: &[StrategySummary],
    runs: &[RunMetrics],
    flashbots_minus_dodo: &[PairedDelta],
    versus_univ2: &[PairedDelta],
    attributions: &[Attribution],
) -> std::io::Result<Vec<PathBuf>> {
    fs::create_dir_all(dir)?;
    let mut written = Vec::new();

    let mut all_deltas: Vec<PairedDelta> = flashbots_minus_dodo.to_vec();
    all_deltas.extend_from_slice(versus_univ2);

    let files: Vec<(&str, String)> = vec![
        ("summary.json", summary_json_document(meta, summaries)),
        ("summary.csv", summary_csv(summaries)),
        ("runs.csv", runs_csv(runs)),
        (
            "paired-stats.json",
            paired_json_document(meta, flashbots_minus_dodo, versus_univ2, attributions),
        ),
        ("paired-deltas.csv", paired_deltas_csv(&all_deltas)),
        ("attribution.csv", attribution_csv(attributions)),
        (
            "REPORT.zh-CN.md",
            markdown_zh(
                meta,
                summaries,
                flashbots_minus_dodo,
                versus_univ2,
                attributions,
            ),
        ),
    ];
    for (name, body) in files {
        let path = dir.join(name);
        fs::write(&path, body)?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::Json;
    use crate::metrics::RunMetrics;

    fn runs() -> Vec<RunMetrics> {
        (0..4)
            .map(|seed| RunMetrics {
                strategy_id: "dodo-k1".into(),
                seed,
                steps: 100,
                retail_edge: 2.0 + seed as f64,
                arbitrage_edge: -1.0,
                arbitrage_loss: 1.0,
                net_edge: 1.0 + seed as f64,
                retail_notional: 50.0,
                competitor_retail_notional: 50.0,
                retail_flow_share: 0.5,
                retail_trade_count: 3,
                arb_count: 2,
                arb_notional: 20.0,
                final_inventory_deviation: 0.01,
                max_inventory_deviation: 0.05,
                final_reserve_x: 101.0,
                final_reserve_y: 9_900.0,
                final_fair_price: 100.5,
                curve_revert_count: 0,
            })
            .collect()
    }

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
            elapsed_seconds: 1.25,
            benchmark_commit: "0123456789abcdef".to_string(),
            benchmark_dirty: false,
        }
    }

    #[test]
    fn summary_json_is_parseable_and_carries_provenance() {
        let runs = runs();
        let summaries = vec![StrategySummary::from_runs(
            "dodo-k1", "dodo", "K=1e18", &runs,
        )];
        let document = summary_json_document(&meta(), &summaries);
        let json = Json::parse(&document).expect("valid JSON");
        assert_eq!(
            json.get("provenance")
                .unwrap()
                .get("dodoUpstream")
                .unwrap()
                .get("commit")
                .unwrap()
                .as_str(),
            Some(DODO_COMMIT)
        );
        assert_eq!(
            json.get("config")
                .unwrap()
                .get("competitor")
                .unwrap()
                .as_str(),
            Some("normalizer")
        );
        let strategies = json.get("strategies").unwrap().as_array().unwrap();
        assert_eq!(strategies.len(), 1);
        assert_eq!(
            strategies[0]
                .get("netEdge")
                .unwrap()
                .get("p50")
                .unwrap()
                .as_num_str(),
            Some("2")
        );
    }

    #[test]
    fn csv_has_a_header_and_one_row_per_record() {
        let runs = runs();
        let summaries = vec![StrategySummary::from_runs(
            "dodo-k1", "dodo", "K=1e18", &runs,
        )];
        assert_eq!(summary_csv(&summaries).lines().count(), 2);
        assert_eq!(runs_csv(&runs).lines().count(), 5);
    }

    #[test]
    fn markdown_is_chinese_and_states_the_pairing_caveat() {
        let runs = runs();
        let summaries = vec![StrategySummary::from_runs(
            "dodo-k1", "dodo", "K=1e18", &runs,
        )];
        let markdown = markdown_zh(&meta(), &summaries, &[], &[], &[]);
        assert!(markdown.contains("Oracle-aware AMM 曲线对比 benchmark 结果"));
        assert!(markdown.contains(PAIRING_CAVEAT_ZH));
        assert!(markdown.contains("统一初始状态"));
        assert!(markdown.contains(DODO_COMMIT));
        assert!(markdown.contains(FLASHBOTS_COMMIT));
    }

    #[test]
    fn markdown_states_scope_and_does_not_overclaim_curve_reverts() {
        let runs = runs();
        let summaries = vec![StrategySummary::from_runs(
            "dodo-k1", "dodo", "K=1e18", &runs,
        )];
        let markdown = markdown_zh(&meta(), &summaries, &[], &[], &[]);

        // Out-of-scope guards must be named explicitly.
        assert!(markdown.contains("targetY emergency lock"));
        assert!(markdown.contains("inventory deviation"));
        assert!(markdown.contains("curve-only"));
        // `curve_reverts = 0` must not be sold as a successful on-chain swap.
        assert!(markdown.contains("不等于**一笔完整的链上 swap 会成功"));
        assert!(
            !markdown.contains("链上语义内成功"),
            "the old overclaiming wording must be gone"
        );
        // Provenance must pin the Mantle reference and the benchmark commit.
        assert!(markdown.contains(MANTLE_COMMIT));
        assert!(markdown.contains(MANTLE_POOL_SHA256));
        assert!(markdown.contains(MANTLE_DESIGN_DOC_SHA256));
        assert!(markdown.contains("0123456789abcdef"));
    }

    #[test]
    fn paired_sections_render_and_flag_indistinguishable_rows() {
        use crate::metrics::Distribution;
        use crate::paired::PairedDelta;

        let delta = PairedDelta {
            treatment: "flashbots-c1".into(),
            baseline: "dodo-k1".into(),
            pairing_index: Some(0),
            metric: "netEdge",
            samples: 1_000,
            mean: 0.007,
            distribution: Distribution::from_samples(&[-1.0, 0.0, 1.0]),
            std_error: 0.01,
            ci95_low: -0.0126,
            ci95_high: 0.0266,
            t_stat: 0.7,
            paired_win_rate: 0.502,
        };
        let runs = runs();
        let summaries = vec![StrategySummary::from_runs(
            "dodo-k1", "dodo", "K=1e18", &runs,
        )];
        let markdown = markdown_zh(
            &meta(),
            &summaries,
            std::slice::from_ref(&delta),
            std::slice::from_ref(&delta),
            &[],
        );

        assert!(markdown.contains("配对统计"));
        assert!(markdown.contains("paired win rate"));
        assert!(markdown.contains("不显著"));
        assert!(markdown.contains("oracle 系统性优势"));

        let csv = paired_deltas_csv(std::slice::from_ref(&delta));
        assert_eq!(csv.lines().count(), 2);
        assert!(csv.contains("false"), "significance flag must be recorded");

        let json = Json::parse(&paired_json_document(
            &meta(),
            std::slice::from_ref(&delta),
            std::slice::from_ref(&delta),
            &[],
        ))
        .expect("valid JSON");
        let rows = json.get("flashbotsMinusDodo").unwrap().as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("significant95").unwrap().as_bool(), Some(false));
        assert_eq!(
            json.get("attributionAnchors")
                .unwrap()
                .get("baseline")
                .unwrap()
                .as_str(),
            Some("univ2-zero-fee")
        );
    }

    #[test]
    fn summary_json_records_the_benchmark_commit_and_scope() {
        let runs = runs();
        let summaries = vec![StrategySummary::from_runs(
            "dodo-k1", "dodo", "K=1e18", &runs,
        )];
        let json = Json::parse(&summary_json_document(&meta(), &summaries)).expect("valid JSON");
        let provenance = json.get("provenance").unwrap();
        assert_eq!(
            provenance.get("benchmarkCommit").unwrap().as_str(),
            Some("0123456789abcdef")
        );
        assert_eq!(
            provenance
                .get("benchmarkWorkingTreeDirty")
                .unwrap()
                .as_bool(),
            Some(false)
        );
        assert_eq!(
            provenance
                .get("mantleReference")
                .unwrap()
                .get("poolSha256")
                .unwrap()
                .as_str(),
            Some(MANTLE_POOL_SHA256)
        );
        assert!(provenance
            .get("scopeNote")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("curve-only"));
        assert!(provenance
            .get("curveRevertsNote")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("不等于"));
    }

    #[test]
    fn mid_price_json_flags_disagreement() {
        let expected = U256::from_u64(100);
        let rows = vec![
            ("a".to_string(), expected),
            ("b".to_string(), U256::from_u64(101)),
        ];
        let document = mid_price_json(&rows, expected);
        let json = Json::parse(&document).expect("valid JSON");
        assert_eq!(json.get("allEqual").unwrap().as_bool(), Some(false));
    }
}
