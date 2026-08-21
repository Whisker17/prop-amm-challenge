//! Result records and their aggregation across paired simulations.
//!
//! Edge is always measured from the AMM's point of view, marked at the external
//! fair price, using the same expressions the existing simulation engine uses:
//!
//! * AMM sells X for Y: `edge = amount_y - amount_x * fair_price`
//! * AMM buys X with Y:  `edge = amount_x * fair_price - amount_y`
//!
//! so a positive edge is profit for the AMM. Retail and arbitrage edge are kept
//! separate, and `net_edge = retail_edge + arbitrage_edge`.

/// One simulation run of one strategy under one seed.
#[derive(Debug, Clone)]
pub struct RunMetrics {
    pub strategy_id: String,
    pub seed: u64,
    pub steps: u32,

    /// Edge earned from retail flow.
    pub retail_edge: f64,
    /// Edge from arbitrage flow (normally negative: the AMM pays the arbitrageur).
    pub arbitrage_edge: f64,
    /// `max(0, -arbitrage_edge)` — money lost to arbitrageurs, as a positive number.
    pub arbitrage_loss: f64,
    /// `retail_edge + arbitrage_edge`.
    pub net_edge: f64,

    /// Retail notional (in Y, marked at the fair price) captured by this strategy.
    pub retail_notional: f64,
    /// Retail notional captured by the competing AMM in the same run.
    pub competitor_retail_notional: f64,
    /// `retail_notional / (retail_notional + competitor_retail_notional)`.
    pub retail_flow_share: f64,
    pub retail_trade_count: u64,

    pub arb_count: u64,
    /// Arbitrage notional (in Y, marked at the fair price).
    pub arb_notional: f64,

    /// `(reserve_x - initial_x) / initial_x` at the end of the run.
    pub final_inventory_deviation: f64,
    /// Largest `|(reserve_x - initial_x) / initial_x|` seen during the run.
    pub max_inventory_deviation: f64,

    pub final_reserve_x: f64,
    pub final_reserve_y: f64,
    pub final_fair_price: f64,

    /// Curve calls that would have reverted on chain despite non-degenerate
    /// inputs. A healthy run reports zero; anything else means the ledger and
    /// the curve state disagreed and the run must not be trusted.
    pub curve_revert_count: u64,
}

/// Percentile summary of one metric across a paired batch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Distribution {
    pub mean: f64,
    pub p5: f64,
    pub p50: f64,
    pub p95: f64,
    pub min: f64,
    pub max: f64,
}

impl Distribution {
    pub fn from_samples(samples: &[f64]) -> Distribution {
        if samples.is_empty() {
            return Distribution {
                mean: f64::NAN,
                p5: f64::NAN,
                p50: f64::NAN,
                p95: f64::NAN,
                min: f64::NAN,
                max: f64::NAN,
            };
        }
        let mut sorted: Vec<f64> = samples.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
        Distribution {
            mean,
            p5: percentile(&sorted, 0.05),
            p50: percentile(&sorted, 0.50),
            p95: percentile(&sorted, 0.95),
            min: sorted[0],
            max: sorted[sorted.len() - 1],
        }
    }
}

/// Nearest-rank percentile on an ascending slice.
pub fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let n = sorted.len();
    let rank = (fraction * n as f64).ceil() as usize;
    let index = rank.saturating_sub(1).min(n - 1);
    sorted[index]
}

/// Aggregate of one strategy across the whole seed batch.
#[derive(Debug, Clone)]
pub struct StrategySummary {
    pub strategy_id: String,
    pub family: String,
    pub parameter: String,
    pub simulations: usize,
    pub steps: u32,

    pub net_edge: Distribution,
    pub retail_edge: Distribution,
    pub arbitrage_edge: Distribution,
    pub arbitrage_loss: Distribution,
    pub retail_flow_share: Distribution,
    pub retail_notional: Distribution,
    pub arb_count: Distribution,
    pub arb_notional: Distribution,
    pub final_inventory_deviation: Distribution,
    pub max_inventory_deviation: Distribution,

    /// Fraction of seeds with a positive net edge.
    pub win_rate: f64,
    pub total_net_edge: f64,
    /// Total curve reverts across the batch. Must be zero for a valid result.
    pub total_curve_reverts: u64,
}

