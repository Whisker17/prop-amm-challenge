//! Paired (per-seed) difference statistics.
//!
//! Every strategy runs on the same seed set, so the honest comparison is the
//! per-seed difference, not the difference of the marginal means: the paired
//! difference cancels the shared price path and order flow and is what carries a
//! usable confidence interval.
//!
//! Reported per difference: mean, P5 / P50 / P95, standard error, a 95%
//! confidence interval (normal approximation on the paired differences, i.e.
//! `mean ± 1.96 · SE`, adequate at n = 1000), the t statistic against zero, and
//! the paired win rate — the fraction of seeds on which the treatment beat the
//! baseline. A near-50% paired win rate with |t| below ~2 means the two curves
//! are indistinguishable on that metric, regardless of how the marginal means
//! happen to rank.

use crate::metrics::{Distribution, RunMetrics};
use crate::strategies::{Family, Strategy};

/// Which per-run quantity a paired difference is taken over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    NetEdge,
    RetailEdge,
    ArbitrageEdge,
    /// Uniswap V3 only: the order-level capacity shortfall, in Y at each
    /// order's own step price. Absent from [`Metric::all`] because the other
    /// families have no such quantity — comparing a V3 arm against UniV2 on it
    /// would be comparing a number against nothing.
    RetailCapacityShortfallY,
}

impl Metric {
    pub fn as_str(self) -> &'static str {
        match self {
            Metric::NetEdge => "netEdge",
            Metric::RetailEdge => "retailEdge",
            Metric::ArbitrageEdge => "arbitrageEdge",
            Metric::RetailCapacityShortfallY => "retailCapacityShortfallNotionalY",
        }
    }

    /// `NaN` when the run carries no value for this metric, so a missing
    /// measurement can never be averaged in as a zero.
    pub fn extract(self, run: &RunMetrics) -> f64 {
        match self {
            Metric::NetEdge => run.net_edge,
            Metric::RetailEdge => run.retail_edge,
            Metric::ArbitrageEdge => run.arbitrage_edge,
            Metric::RetailCapacityShortfallY => run
                .univ3
                .map(|m| m.retail_capacity_shortfall_notional_y)
                .unwrap_or(f64::NAN),
        }
    }

    /// The three economic metrics every family reports.
    pub fn all() -> [Metric; 3] {
        [Metric::NetEdge, Metric::RetailEdge, Metric::ArbitrageEdge]
    }
}

/// One paired difference `treatment - baseline`, seed by seed.
#[derive(Debug, Clone)]
pub struct PairedDelta {
    pub treatment: String,
    pub baseline: String,
    /// Row of the `K` / `concentration` pairing table, when both sides share one.
    pub pairing_index: Option<usize>,
    pub metric: &'static str,
    pub samples: usize,
    pub mean: f64,
    pub distribution: Distribution,
    pub std_error: f64,
    pub ci95_low: f64,
    pub ci95_high: f64,
    pub t_stat: f64,
    /// Fraction of seeds where `treatment > baseline`.
    pub paired_win_rate: f64,
}

impl PairedDelta {
    /// True when the 95% interval excludes zero.
    pub fn is_significant(&self) -> bool {
        (self.ci95_low > 0.0 || self.ci95_high < 0.0) && self.samples > 1
    }
}

/// 1.96 — the normal quantile for a two-sided 95% interval.
pub const Z95: f64 = 1.959_963_984_540_054;

fn paired_samples(treatment: &[RunMetrics], baseline: &[RunMetrics], metric: Metric) -> Vec<f64> {
    // Runs are produced in seed order for every strategy, but pair on the seed
    // itself rather than on position so a filtered batch cannot silently
    // mis-align.
    let mut baseline_by_seed: Vec<(u64, f64)> = baseline
        .iter()
        .map(|run| (run.seed, metric.extract(run)))
        .collect();
    baseline_by_seed.sort_by_key(|(seed, _)| *seed);

    treatment
        .iter()
        .filter_map(|run| {
            let value = metric.extract(run);
            baseline_by_seed
                .binary_search_by_key(&run.seed, |(seed, _)| *seed)
                .ok()
                .map(|index| value - baseline_by_seed[index].1)
        })
        // A metric that one side does not report yields NaN, which must drop out
        // rather than poison the mean. Dropping is visible: `samples` shrinks.
        .filter(|difference| difference.is_finite())
        .collect()
}

