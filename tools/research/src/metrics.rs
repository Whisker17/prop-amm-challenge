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

    /// Uniswap V3 only. `None` for every other family, so a missing value is
    /// never confused with a measured zero.
    pub univ3: Option<Univ3RunMetrics>,
}

/// What a Uniswap V3 arm did that the other curves cannot do.
///
/// Three separate things are recorded and are **never** merged:
///
/// 1. **Quote-probe diagnostics** (`*_quote_reject_*`). The router and the
///    arbitrageur search over candidate trade sizes, calling the curve many
///    times per order. These fields count *refused candidate quotes*, so they
///    scale with how hard the search looked, not with how much flow was turned
///    away. They are **search diagnostics only and must not enter an economic
///    conclusion**, and they must never be described as rejected orders.
/// 2. **Order-level capacity** (`retail_*_capacity_*`). One canonical probe per
///    retail order, taken on the pre-route pool state at the full order size.
///    Deterministic and independent of the search, so this is the field that may
///    carry an economic reading.
/// 3. **Range occupancy**, sampled per step against the published fair price.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Univ3RunMetrics {
    // ---------------- 1. quote-probe diagnostics ----------------
    /// Candidate quotes the adapter refused for lack of room, over the run.
    ///
    /// **Not an order count.** A single retail order can produce many refusals
    /// while the router bisects towards a size that does fit, and the order may
    /// then be filled in full.
    pub capacity_quote_reject_count: u64,
    pub retail_quote_reject_count: u64,
    pub arb_quote_reject_count: u64,
    pub quote_reject_count_buy_x: u64,
    pub quote_reject_count_sell_x: u64,

    /// Requested input on refused candidate quotes, **kept in its own token**.
    ///
    /// The adapter cannot see the step's fair price, so these cannot be marked
    /// to a common numeraire at the moment they are recorded. Marking them
    /// afterwards would use the wrong price. They are therefore reported as two
    /// raw sums that are never added together.
    pub quote_rejected_requested_y: f64,
    pub quote_rejected_requested_x: f64,
    pub quote_fillable_y: f64,
    pub quote_fillable_x: f64,
    /// `requested - fillable`: what upstream Uniswap V3 would have left unspent
    /// on those same candidate quotes.
    pub quote_canonical_unfilled_y: f64,
    pub quote_canonical_unfilled_x: f64,

    // ---------------- 2. order-level capacity ----------------
    /// Retail orders for which a canonical probe was taken.
    pub retail_orders_probed: u64,
    /// Retail orders the pool **alone** could not have taken in full, measured
    /// once per order on the pre-route state at the full order size.
    ///
    /// A probe has three outcomes and they are counted separately: filled,
    /// capacity-limited (this field) and reverted
    /// ([`Self::retail_capacity_probe_revert_count`]). A revert is **not** a
    /// capacity result — the pool did not decline for want of room, the pricing
    /// maths refused to produce an answer at all — so it contributes to neither
    /// this count nor the shortfall.
    ///
    /// This is a statement about the pool's capacity, **not** about where the
    /// flow went: the router splits on price as well as capacity, so an order
    /// counted here may still have been served in full by the pool plus the
    /// competitor, and an order not counted here may still have gone to the
    /// competitor because the competitor quoted better.
    pub retail_full_order_capacity_limited_count: u64,
    /// Canonical probes that hit a revert branch (overflow, underflow, division
    /// by zero, a price outside the tick domain).
    ///
    /// Must be zero in any result that is published. A non-zero value means the
    /// probe could not answer the capacity question for that order, so both the
    /// limited count and the shortfall are silently under-measured, and the
    /// capacity figures cannot be trusted.
    pub retail_capacity_probe_revert_count: u64,
    /// Summed shortfall of those orders — `requested - fillable` — converted to
    /// Y at **the fair price of the step the order arrived in**, never at the
    /// run's final price.
    pub retail_capacity_shortfall_notional_y: f64,
    /// Retail notional this V3 pool actually served, in Y at the step's fair
    /// price. Mirrors `RunMetrics::retail_notional` so the V3 block can be read
    /// on its own.
    pub retail_notional_served: f64,

    // ---------------- 3. range occupancy ----------------
    /// Steps whose published fair price lay outside the position's range.
    /// Replaces any pool-state-derived "out of range" count: what matters is
    /// whether the pool could serve the *market* price, and the pool's own tick
    /// only moves when somebody trades.
    pub fair_price_out_of_range_steps: u64,
    /// Steps sampled, so the rate above has a denominator even when a run ends
    /// early. `fair_price_out_of_range_steps / steps_sampled` is the rate.
    pub steps_sampled: u64,
    /// First step at which the fair price left the range, or `None` if it never
    /// did. A run that leaves at step 3 and one that leaves at step 9 000 have
    /// the same rate only if they both stay out.
    pub first_out_of_range_step: Option<u64>,

    /// Steps at which the pool had non-zero active liquidity at its own current
    /// tick, over `steps_sampled`.
    ///
    /// **Under fill-or-kill this is close to 100% by construction and says very
    /// little.** Crossing a position boundary requires the order that consumes
    /// the last of the liquidity inside it, and that order is exactly the one
    /// the adapter refuses. The pool therefore parks *at* the boundary with its
    /// stored liquidity still non-zero, while being empty on one side — a run
    /// can finish holding 200 X and 0.03 Y and still report 100% here.
    ///
    /// It is reported because it is the standard definition of active liquidity
    /// and because leaving it out would hide the artefact. For "could the pool
    /// serve the market", read `fair_price_out_of_range_steps` instead.
    pub active_liquidity_steps: u64,
}

