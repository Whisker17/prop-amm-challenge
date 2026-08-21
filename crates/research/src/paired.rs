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
}

impl Metric {
    pub fn as_str(self) -> &'static str {
        match self {
            Metric::NetEdge => "netEdge",
            Metric::RetailEdge => "retailEdge",
            Metric::ArbitrageEdge => "arbitrageEdge",
        }
    }

    pub fn extract(self, run: &RunMetrics) -> f64 {
        match self {
            Metric::NetEdge => run.net_edge,
            Metric::RetailEdge => run.retail_edge,
            Metric::ArbitrageEdge => run.arbitrage_edge,
        }
    }

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
            Family::UniV2 => continue,
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