/// Compute one paired difference. `None` when the two runs share no seeds.
pub fn paired_delta(
    treatment: &Strategy,
    treatment_runs: &[RunMetrics],
    baseline: &Strategy,
    baseline_runs: &[RunMetrics],
    metric: Metric,
) -> Option<PairedDelta> {
    let samples = paired_samples(treatment_runs, baseline_runs, metric);
    if samples.is_empty() {
        return None;
    }
    let n = samples.len();
    let mean = samples.iter().sum::<f64>() / n as f64;
    let variance = if n > 1 {
        samples.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64
    } else {
        0.0
    };
    let std_error = if n > 1 {
        (variance / n as f64).sqrt()
    } else {
        f64::NAN
    };
    let wins = samples.iter().filter(|v| **v > 0.0).count();

    Some(PairedDelta {
        treatment: treatment.id.clone(),
        baseline: baseline.id.clone(),
        pairing_index: match (treatment.pairing_index, baseline.pairing_index) {
            (Some(a), Some(b)) if a == b => Some(a),
            _ => None,
        },
        metric: metric.as_str(),
        samples: n,
        mean,
        distribution: Distribution::from_samples(&samples),
        std_error,
        ci95_low: mean - Z95 * std_error,
        ci95_high: mean + Z95 * std_error,
        t_stat: if std_error > 0.0 {
            mean / std_error
        } else {
            f64::NAN
        },
        paired_win_rate: wins as f64 / n as f64,
    })
}

fn find<'a>(
    results: &'a [(Strategy, Vec<RunMetrics>)],
    id: &str,
) -> Option<(&'a Strategy, &'a [RunMetrics])> {
    results
        .iter()
        .find(|(strategy, _)| strategy.id == id)
        .map(|(strategy, runs)| (strategy, runs.as_slice()))
}

/// `Flashbots - DODO` on every row of the pairing table, for every metric.
pub fn flashbots_minus_dodo(results: &[(Strategy, Vec<RunMetrics>)]) -> Vec<PairedDelta> {
    let mut deltas = Vec::new();
    for (flashbots, flashbots_runs) in results
        .iter()
        .filter(|(s, _)| s.family == Family::Flashbots)
    {
        let Some(index) = flashbots.pairing_index else {
            continue;
        };
        let Some((dodo, dodo_runs)) = results
            .iter()
            .find(|(s, _)| s.family == Family::Dodo && s.pairing_index == Some(index))
            .map(|(s, runs)| (s, runs.as_slice()))
        else {
            continue;
        };
        for metric in Metric::all() {
            if let Some(delta) = paired_delta(flashbots, flashbots_runs, dodo, dodo_runs, metric) {
                deltas.push(delta);
            }
        }
    }
    deltas.sort_by_key(|delta| (delta.pairing_index.unwrap_or(usize::MAX), delta.metric));
    deltas
}

/// Every strategy against the zero-fee Uniswap V2 baseline.
pub fn versus_univ2(results: &[(Strategy, Vec<RunMetrics>)]) -> Vec<PairedDelta> {
    let Some((univ2, univ2_runs)) = find(results, UNIV2_ID) else {
        return Vec::new();
    };
    let mut deltas = Vec::new();
    for (strategy, runs) in results {
        if strategy.id == univ2.id {
            continue;
        }
        for metric in Metric::all() {
            if let Some(delta) = paired_delta(strategy, runs, univ2, univ2_runs, metric) {
                deltas.push(delta);
            }
        }
    }
    deltas
}

