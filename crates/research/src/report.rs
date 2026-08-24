//! Machine-readable (JSON + CSV) and human-readable (Chinese Markdown) output.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::experiment::{BatchConfig, Competitor};
use crate::json::{escape, num};
use crate::metrics::{Distribution, RunMetrics, StrategySummary, Univ3Summary};
use crate::paired::Univ3SanityGate;
use crate::paired::{self, Attribution, PairedDelta, Univ3Attribution};
use crate::probe::QuoteRow;
use crate::provenance::Provenance;
use crate::report_focus;
use crate::strategies::StrategySet;
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
    /// Where this result set came from: the commit the **binary** was built
    /// from, plus the repository state at run start and run end. See
    /// [`crate::provenance`] for why one runtime capture is not enough.
    pub provenance: Provenance,
    /// Which catalogue was run. Recorded because two result sets are only
    /// comparable when they contain the same strategies.
    pub strategy_set: String,
}

impl RunMeta {
    pub fn from_batch(batch: &BatchConfig, elapsed_seconds: f64) -> RunMeta {
        RunMeta::from_batch_with_set(batch, elapsed_seconds, StrategySet::Legacy)
    }

    pub fn from_batch_with_set(
        batch: &BatchConfig,
        elapsed_seconds: f64,
        set: StrategySet,
    ) -> RunMeta {
        let mut provenance = Provenance::begin();
        provenance.finish();
        RunMeta::from_batch_with_provenance(batch, elapsed_seconds, set, provenance)
    }