impl Univ3RunMetrics {
    /// Fraction of sampled steps whose fair price was outside the range.
    pub fn fair_price_out_of_range_rate(&self) -> f64 {
        if self.steps_sampled == 0 {
            f64::NAN
        } else {
            self.fair_price_out_of_range_steps as f64 / self.steps_sampled as f64
        }
    }

    /// Fraction of sampled steps with non-zero active liquidity.
    pub fn active_liquidity_rate(&self) -> f64 {
        if self.steps_sampled == 0 {
            f64::NAN
        } else {
            self.active_liquidity_steps as f64 / self.steps_sampled as f64
        }
    }

    /// Fraction of retail orders the pool alone could not have taken in full.
    pub fn retail_capacity_limited_rate(&self) -> f64 {
        if self.retail_orders_probed == 0 {
            f64::NAN
        } else {
            self.retail_full_order_capacity_limited_count as f64 / self.retail_orders_probed as f64
        }
    }
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

    /// Fraction of seeds with a positive net edge. This is a marginal rate, not
    /// a comparison against another strategy — see [`crate::paired`] for
    /// per-seed paired win rates between two strategies.
    pub positive_net_rate: f64,
    pub total_net_edge: f64,
    /// Total curve reverts across the batch. Must be zero for a valid result.
    pub total_curve_reverts: u64,

    /// Present only when every run in the batch carried V3 metrics.
    pub univ3: Option<Univ3Summary>,
}

/// Batch aggregate of the Uniswap V3 measures.
///
/// The `quote_*` fields are search diagnostics; the `retail_*_capacity_*` fields
/// are the order-level, search-independent ones. See [`Univ3RunMetrics`].
#[derive(Debug, Clone)]
pub struct Univ3Summary {
    // search diagnostics
    pub total_capacity_quote_rejects: u64,
    pub total_retail_quote_rejects: u64,
    pub total_arb_quote_rejects: u64,
    pub total_quote_rejects_buy_x: u64,
    pub total_quote_rejects_sell_x: u64,
    /// Per-seed sums, per token, never combined.
    pub quote_rejected_requested_y: Distribution,
    pub quote_rejected_requested_x: Distribution,
    pub quote_canonical_unfilled_y: Distribution,
    pub quote_canonical_unfilled_x: Distribution,

    // order-level capacity
    pub total_retail_orders_probed: u64,
    pub total_retail_capacity_limited_orders: u64,
    /// Must be zero in a publishable result. See
    /// [`Univ3RunMetrics::retail_capacity_probe_revert_count`].
    pub total_retail_capacity_probe_reverts: u64,
    pub retail_capacity_limited_rate: Distribution,
    pub retail_capacity_shortfall_notional_y: Distribution,
    pub retail_notional_served: Distribution,