pub const UNIV2_ID: &str = "univ2-zero-fee";
/// Oracle-aware anchors whose curve *shape* coincides with a zero-fee constant
/// product at the opening inventory.
pub const DODO_ANCHOR_ID: &str = "dodo-k1000000000000000000";
pub const FLASHBOTS_ANCHOR_ID: &str = "flashbots-c1";

/// Attribution of a strategy's advantage over zero-fee Uniswap V2.
///
/// The two components are measured against an **anchor**: the oracle-aware
/// strategy in the same family whose curve shape coincides with a zero-fee
/// constant product at the opening inventory (`DODO K = 1e18`, i.e. the
/// constant-product case of the PMM, and `Flashbots concentration = 1`, whose
/// virtual quote reserve equals the real one at `reserveX == targetX`). The
/// `quote-matrix` output shows those two quoting bit-identically to Uniswap V2
/// at that point.
///
/// * `oracle_system_advantage = mean(anchor - univ2)` — same curve shape at the
///   opening inventory, so what remains is the effect of repricing on the
///   published oracle.
/// * `curve_shape_effect = mean(strategy - anchor)` — same oracle, so what
///   remains is the effect of the curve parameter.
/// * `total_vs_univ2 = mean(strategy - univ2)`, which equals the sum exactly,
///   because means of paired differences over one seed set are linear.
///
/// The decomposition is anchored at the opening inventory. It is **not** a claim
/// that an oracle-aware curve equals constant product away from that point: once
/// the oracle moves, the anchor reprices and a passive constant product does not.
/// Only means decompose; percentiles do not, and are therefore not split.
#[derive(Debug, Clone)]
pub struct Attribution {
    pub strategy: String,
    pub family: String,
    pub parameter: String,
    pub anchor: String,
    pub metric: &'static str,
    pub oracle_system_advantage: f64,
    pub curve_shape_effect: f64,
    pub total_vs_univ2: f64,
}

/// Split each oracle-aware strategy's advantage over Uniswap V2 into the
/// oracle-system component and the curve-shape component.
pub fn attribute_versus_univ2(results: &[(Strategy, Vec<RunMetrics>)]) -> Vec<Attribution> {
    let Some((univ2, univ2_runs)) = find(results, UNIV2_ID) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for (strategy, runs) in results {
        if strategy.id == univ2.id {
            continue;
        }
        let anchor_id = match strategy.family {
            Family::Dodo => DODO_ANCHOR_ID,
            Family::Flashbots => FLASHBOTS_ANCHOR_ID,
            // Uniswap V2 is the anchor itself, and the Uniswap V3 arms consume
            // no oracle at all, so an "oracle system advantage" is not a
            // quantity that exists for them. They are decomposed separately, by
            // [`attribute_univ3_concentration`], rather than being pushed
            // through a split whose first term would be misnamed.
            Family::UniV2 | Family::UniV3FullRange | Family::UniV3Concentrated => continue,
        };
        let Some((anchor, anchor_runs)) = find(results, anchor_id) else {
            continue;
        };

        for metric in Metric::all() {
            let Some(system) = paired_delta(anchor, anchor_runs, univ2, univ2_runs, metric) else {
                continue;
            };
            let Some(shape) = paired_delta(strategy, runs, anchor, anchor_runs, metric) else {
                continue;
            };
            let Some(total) = paired_delta(strategy, runs, univ2, univ2_runs, metric) else {
                continue;
            };
            out.push(Attribution {
                strategy: strategy.id.clone(),
                family: strategy.family.as_str().to_string(),
                parameter: strategy.parameter.clone(),
                anchor: anchor.id.clone(),
                metric: metric.as_str(),
                oracle_system_advantage: system.mean,
                curve_shape_effect: shape.mean,
                total_vs_univ2: total.mean,
            });
        }
    }
    out
}

/// The Uniswap V3 full-range arm, which anchors the V3 decomposition.
pub const UNIV3_FULL_RANGE_ID: &str = "univ3-full-range-zero-fee";