    pub fn from_batch_with_provenance(
        batch: &BatchConfig,
        elapsed_seconds: f64,
        set: StrategySet,
        provenance: Provenance,
    ) -> RunMeta {
        RunMeta {
            provenance,
            strategy_set: set.as_str().to_string(),
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

impl RunMeta {
    /// The commit a result set may claim: the one that built the binary.
    pub fn benchmark_commit(&self) -> &str {
        self.provenance.benchmark_commit()
    }

    /// True when binary, run-start and run-end all agree and all are clean.
    pub fn provenance_match(&self) -> bool {
        self.provenance.matches()
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
/// Mantle design document, as read at [`MANTLE_COMMIT`].
pub const MANTLE_DESIGN_DOC: &str = "docs/[TD] mantle PropAmm 合约设计.md";
/// sha256 of the document **as it was read**. The reference repository is
/// read-only to this benchmark but not frozen, and it has since advanced past
/// `MANTLE_COMMIT`; at the time of writing the document there hashes to
/// [`MANTLE_DESIGN_DOC_SHA256_OBSERVED_LATER`]. The hash below is deliberately
/// *not* updated to match, because updating it would claim the newer text had
/// been read when it has not.
pub const MANTLE_DESIGN_DOC_SHA256: &str =
    "c395caa6eb5c68595274bf1c1af29882c23c1228fdf7a2dda019961fb7e52b3a";
/// What the same path hashed to on a later check. Recorded so the divergence is
/// visible rather than discovered. Every **code** file this port depends on
/// (`MantlePropAmmPool.sol`, `MantlePropAmmTypes.sol` and the three DODO vendor
/// copies) still matches its pinned hash, so no measurement is affected.
pub const MANTLE_DESIGN_DOC_SHA256_OBSERVED_LATER: &str =
    "f3f6f4aaafd4aa4e68515e19f3cb1c1fe4fa303d95e51a9be9c8c1ec175027dd";
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

pub const UNIV3_QUOTE_PROBE_NOTE_ZH: &str = "这些是**候选报价级**诊断，不是订单计数。router 与 arbitrageur 会对同一笔订单搜索多个候选成交量，每个候选都会调用一次曲线；某个候选因区间容量不足被 fill-or-kill 拒绝，完全可以与「该订单最终被全额成交」同时成立。因此这些计数随搜索强度变化，**只能作为搜索相关的诊断项，不得进入任何经济结论，也不得被称为『整单拒绝』**。金额按提供的代币分列（X 与 Y 各自求和），不相加、也不折算：adapter 在记录时看不到当步公允价，事后用收盘价回标是错的。";

pub const UNIV3_ORDER_CAPACITY_NOTE_ZH: &str = "这些是**订单级**指标：对每一笔零售订单，在**路由前的池状态**上、按**订单全额**做一次 canonical probe，只做一次，且不写入任何计数器。因此它与搜索次数无关，是确定性的。`retailCapacityShortfallNotionalY` 在**订单到达那一步的公允价**下折算成 Y，绝不使用收盘价回标。注意：**池自身的容量缺口与实际路由给 competitor 的流量不是同一件事**——router 同时按价格和容量拆单，被计入缺口的订单仍可能由「池 + competitor」合起来全额成交，而未计入缺口的订单也可能因为 competitor 报价更好而流走。";

pub const UNIV3_RANGE_NOTE_ZH: &str = "`fairPriceOutOfRangeRate` 按**发布的公允价**逐步采样，衡量池能否在市场价上报价；`activeLiquidityRate` 按池自身 tick 采样，在 fill-or-kill 下几乎恒为 100%，信息量很低（越过边界需要恰好吃掉区间内最后一份流动性的那一单，而那一单正是会被整单拒绝的）。两者是不同的问题，不可互相替代。V3 仓位一次性铸造后**从不再平衡**。";

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
    if let Some(v3) = &summary.univ3 {
        out.push_str(", \"univ3\": ");
        out.push_str(&univ3_summary_json(v3));
    }
    out.push('}');
    out
}

/// The Uniswap V3 block. Absent entirely for non-V3 strategies, so a reader
/// never sees a zeroed capacity count where no capacity path exists.
///
/// The two groups are namespaced apart on purpose: `quoteProbeDiagnostics` are
/// per-candidate-quote counts produced by the router's and arbitrageur's search
/// and carry no economic reading, while `orderLevelCapacity` is one canonical
/// probe per retail order and does.
fn univ3_summary_json(summary: &Univ3Summary) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "{{\"quoteProbeDiagnostics\": {{\"capacityQuoteRejectCount\": {}, \
         \"retailQuoteRejectCount\": {}, \"arbQuoteRejectCount\": {}, \
         \"quoteRejectCountBuyX\": {}, \"quoteRejectCountSellX\": {}, ",
        summary.total_capacity_quote_rejects,
        summary.total_retail_quote_rejects,
        summary.total_arb_quote_rejects,
        summary.total_quote_rejects_buy_x,
        summary.total_quote_rejects_sell_x
    );
    let diagnostics = [
        (
            "quoteRejectedRequestedY",
            &summary.quote_rejected_requested_y,
        ),
        (
            "quoteRejectedRequestedX",
            &summary.quote_rejected_requested_x,
        ),
        (
            "quoteCanonicalUnfilledY",
            &summary.quote_canonical_unfilled_y,
        ),
        (
            "quoteCanonicalUnfilledX",
            &summary.quote_canonical_unfilled_x,
        ),
    ];
    let rendered: Vec<String> = diagnostics
        .iter()
        .map(|(name, d)| distribution_json(name, d))
        .collect();
    out.push_str(&rendered.join(", "));
    let _ = write!(
        out,
        ", \"note\": \"{}\"}}, ",
        escape(UNIV3_QUOTE_PROBE_NOTE_ZH)
    );

    let _ = write!(
        out,
        "\"orderLevelCapacity\": {{\"retailOrdersProbed\": {}, \
         \"retailFullOrderCapacityLimitedCount\": {}, ",
        summary.total_retail_orders_probed, summary.total_retail_capacity_limited_orders
    );
    let capacity = [
        (
            "retailCapacityLimitedRate",
            &summary.retail_capacity_limited_rate,
        ),
        (
            "retailCapacityShortfallNotionalY",
            &summary.retail_capacity_shortfall_notional_y,
        ),
        ("retailNotionalServed", &summary.retail_notional_served),
    ];
    let rendered: Vec<String> = capacity
        .iter()
        .map(|(name, d)| distribution_json(name, d))
        .collect();
    out.push_str(&rendered.join(", "));
    let _ = write!(
        out,
        ", \"note\": \"{}\"}}, ",
        escape(UNIV3_ORDER_CAPACITY_NOTE_ZH)
    );

    let _ = write!(
        out,
        "\"rangeOccupancy\": {{\"seedsThatLeftRange\": {}, ",
        summary.seeds_that_left_range
    );
    let occupancy = [
        (
            "fairPriceOutOfRangeRate",
            &summary.fair_price_out_of_range_rate,
        ),
        ("activeLiquidityRate", &summary.active_liquidity_rate),
        ("firstOutOfRangeStep", &summary.first_out_of_range_step),
    ];
    let rendered: Vec<String> = occupancy
        .iter()
        .map(|(name, d)| distribution_json(name, d))
        .collect();
    out.push_str(&rendered.join(", "));
    let _ = write!(out, ", \"note\": \"{}\"}}", escape(UNIV3_RANGE_NOTE_ZH));
    out.push('}');
    out
}

/// One row per V3 strategy. Quote-probe columns are prefixed `quote_` so that a
/// spreadsheet reader cannot mistake them for order counts.
pub fn univ3_capacity_csv(summaries: &[StrategySummary]) -> String {
    let mut out = String::new();
    out.push_str(
        "strategy,family,parameter,simulations,steps,\
retail_orders_probed,retail_full_order_capacity_limited_count,\
retail_capacity_probe_revert_count,\
retail_capacity_limited_rate_mean,retail_capacity_shortfall_notional_y_mean,\
retail_capacity_shortfall_notional_y_p95,retail_notional_served_mean,\
quote_capacity_reject_count,quote_retail_reject_count,quote_arb_reject_count,\
quote_reject_count_buy_x,quote_reject_count_sell_x,\
quote_rejected_requested_y_mean,quote_rejected_requested_x_mean,\
quote_canonical_unfilled_y_mean,quote_canonical_unfilled_x_mean,\
fair_price_out_of_range_rate_mean,fair_price_out_of_range_rate_p95,\
active_liquidity_rate_mean,first_out_of_range_step_mean,seeds_that_left_range\n",
    );
    for s in summaries {
        let Some(v3) = &s.univ3 else { continue };
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            s.strategy_id,
            csv_field(&s.family),
            csv_field(&s.parameter),
            s.simulations,
            s.steps,
            v3.total_retail_orders_probed,
            v3.total_retail_capacity_limited_orders,
            v3.total_retail_capacity_probe_reverts,
            v3.retail_capacity_limited_rate.mean,
            v3.retail_capacity_shortfall_notional_y.mean,
            v3.retail_capacity_shortfall_notional_y.p95,
            v3.retail_notional_served.mean,
            v3.total_capacity_quote_rejects,
            v3.total_retail_quote_rejects,
            v3.total_arb_quote_rejects,
            v3.total_quote_rejects_buy_x,
            v3.total_quote_rejects_sell_x,
            v3.quote_rejected_requested_y.mean,
            v3.quote_rejected_requested_x.mean,
            v3.quote_canonical_unfilled_y.mean,
            v3.quote_canonical_unfilled_x.mean,
            v3.fair_price_out_of_range_rate.mean,
            v3.fair_price_out_of_range_rate.p95,
            v3.active_liquidity_rate.mean,
            v3.first_out_of_range_step.mean,
            v3.seeds_that_left_range
        );
    }
    out
}

/// The V3 concentration decomposition, one row per strategy and metric.
pub fn univ3_attribution_csv(attributions: &[Univ3Attribution]) -> String {
    let mut out = String::new();
    out.push_str(
        "strategy,family,parameter,anchor,metric,\
full_range_vs_univ2,concentration_effect,total_vs_univ2\n",
    );
    for a in attributions {
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{}",
            a.strategy,
            csv_field(&a.family),
            csv_field(&a.parameter),
            a.anchor,
            a.metric,
            a.full_range_vs_univ2,
            a.concentration_effect,
            a.total_vs_univ2
        );
    }
    out
}

