//! Machine-readable (JSON + CSV) and human-readable (Chinese Markdown) output.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::experiment::{BatchConfig, Competitor};
use crate::json::{escape, num};
use crate::metrics::{Distribution, RunMetrics, StrategySummary};
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
}

impl RunMeta {
    pub fn from_batch(batch: &BatchConfig, elapsed_seconds: f64) -> RunMeta {
        RunMeta {
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
pub const PAIRING_CAVEAT_ZH: &str = "K ≈ 1 / concentration 仅用于匹配平衡点附近的局部曲率；它不修改、不替换任何原始公式，也不表示两条全局曲线等价。";

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
        "{{\"strategy\": \"{}\", \"family\": \"{}\", \"parameter\": \"{}\", \"simulations\": {}, \"steps\": {}, \"winRate\": {}, \"totalNetEdge\": {}, \"totalCurveReverts\": {}, ",
        escape(&summary.strategy_id),
        escape(&summary.family),
        escape(&summary.parameter),
        summary.simulations,
        summary.steps,
        num(summary.win_rate),
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
        ("finalInventoryDeviation", &summary.final_inventory_deviation),
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
        "  \"provenance\": {{\n    \"dodoUpstream\": {{\"repository\": \"https://github.com/DODOEX/contractV2\", \"commit\": \"{DODO_COMMIT}\"}},\n    \"flashbotsUpstream\": {{\"repository\": \"https://github.com/flashbots/priority-update-registry\", \"commit\": \"{FLASHBOTS_COMMIT}\"}},\n    \"univ2\": \"UniswapV2Library.getAmountOut with the fee numerator set to 1000/1000 (zero fee)\",\n    \"pairingCaveat\": \"{}\"\n  }},\n",
        escape(PAIRING_CAVEAT_ZH)
    );
    let _ = write!(
        out,
        "  \"config\": {{\"simulations\": {}, \"steps\": {}, \"seedStart\": {}, \"seedStride\": {}, \"competitor\": \"{}\", \"workers\": {}, \"initialPrice\": {}, \"initialX\": {}, \"initialY\": {}, \"lpFeeRate\": 0, \"elapsedSeconds\": {}}},\n",
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
        "strategy,family,parameter,simulations,steps,win_rate,total_net_edge,\
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
            s.win_rate,
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
        .map(|(id, value)| format!("    {{\"strategy\": \"{}\", \"midPriceWad\": \"{value}\"}}", escape(id)))
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

/// Chinese Markdown summary.
pub fn markdown_zh(meta: &RunMeta, summaries: &[StrategySummary]) -> String {
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
    let _ = writeln!(out, "- 耗时：{:.1} 秒\n", meta.elapsed_seconds);

    out.push_str("## 算法来源（锁定 commit）\n\n");
    let _ = writeln!(
        out,
        "- DODO V2 PMM：`DODOEX/contractV2@{DODO_COMMIT}`（`DecimalMath.sol` / `DODOMath.sol` / `PMMPricing.sol`）"
    );
    let _ = writeln!(
        out,
        "- Flashbots ExamplePropAmm：`flashbots/priority-update-registry@{FLASHBOTS_COMMIT}`"
    );
    out.push_str("- Uniswap V2：`UniswapV2Library.getAmountOut`，仅把手续费分子由 997 改为 1000（零手续费），其余整数运算顺序保持不变\n\n");
    let _ = writeln!(out, "> {PAIRING_CAVEAT_ZH}\n");

    out.push_str("## 运行健康检查\n\n");
    let total_reverts: u64 = summaries.iter().map(|s| s.total_curve_reverts).sum();
    if total_reverts == 0 {
        out.push_str("- 曲线 revert 次数：0（所有曲线调用都在链上语义内成功，结果可信）\n\n");
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
    out.push_str("| 排名 | 策略 | 参数 | netEdge 均值 | P5 | P50 | P95 | 胜率 |\n");
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
            pct(s.win_rate)
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
    out.push_str("| 策略 | 期末偏离均值 | 期末偏离 P5 | 期末偏离 P95 | 最大偏离均值 | 最大偏离 P95 |\n");
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
    out.push_str("| 策略 | netEdge P5/P50/P95 | retailEdge P5/P50/P95 | arbitrageEdge P5/P50/P95 |\n");
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

    out.push_str("\n## 说明\n\n");
    out.push_str("- edge 一律从 AMM 角度、按外部 fair price 计价：正值表示 AMM 获利。\n");
    out.push_str("- `arbitrageEdge` 通常为负（AMM 向套利者付出），`arbitrageLoss = max(0, -arbitrageEdge)`。\n");
    out.push_str("- `netEdge = retailEdge + arbitrageEdge`。\n");
    out.push_str("- 曲线内部全部使用整数定点数（uint256）运算；`f64` 只出现在 oracle 量化与模拟账本边界。\n");
    out.push_str("- `curve_reverts` 统计的是在非退化输入下、链上会 revert 的曲线调用次数；健康的运行应为 0。\n");
    out.push_str("- Uni V2 不消费 oracle 价格，这是该曲线本身的性质，作为无 oracle 基准列出。\n");
    out
}

/// Write every artefact into `dir`.
pub fn write_all(
    dir: &Path,
    meta: &RunMeta,
    summaries: &[StrategySummary],
    runs: &[RunMetrics],
) -> std::io::Result<Vec<PathBuf>> {
    fs::create_dir_all(dir)?;
    let mut written = Vec::new();

    let files: Vec<(&str, String)> = vec![
        ("summary.json", summary_json_document(meta, summaries)),
        ("summary.csv", summary_csv(summaries)),
        ("runs.csv", runs_csv(runs)),
        ("REPORT.zh-CN.md", markdown_zh(meta, summaries)),
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
        }
    }

    #[test]
    fn summary_json_is_parseable_and_carries_provenance() {
        let runs = runs();
        let summaries = vec![StrategySummary::from_runs("dodo-k1", "dodo", "K=1e18", &runs)];
        let document = summary_json_document(&meta(), &summaries);
        let json = Json::parse(&document).expect("valid JSON");
        assert_eq!(
            json.get("provenance").unwrap().get("dodoUpstream").unwrap().get("commit").unwrap().as_str(),
            Some(DODO_COMMIT)
        );
        assert_eq!(
            json.get("config").unwrap().get("competitor").unwrap().as_str(),
            Some("normalizer")
        );
        let strategies = json.get("strategies").unwrap().as_array().unwrap();
        assert_eq!(strategies.len(), 1);
        assert_eq!(
            strategies[0].get("netEdge").unwrap().get("p50").unwrap().as_num_str(),
            Some("2")
        );
    }

    #[test]
    fn csv_has_a_header_and_one_row_per_record() {
        let runs = runs();
        let summaries = vec![StrategySummary::from_runs("dodo-k1", "dodo", "K=1e18", &runs)];
        assert_eq!(summary_csv(&summaries).lines().count(), 2);
        assert_eq!(runs_csv(&runs).lines().count(), 5);
    }

    #[test]
    fn markdown_is_chinese_and_states_the_pairing_caveat() {
        let runs = runs();
        let summaries = vec![StrategySummary::from_runs("dodo-k1", "dodo", "K=1e18", &runs)];
        let markdown = markdown_zh(&meta(), &summaries);
        assert!(markdown.contains("Oracle-aware AMM 曲线对比 benchmark 结果"));
        assert!(markdown.contains(PAIRING_CAVEAT_ZH));
        assert!(markdown.contains("统一初始状态"));
        assert!(markdown.contains(DODO_COMMIT));
        assert!(markdown.contains(FLASHBOTS_COMMIT));
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