/// Every strategy against the zero-fee **full-range Uniswap V3** baseline.
///
/// Empty when the run did not include the V3 arms. The V3 full-range arm itself
/// is skipped, as is any metric a strategy does not report.
pub fn versus_univ3_full_range(results: &[(Strategy, Vec<RunMetrics>)]) -> Vec<PairedDelta> {
    let Some((univ3, univ3_runs)) = find(results, UNIV3_FULL_RANGE_ID) else {
        return Vec::new();
    };
    let mut deltas = Vec::new();
    for (strategy, runs) in results {
        if strategy.id == univ3.id {
            continue;
        }
        for metric in Metric::all() {
            if let Some(delta) = paired_delta(strategy, runs, univ3, univ3_runs, metric) {
                deltas.push(delta);
            }
        }
    }
    deltas
}

/// `full-range V3 − UniV2`, per metric: the sanity gate.
///
/// Both are passive, zero-fee and hold the same opening capital, so the expected
/// difference is zero. It is **not** exactly zero and must not be asserted to be:
/// the V3 port is a different integer implementation on a finite tick domain,
/// and both sides are quantised to nano at the adapter boundary. What this
/// returns is the residual, for reporting against a stated tolerance.
pub fn univ3_full_range_minus_univ2(results: &[(Strategy, Vec<RunMetrics>)]) -> Vec<PairedDelta> {
    let (Some((univ3, univ3_runs)), Some((univ2, univ2_runs))) =
        (find(results, UNIV3_FULL_RANGE_ID), find(results, UNIV2_ID))
    else {
        return Vec::new();
    };
    Metric::all()
        .into_iter()
        .filter_map(|metric| paired_delta(univ3, univ3_runs, univ2, univ2_runs, metric))
        .collect()
}

/// The full-range V3 sanity gate: residuals against UniV2, with a derived bound.
///
/// Both arms are passive, zero-fee, and hold the same opening capital, so their
/// per-seed edges should agree — but **not exactly**, and this deliberately does
/// not assert that they do. Two real sources of residual are already measured:
///
/// * from the *same* state, the two formulas differ by up to **1 693 wei** on a
///   single quote (`univ3_vs_univ2_continuous`), because V3 derives the output
///   through `sqrtPriceX96` while V2 uses the reserve ratio directly;
/// * once the two are allowed to run independently, their states drift by up to
///   **484 wei** over 400 swaps in the same test.
///
/// Both are far below the adapter's own quantisation, which is the term that
/// actually dominates: every quote leaves the integer domain through a floor to
/// nano (`1e-9`), so a single fill can differ by up to one nano of the received
/// token. Marked at the fair price that is `1e-9 · max(1, price)` per trade, and
/// a run does `retail_trade_count + arb_count` trades. [`derived_tolerance`]
/// is exactly that product — no fitted constant.
#[derive(Debug, Clone)]
pub struct Univ3SanityGate {
    /// Per-seed `V3 − V2` net-edge residuals, aligned by seed.
    pub residuals: Distribution,
    pub samples: usize,
    /// Largest `|residual|` seen.
    pub max_abs_residual: f64,
    /// Largest tolerance derived across the seeds.
    pub tolerance: f64,
    /// That tolerance broken into its terms, for the seed that produced it.
    pub tolerance_terms: ToleranceTerms,
    /// Seeds whose `|residual|` exceeded their own derived tolerance.
    pub seeds_over_tolerance: usize,
    /// Capacity rejections recorded by the full-range arm. Must be zero: a
    /// full-range position spans the whole tick domain, so nothing can be
    /// refused for want of room, and a non-zero value means the range or the
    /// adapter is wrong rather than that the market moved.
    pub full_range_capacity_limited_orders: u64,
}

impl Univ3SanityGate {
    /// Whether every seed stayed inside its derived tolerance and the
    /// full-range arm refused nothing.
    pub fn passed(&self) -> bool {
        self.seeds_over_tolerance == 0 && self.full_range_capacity_limited_orders == 0
    }