/// Whether the result set may be published: provenance agrees **and** every hard
/// gate passed. A `false` here means the numbers can be looked at but must not
/// be presented as a deliverable.
pub fn publishable(meta: &RunMeta, summaries: &[StrategySummary]) -> bool {
    meta.provenance_match()
        && total_curve_reverts(summaries) == 0
        && total_capacity_probe_reverts(summaries) == 0
        && full_range_capacity_limited(summaries) == 0
}

pub fn total_curve_reverts(summaries: &[StrategySummary]) -> u64 {
    summaries.iter().map(|s| s.total_curve_reverts).sum()
}

/// Canonical capacity probes that reverted, across every V3 arm.
pub fn total_capacity_probe_reverts(summaries: &[StrategySummary]) -> u64 {
    summaries
        .iter()
        .filter_map(|s| s.univ3.as_ref())
        .map(|v3| v3.total_retail_capacity_probe_reverts)
        .sum()
}

/// Capacity-limited orders on the full-range arm, which must be impossible.
pub fn full_range_capacity_limited(summaries: &[StrategySummary]) -> u64 {
    summaries
        .iter()
        .filter(|s| s.strategy_id == paired::UNIV3_FULL_RANGE_ID)
        .filter_map(|s| s.univ3.as_ref())
        .map(|v3| v3.total_retail_capacity_limited_orders)
        .sum()
}