impl StrategySummary {
    pub fn from_runs(
        strategy_id: &str,
        family: &str,
        parameter: &str,
        runs: &[RunMetrics],
    ) -> StrategySummary {
        let collect = |f: fn(&RunMetrics) -> f64| -> Vec<f64> { runs.iter().map(f).collect() };
        let net: Vec<f64> = collect(|r| r.net_edge);
        let positive = net.iter().filter(|v| **v > 0.0).count();
        StrategySummary {
            strategy_id: strategy_id.to_string(),
            family: family.to_string(),
            parameter: parameter.to_string(),
            simulations: runs.len(),
            steps: runs.first().map(|r| r.steps).unwrap_or(0),
            net_edge: Distribution::from_samples(&net),
            retail_edge: Distribution::from_samples(&collect(|r| r.retail_edge)),
            arbitrage_edge: Distribution::from_samples(&collect(|r| r.arbitrage_edge)),
            arbitrage_loss: Distribution::from_samples(&collect(|r| r.arbitrage_loss)),
            retail_flow_share: Distribution::from_samples(&collect(|r| r.retail_flow_share)),
            retail_notional: Distribution::from_samples(&collect(|r| r.retail_notional)),
            arb_count: Distribution::from_samples(&collect(|r| r.arb_count as f64)),
            arb_notional: Distribution::from_samples(&collect(|r| r.arb_notional)),
            final_inventory_deviation: Distribution::from_samples(&collect(|r| {
                r.final_inventory_deviation
            })),
            max_inventory_deviation: Distribution::from_samples(&collect(|r| {
                r.max_inventory_deviation
            })),
            win_rate: if runs.is_empty() {
                f64::NAN
            } else {
                positive as f64 / runs.len() as f64
            },
            total_net_edge: net.iter().sum(),
            total_curve_reverts: runs.iter().map(|r| r.curve_revert_count).sum(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_use_nearest_rank() {
        let sorted: Vec<f64> = (1..=100).map(|v| v as f64).collect();
        assert_eq!(percentile(&sorted, 0.05), 5.0);
        assert_eq!(percentile(&sorted, 0.50), 50.0);
        assert_eq!(percentile(&sorted, 0.95), 95.0);
        assert_eq!(percentile(&sorted, 0.0), 1.0);
        assert_eq!(percentile(&sorted, 1.0), 100.0);
    }

    #[test]
    fn distribution_of_a_single_sample_is_that_sample() {
        let d = Distribution::from_samples(&[3.5]);
        assert_eq!(d.mean, 3.5);
        assert_eq!(d.p5, 3.5);
        assert_eq!(d.p50, 3.5);
        assert_eq!(d.p95, 3.5);
        assert_eq!(d.min, 3.5);
        assert_eq!(d.max, 3.5);
    }

    #[test]
    fn empty_distribution_is_all_nan() {
        let d = Distribution::from_samples(&[]);
        assert!(d.mean.is_nan() && d.p50.is_nan());
    }

    fn run(net: f64) -> RunMetrics {
        RunMetrics {
            strategy_id: "s".into(),
            seed: 0,
            steps: 10,
            retail_edge: net + 1.0,
            arbitrage_edge: -1.0,
            arbitrage_loss: 1.0,
            net_edge: net,
            retail_notional: 100.0,
            competitor_retail_notional: 100.0,
            retail_flow_share: 0.5,
            retail_trade_count: 2,
            arb_count: 1,
            arb_notional: 10.0,
            final_inventory_deviation: 0.01,
            max_inventory_deviation: 0.02,
            final_reserve_x: 100.0,
            final_reserve_y: 10_000.0,
            final_fair_price: 100.0,
            curve_revert_count: 0,
        }
    }

    #[test]
    fn summary_counts_win_rate_and_totals() {
        let runs = vec![run(1.0), run(-2.0), run(3.0), run(0.0)];
        let summary = StrategySummary::from_runs("s", "dodo", "K=1e18", &runs);
        assert_eq!(summary.simulations, 4);
        assert_eq!(summary.win_rate, 0.5);
        assert_eq!(summary.total_net_edge, 2.0);
        assert_eq!(summary.total_curve_reverts, 0);
        assert_eq!(summary.net_edge.min, -2.0);
        assert_eq!(summary.net_edge.max, 3.0);
    }
}