    // range occupancy
    pub fair_price_out_of_range_rate: Distribution,
    pub active_liquidity_rate: Distribution,
    /// Over the seeds that ever left the range. `seeds_that_left_range` is the
    /// denominator, so a low mean is not read as "leaves early" when in fact
    /// almost nothing left at all.
    pub first_out_of_range_step: Distribution,
    pub seeds_that_left_range: usize,
}

impl Univ3Summary {
    fn from_runs(runs: &[RunMetrics]) -> Option<Univ3Summary> {
        let v3: Vec<Univ3RunMetrics> = runs.iter().filter_map(|r| r.univ3).collect();
        if v3.is_empty() || v3.len() != runs.len() {
            return None;
        }
        let collect = |f: fn(&Univ3RunMetrics) -> f64| -> Vec<f64> { v3.iter().map(f).collect() };
        let first_out: Vec<f64> = v3
            .iter()
            .filter_map(|m| m.first_out_of_range_step)
            .map(|s| s as f64)
            .collect();
        Some(Univ3Summary {
            total_capacity_quote_rejects: v3.iter().map(|m| m.capacity_quote_reject_count).sum(),
            total_retail_quote_rejects: v3.iter().map(|m| m.retail_quote_reject_count).sum(),
            total_arb_quote_rejects: v3.iter().map(|m| m.arb_quote_reject_count).sum(),
            total_quote_rejects_buy_x: v3.iter().map(|m| m.quote_reject_count_buy_x).sum(),
            total_quote_rejects_sell_x: v3.iter().map(|m| m.quote_reject_count_sell_x).sum(),
            quote_rejected_requested_y: Distribution::from_samples(&collect(|m| {
                m.quote_rejected_requested_y
            })),
            quote_rejected_requested_x: Distribution::from_samples(&collect(|m| {
                m.quote_rejected_requested_x
            })),
            quote_canonical_unfilled_y: Distribution::from_samples(&collect(|m| {
                m.quote_canonical_unfilled_y
            })),
            quote_canonical_unfilled_x: Distribution::from_samples(&collect(|m| {
                m.quote_canonical_unfilled_x
            })),
            total_retail_orders_probed: v3.iter().map(|m| m.retail_orders_probed).sum(),
            total_retail_capacity_limited_orders: v3
                .iter()
                .map(|m| m.retail_full_order_capacity_limited_count)
                .sum(),
            total_retail_capacity_probe_reverts: v3
                .iter()
                .map(|m| m.retail_capacity_probe_revert_count)
                .sum(),
            retail_capacity_limited_rate: Distribution::from_samples(&collect(|m| {
                m.retail_capacity_limited_rate()
            })),
            retail_capacity_shortfall_notional_y: Distribution::from_samples(&collect(|m| {
                m.retail_capacity_shortfall_notional_y
            })),
            retail_notional_served: Distribution::from_samples(&collect(|m| {
                m.retail_notional_served
            })),
            fair_price_out_of_range_rate: Distribution::from_samples(&collect(|m| {
                m.fair_price_out_of_range_rate()
            })),
            active_liquidity_rate: Distribution::from_samples(&collect(|m| {
                m.active_liquidity_rate()
            })),
            first_out_of_range_step: Distribution::from_samples(&first_out),
            seeds_that_left_range: first_out.len(),
        })
    }
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
            positive_net_rate: if runs.is_empty() {
                f64::NAN
            } else {
                positive as f64 / runs.len() as f64
            },
            total_net_edge: net.iter().sum(),
            total_curve_reverts: runs.iter().map(|r| r.curve_revert_count).sum(),
            univ3: Univ3Summary::from_runs(runs),
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
            univ3: None,
        }
    }

    #[test]
    fn summary_counts_positive_net_rate_and_totals() {
        let runs = vec![run(1.0), run(-2.0), run(3.0), run(0.0)];
        let summary = StrategySummary::from_runs("s", "dodo", "K=1e18", &runs);
        assert_eq!(summary.simulations, 4);
        assert_eq!(summary.positive_net_rate, 0.5);
        assert_eq!(summary.total_net_edge, 2.0);
        assert_eq!(summary.total_curve_reverts, 0);
        assert_eq!(summary.net_edge.min, -2.0);
        assert_eq!(summary.net_edge.max, 3.0);
    }
}