    pub fn summary_line(&self) -> String {
        format!(
            "full-range V3 sanity gate: {} — max |netEdge residual vs UniV2| {:.3e} over {} seeds, \
             derived tolerance {:.3e}, {} seed(s) over tolerance, {} capacity-limited order(s)",
            if self.passed() { "PASS" } else { "FAIL" },
            self.max_abs_residual,
            self.samples,
            self.tolerance,
            self.seeds_over_tolerance,
            self.full_range_capacity_limited_orders
        )
    }
}

/// Golden-section stopping tolerance on the router's split fraction.
///
/// Mirrors `GOLDEN_ALPHA_TOL` in `prop_amm_sim::router`, which is private. If
/// that constant changes, this bound is wrong — `the_sanity_tolerance_terms_are_ordered`
/// is the tripwire.
pub const ROUTER_ALPHA_TOL: f64 = 1e-3;

/// Golden-section stopping tolerance on the arbitrageur's input size, relative.
/// Mirrors `GOLDEN_INPUT_REL_TOL` in `prop_amm_sim::arbitrageur`.
pub const ARB_INPUT_REL_TOL: f64 = 1e-2;

/// One nano, the adapter's quantisation step.
pub const NANO: f64 = 1e-9;

/// The three terms of the sanity tolerance, kept apart so the report can say
/// which one dominates instead of quoting one opaque number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToleranceTerms {
    /// Floor-to-nano on every fill, marked at the fair price.
    pub quantisation: f64,
    /// The arbitrageur's size search, which stops at a relative tolerance.
    pub arb_search: f64,
    /// The router's split search, which stops at an absolute tolerance on the
    /// split fraction.
    pub router_search: f64,
}

impl ToleranceTerms {
    pub fn total(&self) -> f64 {
        self.quantisation + self.arb_search + self.router_search
    }
}

/// Tolerance for the full-range V3 vs UniV2 residual, derived term by term.
///
/// **Quantisation.** Every quote leaves the integer domain through a floor to
/// nano, so one fill can differ by up to one nano of the received token. Marked
/// at the fair price that is `1e-9 · max(1, price)` per trade, over
/// `retail_trade_count + arb_count` trades.
///
/// **Search.** This is the term that actually dominates, and it is not a
/// property of either curve: the simulation's arbitrageur resolves its trade
/// size only to [`ARB_INPUT_REL_TOL`] (1% relative) and the router resolves its
/// split only to [`ROUTER_ALPHA_TOL`] (1e-3 absolute). A one-wei difference in a
/// quote can therefore land the golden-section search on a different point
/// inside its own stopping window. Both searches maximise a smooth objective, so
/// at the optimum the gradient vanishes and a displacement `δ` costs `O(δ²)` of
/// the objective — whose scale is the flow's notional. Hence
/// `arb_notional · ARB_INPUT_REL_TOL²` and `retail_notional · ROUTER_ALPHA_TOL²`.
///
/// Measured at the time of writing over 8 seeds, the quantisation term is two to
/// three orders of magnitude below the observed residual while the arb-search
/// term is one to two orders above it:
///
/// | steps | observed max residual | quantisation | arb search |
/// | --- | --- | --- | --- |
/// | 300 | 5.3e-3 | 4.4e-5 | 7.1e-1 |
/// | 1 000 | 9.0e-2 | 1.6e-4 | 2.7e0 |
/// | 3 000 | 7.8e-2 | 4.6e-4 | 7.2e0 |
///
/// The bound is therefore conservative by roughly 30x-130x. It is kept anyway
/// because it is derived rather than fitted and it scales with the run: a
/// formula regression large enough to matter is far larger than this.
pub fn tolerance_terms(run: &RunMetrics) -> ToleranceTerms {
    let trades = (run.retail_trade_count + run.arb_count) as f64;
    ToleranceTerms {
        quantisation: trades * NANO * run.final_fair_price.max(1.0),
        arb_search: run.arb_notional * ARB_INPUT_REL_TOL * ARB_INPUT_REL_TOL,
        router_search: run.retail_notional * ROUTER_ALPHA_TOL * ROUTER_ALPHA_TOL,
    }
}