pub fn summary_json_document(meta: &RunMeta, summaries: &[StrategySummary]) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    let _ = write!(
        out,
        "  \"provenance\": {{\n    \"benchmarkCommit\": \"{}\",\n    \"benchmarkCommitIs\": \"the commit the BINARY was built from, not the repository HEAD when the report was written\",\n    \"binaryCommit\": \"{}\",\n    \"binaryWorkingTreeDirty\": {},\n    \"runStartCommit\": \"{}\",\n    \"runStartWorkingTreeDirty\": {},\n    \"runEndCommit\": \"{}\",\n    \"runEndWorkingTreeDirty\": {},\n    \"provenanceMatch\": {},\n    \"provenanceMismatchReasons\": [{}],\n    \"dodoUpstream\": {{\"repository\": \"https://github.com/DODOEX/contractV2\", \"commit\": \"{DODO_COMMIT}\"}},\n    \"flashbotsUpstream\": {{\"repository\": \"https://github.com/flashbots/priority-update-registry\", \"commit\": \"{FLASHBOTS_COMMIT}\"}},\n    \"univ2\": \"UniswapV2Library.getAmountOut with the fee numerator set to 1000/1000 (zero fee)\",\n    \"mantleReference\": {{\"repository\": \"mantle-propamm-contracts\", \"branch\": \"{MANTLE_BRANCH}\", \"commit\": \"{MANTLE_COMMIT}\", \"readOnly\": true, \"poolSha256\": \"{MANTLE_POOL_SHA256}\", \"typesSha256\": \"{MANTLE_TYPES_SHA256}\", \"designDoc\": \"{}\", \"designDocSha256\": \"{MANTLE_DESIGN_DOC_SHA256}\", \"designDocSha256ObservedLater\": \"{MANTLE_DESIGN_DOC_SHA256_OBSERVED_LATER}\", \"designDocNote\": \"the design document has changed in the read-only reference repository since it was read; every code file this port depends on still matches its pinned hash\", \"vendorDecimalMathSha256\": \"{MANTLE_VENDOR_DECIMAL_MATH_SHA256}\", \"vendorDodoMathSha256\": \"{MANTLE_VENDOR_DODO_MATH_SHA256}\", \"vendorPmmPricingSha256\": \"{MANTLE_VENDOR_PMM_PRICING_SHA256}\"}},\n    \"pairingCaveat\": \"{}\",\n    \"scopeNote\": \"{}\",\n    \"curveRevertsNote\": \"{}\"\n  }},\n",
        escape(meta.benchmark_commit()),
        escape(&meta.provenance.binary_commit),
        meta.provenance.binary_dirty,
        escape(&meta.provenance.run_start.commit),
        meta.provenance.run_start.dirty,
        escape(&meta.provenance.run_end.commit),
        meta.provenance.run_end.dirty,
        meta.provenance_match(),
        meta.provenance
            .mismatch_reasons()
            .iter()
            .map(|reason| format!("\"{}\"", escape(reason)))
            .collect::<Vec<_>>()
            .join(", "),
        escape(MANTLE_DESIGN_DOC),
        escape(PAIRING_CAVEAT_ZH),
        escape(SCOPE_NOTE_ZH),
        escape(CURVE_REVERTS_NOTE_ZH)
    );
    let _ = writeln!(
        out,
        "  \"config\": {{\"simulations\": {}, \"steps\": {}, \"seedStart\": {}, \"seedStride\": {}, \"competitor\": \"{}\", \"strategySet\": \"{}\", \"workers\": {}, \"initialPrice\": {}, \"initialX\": {}, \"initialY\": {}, \"lpFeeRate\": 0, \"elapsedSeconds\": {}}},\n  \"publishable\": {},\n  \"publishableIs\": \"provenanceMatch AND every gate passed; false means the result set may be read but not published\",\n  \"gates\": {{\"curveReverts\": {}, \"univ3CapacityProbeReverts\": {}, \"univ3FullRangeCapacityLimitedOrders\": {}}},",
        meta.simulations,
        meta.steps,
        meta.seed_start,
        meta.seed_stride,
        meta.competitor.as_str(),
        escape(&meta.strategy_set),
        meta.workers,
        num(meta.initial_price),
        num(meta.initial_x),
        num(meta.initial_y),
        num(meta.elapsed_seconds),
        publishable(meta, summaries),
        total_curve_reverts(summaries),
        total_capacity_probe_reverts(summaries),
        full_range_capacity_limited(summaries)
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

/// One CSV field, quoted per RFC 4180 only when it needs to be.
///
/// The V3 arms carry a `parameter` string containing commas
/// (`fee=0, halfWidth=201 ticks, ...`), which would otherwise split into extra
/// columns. Values without a comma, quote or newline are returned unchanged, so
/// every pre-existing file stays byte-identical.
fn csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
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
            csv_field(&s.family),
            csv_field(&s.parameter),
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
            csv_field(&a.family),
            csv_field(&a.parameter),
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
        escape(meta.benchmark_commit()),
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
            csv_field(&row.family),
            csv_field(&row.parameter),
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
    univ3_capacity_deltas: &[PairedDelta],
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
        meta.benchmark_commit(),
        if !meta.provenance_match() {
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
    univ3_section_zh(&mut out, summaries, univ3_capacity_deltas);

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

/// Everything one benchmark run produced, ready to be written out.
pub struct Artefacts<'a> {
    pub summaries: &'a [StrategySummary],
    pub runs: &'a [RunMetrics],
    pub flashbots_minus_dodo: &'a [PairedDelta],
    pub versus_univ2: &'a [PairedDelta],
    pub attributions: &'a [Attribution],
    /// Empty unless the run included the Uniswap V3 arms.
    pub univ3_attributions: &'a [Univ3Attribution],
    /// Every strategy against the full-range V3 baseline. Empty without V3.
    pub versus_univ3: &'a [PairedDelta],
    /// `full-range V3 − UniV2`, the sanity residual. Empty without V3.
    pub univ3_minus_univ2: &'a [PairedDelta],
    /// `concentrated − full-range` on the order-level capacity shortfall.
    pub univ3_capacity_deltas: &'a [PairedDelta],
    /// Static quote matrix at the opening state, quoted by the focused reports.
    pub quote_rows: &'a [QuoteRow],
    /// The full-range V3 sanity gate. `None` without the V3 arms.
    pub univ3_sanity: Option<&'a Univ3SanityGate>,
}

/// The Uniswap V3 section. Emitted only when the run contained V3 arms.
fn univ3_section_zh(
    out: &mut String,
    summaries: &[StrategySummary],
    capacity_deltas: &[PairedDelta],
) {
    let v3: Vec<&StrategySummary> = summaries.iter().filter(|s| s.univ3.is_some()).collect();
    if v3.is_empty() {
        return;
    }

    out.push_str("\n## Uniswap V3：容量与区间占用\n\n");
    out.push_str(
        "V3 是本 benchmark 中**唯一**可能只吃掉部分输入的曲线。模拟接口无法表达部分成交，\
         因此 adapter 采用 fill-or-kill：该笔报价不接，返回 0。**这不是 revert**，\
         `curve_reverts` 不计入。\n\n",
    );

    out.push_str("### 订单级容量（确定性，可用于结论）\n\n");
    out.push_str(
        "对每笔零售订单，在**路由前**的池状态上、按**订单全额**做一次 canonical probe，\
         每单一次，与搜索次数无关。缺口按**订单当步的公允价**折算成 Y。\n\n",
    );
    out.push_str("| 策略 | 参数 | 探测订单数 | 容量不足订单数 | probe revert 数 | 占比 | 缺口 Y 均值 | 缺口 Y P95 | V3 实际服务的零售 Y |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for s in &v3 {
        let Some(m) = &s.univ3 else { continue };
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} |",
            s.strategy_id,
            s.parameter,
            m.total_retail_orders_probed,
            m.total_retail_capacity_limited_orders,
            m.total_retail_capacity_probe_reverts,
            pct(m.retail_capacity_limited_rate.mean),
            fixed(m.retail_capacity_shortfall_notional_y.mean),
            fixed(m.retail_capacity_shortfall_notional_y.p95),
            fixed(m.retail_notional_served.mean)
        );
    }
    let _ = writeln!(out, "\n{UNIV3_ORDER_CAPACITY_NOTE_ZH}");

    if !capacity_deltas.is_empty() {
        out.push_str("\n#### 逐 seed 配对差：concentrated − full-range（容量缺口 Y）\n\n");
        out.push_str("| 策略 | 基准 | 均值差 | 95% CI | t | paired win rate |\n");
        out.push_str("| --- | --- | --- | --- | --- | --- |\n");
        for delta in capacity_deltas {
            let _ = writeln!(
                out,
                "| `{}` | `{}` | {} | [{}, {}] | {:.2} | {} |",
                delta.treatment,
                delta.baseline,
                fixed(delta.mean),
                fixed(delta.ci95_low),
                fixed(delta.ci95_high),
                delta.t_stat,
                pct(delta.paired_win_rate)
            );
        }
        out.push_str(
            "\n差为正表示该 concentrated 仓位的容量缺口大于 full-range。\
             full-range 的缺口期望为 0，因此这一列基本就是 concentrated 自身的缺口。\n",
        );
    }

    out.push_str("\n### 候选报价诊断（**不得用于经济结论**）\n\n");
    out.push_str(
        "以下是**候选报价级**计数：router 与 arbitrageur 对同一笔订单会搜索多个候选成交量，\
         每个候选都调用一次曲线。某个候选被拒与该订单最终全额成交**可以同时成立**，\
         因此这些数字随搜索强度变化，只能用于诊断搜索行为。金额按代币分列，不相加、不折算。\n\n",
    );
    out.push_str("| 策略 | 候选拒绝数 | 零售 / 套利 | 买X / 卖X | 被拒请求 Y 均值 | 被拒请求 X 均值 | upstream 会剩 Y | upstream 会剩 X |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for s in &v3 {
        let Some(m) = &s.univ3 else { continue };
        let _ = writeln!(
            out,
            "| `{}` | {} | {} / {} | {} / {} | {} | {} | {} | {} |",
            s.strategy_id,
            m.total_capacity_quote_rejects,
            m.total_retail_quote_rejects,
            m.total_arb_quote_rejects,
            m.total_quote_rejects_buy_x,
            m.total_quote_rejects_sell_x,
            fixed(m.quote_rejected_requested_y.mean),
            fixed(m.quote_rejected_requested_x.mean),
            fixed(m.quote_canonical_unfilled_y.mean),
            fixed(m.quote_canonical_unfilled_x.mean)
        );
    }
    let _ = writeln!(out, "\n{UNIV3_QUOTE_PROBE_NOTE_ZH}");

    out.push_str("\n### 公允价区间占用\n\n");
    out.push_str("| 策略 | 出界步数占比 均值 | P95 | 有效流动性步数占比 均值 | 首次出界步 均值 | 曾出界的 seed 数 |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- |\n");
    for s in &v3 {
        let Some(m) = &s.univ3 else { continue };
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} / {} |",
            s.strategy_id,
            pct(m.fair_price_out_of_range_rate.mean),
            pct(m.fair_price_out_of_range_rate.p95),
            pct(m.active_liquidity_rate.mean),
            fixed(m.first_out_of_range_step.mean),
            m.seeds_that_left_range,
            s.simulations
        );
    }
    let _ = writeln!(out, "\n{UNIV3_RANGE_NOTE_ZH}");
    out.push_str(
        "\n仓位一次铸造后**从不再平衡**，这是该敏感性实验的前提，不是遗漏。\
         区间宽度按**边际价格冲击**匹配对照组的 `concentration`（同等投入资本），\
         逐条策略的实际达成倍数写在 `参数` 列里，未四舍五入抹掉。\n",
    );
}