/// [`tolerance_terms`] summed.
pub fn derived_tolerance(run: &RunMetrics) -> f64 {
    tolerance_terms(run).total()
}

/// Build the sanity gate. `None` when the run had no V3 full-range arm.
pub fn univ3_full_range_sanity(results: &[(Strategy, Vec<RunMetrics>)]) -> Option<Univ3SanityGate> {
    let (univ3, univ3_runs) = find(results, UNIV3_FULL_RANGE_ID)?;
    let (_, univ2_runs) = find(results, UNIV2_ID)?;
    let _ = univ3;

    let mut by_seed: Vec<(u64, f64)> = univ2_runs
        .iter()
        .map(|run| (run.seed, run.net_edge))
        .collect();
    by_seed.sort_by_key(|(seed, _)| *seed);

    let mut residuals = Vec::new();
    let mut max_abs = 0.0_f64;
    let mut tolerance = 0.0_f64;
    let mut terms = ToleranceTerms {
        quantisation: 0.0,
        arb_search: 0.0,
        router_search: 0.0,
    };
    let mut over = 0usize;
    for run in univ3_runs {
        let Ok(index) = by_seed.binary_search_by_key(&run.seed, |(seed, _)| *seed) else {
            continue;
        };
        let residual = run.net_edge - by_seed[index].1;
        if !residual.is_finite() {
            continue;
        }
        let seed_terms = tolerance_terms(run);
        let seed_tolerance = seed_terms.total();
        if residual.abs() > seed_tolerance {
            over += 1;
        }
        max_abs = max_abs.max(residual.abs());
        if seed_tolerance > tolerance {
            tolerance = seed_tolerance;
            terms = seed_terms;
        }
        residuals.push(residual);
    }
    if residuals.is_empty() {
        return None;
    }

    Some(Univ3SanityGate {
        samples: residuals.len(),
        residuals: Distribution::from_samples(&residuals),
        max_abs_residual: max_abs,
        tolerance,
        tolerance_terms: terms,
        seeds_over_tolerance: over,
        full_range_capacity_limited_orders: univ3_runs
            .iter()
            .filter_map(|run| run.univ3)
            .map(|m| m.retail_full_order_capacity_limited_count)
            .sum(),
    })
}

/// `concentrated V3 − full-range V3` on the order-level capacity shortfall.
///
/// Both sides report the metric, which is why this comparison is legitimate and
/// the same comparison against UniV2 is not.
pub fn univ3_capacity_deltas(results: &[(Strategy, Vec<RunMetrics>)]) -> Vec<PairedDelta> {
    let Some((full_range, full_range_runs)) = find(results, UNIV3_FULL_RANGE_ID) else {
        return Vec::new();
    };
    results
        .iter()
        .filter(|(s, _)| s.family == Family::UniV3Concentrated)
        .filter_map(|(strategy, runs)| {
            paired_delta(
                strategy,
                runs,
                full_range,
                full_range_runs,
                Metric::RetailCapacityShortfallY,
            )
        })
        .collect()
}

/// The Uniswap V3 split, which deliberately has different terms from
/// [`Attribution`] because V3 consumes no oracle.
///
/// * `full_range_vs_univ2 = mean(univ3-full-range − univ2)` — two passive
///   zero-fee constant-product curves on the same capital. This is an
///   **implementation and adapter term**, not an economic effect: it carries the
///   V3 integer path, the fill-or-kill policy and the nano quantisation. It is
///   reported so that the concentration term below is not credited with it.
/// * `concentration_effect = mean(strategy − univ3-full-range)` — same curve
///   family, same oracle situation (none), so what remains is the effect of
///   concentrating the same capital into a finite, never-rebalanced range.
/// * `total_vs_univ2 = mean(strategy − univ2)`, exactly the sum.
#[derive(Debug, Clone)]
pub struct Univ3Attribution {
    pub strategy: String,
    pub family: String,
    pub parameter: String,
    pub anchor: String,
    pub metric: &'static str,
    pub full_range_vs_univ2: f64,
    pub concentration_effect: f64,
    pub total_vs_univ2: f64,
}

/// Split each concentrated V3 arm's difference from Uniswap V2 into the
/// full-range baseline term and the concentration term.
///
/// Returns empty when the run did not include the V3 arms.
pub fn attribute_univ3_concentration(
    results: &[(Strategy, Vec<RunMetrics>)],
) -> Vec<Univ3Attribution> {
    let (Some((univ2, univ2_runs)), Some((full_range, full_range_runs))) =
        (find(results, UNIV2_ID), find(results, UNIV3_FULL_RANGE_ID))
    else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for (strategy, runs) in results {
        if strategy.family != Family::UniV3Concentrated {
            continue;
        }
        for metric in Metric::all() {
            let Some(baseline) =
                paired_delta(full_range, full_range_runs, univ2, univ2_runs, metric)
            else {
                continue;
            };
            let Some(concentration) =
                paired_delta(strategy, runs, full_range, full_range_runs, metric)
            else {
                continue;
            };
            let Some(total) = paired_delta(strategy, runs, univ2, univ2_runs, metric) else {
                continue;
            };
            out.push(Univ3Attribution {
                strategy: strategy.id.clone(),
                family: strategy.family.as_str().to_string(),
                parameter: strategy.parameter.clone(),
                anchor: full_range.id.clone(),
                metric: metric.as_str(),
                full_range_vs_univ2: baseline.mean,
                concentration_effect: concentration.mean,
                total_vs_univ2: total.mean,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::strategy_by_id;

    fn run(strategy: &str, seed: u64, net: f64, retail: f64, arb: f64) -> RunMetrics {
        RunMetrics {
            strategy_id: strategy.to_string(),
            seed,
            steps: 10,
            retail_edge: retail,
            arbitrage_edge: arb,
            arbitrage_loss: if arb < 0.0 { -arb } else { 0.0 },
            net_edge: net,
            retail_notional: 1.0,
            competitor_retail_notional: 1.0,
            retail_flow_share: 0.5,
            retail_trade_count: 1,
            arb_count: 1,
            arb_notional: 1.0,
            final_inventory_deviation: 0.0,
            max_inventory_deviation: 0.0,
            final_reserve_x: 100.0,
            final_reserve_y: 10_000.0,
            final_fair_price: 100.0,
            curve_revert_count: 0,
            univ3: None,
        }
    }

    fn results(flashbots: &[f64], dodo: &[f64], univ2: &[f64]) -> Vec<(Strategy, Vec<RunMetrics>)> {
        let fb = strategy_by_id(FLASHBOTS_ANCHOR_ID).unwrap();
        let dd = strategy_by_id(DODO_ANCHOR_ID).unwrap();
        let uv = strategy_by_id(UNIV2_ID).unwrap();
        vec![
            (
                fb.clone(),
                flashbots
                    .iter()
                    .enumerate()
                    .map(|(i, v)| run(&fb.id, i as u64, *v, *v, 0.0))
                    .collect(),
            ),
            (
                dd.clone(),
                dodo.iter()
                    .enumerate()
                    .map(|(i, v)| run(&dd.id, i as u64, *v, *v, 0.0))
                    .collect(),
            ),
            (
                uv.clone(),
                univ2
                    .iter()
                    .enumerate()
                    .map(|(i, v)| run(&uv.id, i as u64, *v, *v, 0.0))
                    .collect(),
            ),
        ]
    }

    #[test]
    fn paired_difference_cancels_the_shared_component() {
        // Treatment is baseline plus a constant, drowned in shared noise: the
        // marginal means are nearly useless, the paired difference is exact.
        let shared = [10.0, -30.0, 55.0, -12.0, 7.0, 90.0, -66.0, 3.0];
        let baseline: Vec<f64> = shared.to_vec();
        let treatment: Vec<f64> = shared.iter().map(|v| v + 2.0).collect();
        let results = results(&treatment, &baseline, &baseline);

        let deltas = flashbots_minus_dodo(&results);
        let net = deltas
            .iter()
            .find(|d| d.metric == "netEdge")
            .expect("net edge delta");
        assert_eq!(net.samples, 8);
        assert!((net.mean - 2.0).abs() < 1e-12);
        assert!(net.std_error < 1e-12, "a constant offset has no dispersion");
        assert_eq!(net.paired_win_rate, 1.0);
        assert!(net.is_significant());
    }

    #[test]
    fn an_indistinguishable_pair_is_reported_as_such() {
        // Same distribution, alternating sign: mean ~0, win rate ~50%.
        let baseline: Vec<f64> = (0..100).map(|i| (i as f64) * 0.1).collect();
        let treatment: Vec<f64> = baseline
            .iter()
            .enumerate()
            .map(|(i, v)| if i % 2 == 0 { v + 1.0 } else { v - 1.0 })
            .collect();
        let results = results(&treatment, &baseline, &baseline);
        let deltas = flashbots_minus_dodo(&results);
        let net = deltas.iter().find(|d| d.metric == "netEdge").unwrap();

        assert!(net.mean.abs() < 1e-12);
        assert_eq!(net.paired_win_rate, 0.5);
        assert!(
            !net.is_significant(),
            "a zero-mean difference must not read as real"
        );
        assert!(net.ci95_low < 0.0 && net.ci95_high > 0.0);
    }

    #[test]
    fn pairing_is_by_seed_not_by_position() {
        let fb = strategy_by_id(FLASHBOTS_ANCHOR_ID).unwrap();
        let dd = strategy_by_id(DODO_ANCHOR_ID).unwrap();
        // Same seeds, opposite order, values chosen so a positional pairing
        // would give a different answer.
        let treatment = vec![run(&fb.id, 0, 1.0, 1.0, 0.0), run(&fb.id, 1, 2.0, 2.0, 0.0)];
        let baseline = vec![
            run(&dd.id, 1, 20.0, 20.0, 0.0),
            run(&dd.id, 0, 10.0, 10.0, 0.0),
        ];
        let delta = paired_delta(&fb, &treatment, &dd, &baseline, Metric::NetEdge).unwrap();
        assert_eq!(delta.samples, 2);
        assert!((delta.mean - (-13.5)).abs() < 1e-12, "got {}", delta.mean);
    }

    #[test]
    fn attribution_components_sum_to_the_total() {
        let univ2 = vec![-10.0, -12.0, -9.0, -11.0];
        let anchor = vec![1.0, 2.0, 0.5, 1.5];
        let results = results(&anchor, &anchor, &univ2);
        let attributions = attribute_versus_univ2(&results);
        assert!(!attributions.is_empty());
        for attribution in attributions {
            let sum = attribution.oracle_system_advantage + attribution.curve_shape_effect;
            assert!(
                (sum - attribution.total_vs_univ2).abs() < 1e-9,
                "{}: {} + {} != {}",
                attribution.strategy,
                attribution.oracle_system_advantage,
                attribution.curve_shape_effect,
                attribution.total_vs_univ2
            );
        }
    }

    #[test]
    fn missing_baseline_yields_no_deltas() {
        let fb = strategy_by_id(FLASHBOTS_ANCHOR_ID).unwrap();
        let empty: Vec<RunMetrics> = Vec::new();
        assert!(paired_delta(&fb, &empty, &fb, &empty, Metric::NetEdge).is_none());
        assert!(versus_univ2(&[]).is_empty());
        assert!(attribute_versus_univ2(&[]).is_empty());
    }
}