/// Write every artefact into `dir`.
pub fn write_all(
    dir: &Path,
    meta: &RunMeta,
    artefacts: &Artefacts<'_>,
) -> std::io::Result<Vec<PathBuf>> {
    let Artefacts {
        summaries,
        runs,
        flashbots_minus_dodo,
        versus_univ2,
        attributions,
        univ3_attributions,
        versus_univ3,
        univ3_minus_univ2,
        univ3_capacity_deltas,
        quote_rows,
        univ3_sanity,
    } = *artefacts;
    fs::create_dir_all(dir)?;
    let mut written = Vec::new();

    let mut all_deltas: Vec<PairedDelta> = flashbots_minus_dodo.to_vec();
    all_deltas.extend_from_slice(versus_univ2);
    all_deltas.extend_from_slice(versus_univ3);
    all_deltas.extend_from_slice(univ3_capacity_deltas);

    // The baseline report carries both baselines and the sanity residual.
    let mut baseline_deltas: Vec<PairedDelta> = versus_univ2.to_vec();
    baseline_deltas.extend_from_slice(versus_univ3);
    baseline_deltas.extend_from_slice(univ3_minus_univ2);

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
            "REPORT-dodo-vs-flashbots.zh-CN.md",
            report_focus::dodo_vs_flashbots(meta, summaries, flashbots_minus_dodo, quote_rows),
        ),
        (
            "dodo-vs-flashbots.csv",
            report_focus::dodo_vs_flashbots_csv(flashbots_minus_dodo),
        ),
        (
            "dodo-vs-flashbots.json",
            report_focus::dodo_vs_flashbots_json(meta, flashbots_minus_dodo),
        ),
        (
            "REPORT-vs-baselines.zh-CN.md",
            report_focus::vs_baselines(
                meta,
                summaries,
                versus_univ2,
                versus_univ3,
                univ3_minus_univ2,
                univ3_sanity,
                attributions,
            ),
        ),
        (
            "versus-baselines.csv",
            report_focus::vs_baselines_csv(&baseline_deltas),
        ),
        (
            "versus-baselines.json",
            report_focus::vs_baselines_json(meta, &baseline_deltas, attributions),
        ),
        (
            "REPORT.zh-CN.md",
            markdown_zh(
                meta,
                summaries,
                flashbots_minus_dodo,
                versus_univ2,
                attributions,
                univ3_capacity_deltas,
            ),
        ),
    ];
    for (name, body) in files {
        let path = dir.join(name);
        fs::write(&path, body)?;
        written.push(path);
    }

    // Written only when the run actually contained V3 arms, so an empty file is
    // never mistaken for "measured, and there was nothing".
    if summaries.iter().any(|s| s.univ3.is_some()) {
        let path = dir.join("univ3-capacity.csv");
        fs::write(&path, univ3_capacity_csv(summaries))?;
        written.push(path);
    }
    if !univ3_attributions.is_empty() {
        let path = dir.join("univ3-attribution.csv");
        fs::write(&path, univ3_attribution_csv(univ3_attributions))?;
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
                univ3: None,
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
            provenance: crate::provenance::Provenance {
                binary_commit: "0".repeat(40),
                binary_dirty: false,
                run_start: crate::provenance::GitState {
                    commit: "0".repeat(40),
                    dirty: false,
                },
                run_end: crate::provenance::GitState {
                    commit: "0".repeat(40),
                    dirty: false,
                },
            },
            strategy_set: "legacy".to_string(),
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
        let markdown = markdown_zh(&meta(), &summaries, &[], &[], &[], &[]);
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
        let markdown = markdown_zh(&meta(), &summaries, &[], &[], &[], &[]);

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
        assert!(
            markdown.contains(&"0".repeat(40)),
            "the binary commit must appear"
        );
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
            Some("0".repeat(40).as_str())
        );
        // The three captures must all be present and named for what they are.
        for (field, expected) in [
            ("binaryCommit", "0".repeat(40)),
            ("runStartCommit", "0".repeat(40)),
            ("runEndCommit", "0".repeat(40)),
        ] {
            assert_eq!(
                provenance.get(field).unwrap().as_str(),
                Some(expected.as_str()),
                "{field}"
            );
        }
        for field in [
            "binaryWorkingTreeDirty",
            "runStartWorkingTreeDirty",
            "runEndWorkingTreeDirty",
        ] {
            assert_eq!(
                provenance.get(field).unwrap().as_bool(),
                Some(false),
                "{field}"
            );
        }
        assert_eq!(
            provenance.get("provenanceMatch").unwrap().as_bool(),
            Some(true)
        );
        // benchmarkCommit must be the BINARY's commit, and say so.
        assert!(provenance
            .get("benchmarkCommitIs")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("BINARY"));
        // The old single-capture field must be gone: it invited exactly the
        // mis-stamping this replaced.
        assert!(
            provenance.get("benchmarkWorkingTreeDirty").is_none(),
            "the ambiguous single dirty flag must not come back"
        );
        assert_eq!(json.get("publishable").unwrap().as_bool(), Some(true));
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
