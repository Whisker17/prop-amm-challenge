//! The research simulation loop.
//!
//! Every component that generates or consumes order flow is reused from
//! `prop-amm-sim` **without modification**: [`GBMPriceProcess`],
//! [`RetailTrader`], [`Arbitrageur`], [`OrderRouter`] and [`BpfAmm`]. The only
//! thing added here is the research-only oracle publication path and the metric
//! collection.
//!
//! Per step, in this exact order:
//!
//! 1. `fair_price = price.step()`
//! 2. quantise once (`price_to_wad`) and publish the same WAD to the
//!    oracle-aware curve under test (DODO `i`, Flashbots `multX`; `multY` stays
//!    `1e18`)
//! 3. run the arbitrageur against both AMMs
//! 4. generate retail orders and route them
//!
//! The fair price is not exposed to anything beyond the components that already
//! received it in the existing engine (`Arbitrageur`, `OrderRouter`) plus the
//! oracle publication itself.
//!
//! ### Why the comparison is paired
//!
//! For a given seed the price path and the retail order stream are produced by
//! seeded RNGs whose consumption is independent of which curve is under test,
//! and the arbitrageur draws exactly one sample per step for the strategy AMM
//! regardless of family. So all strategies see identical quotes, identical
//! orders and identical capital.

use prop_amm_shared::config::{HyperparameterVariance, SimulationConfig};
use prop_amm_shared::normalizer::compute_swap as normalizer_swap;
use prop_amm_sim::amm::BpfAmm;
use prop_amm_sim::arbitrageur::Arbitrageur;
use prop_amm_sim::price_process::GBMPriceProcess;
use prop_amm_sim::retail::RetailTrader;
use prop_amm_sim::router::OrderRouter;
use rayon::prelude::*;

use crate::curves;
use crate::metrics::{RunMetrics, Univ3RunMetrics};
use crate::strategies::Strategy;
use crate::u256::U256;
use crate::univ3_curve;
use crate::wad::price_to_wad;

/// Who the strategy competes with for retail flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Competitor {
    /// The challenge's constant-product-with-fee normalizer, as in the existing
    /// engine (fee and liquidity multiplier come from the per-seed config).
    Normalizer,
    /// A curve that never quotes, so every routed order reaches the strategy.
    None,
}

impl Competitor {
    pub fn as_str(self) -> &'static str {
        match self {
            Competitor::Normalizer => "normalizer",
            Competitor::None => "solo",
        }
    }

    pub fn parse(text: &str) -> Option<Competitor> {
        match text {
            "paired" | "normalizer" => Some(Competitor::Normalizer),
            "solo" | "none" => Some(Competitor::None),
            _ => None,
        }
    }
}

/// Batch definition. Seeds are shared by every strategy, which is what makes the
/// comparison paired.
#[derive(Debug, Clone)]
pub struct BatchConfig {
    pub simulations: u32,
    pub steps: u32,
    pub seed_start: u64,
    pub seed_stride: u64,
    pub competitor: Competitor,
    pub workers: usize,
}

impl Default for BatchConfig {
    fn default() -> Self {
        BatchConfig {
            simulations: 20,
            steps: 1_000,
            seed_start: 0,
            seed_stride: 1,
            competitor: Competitor::Normalizer,
            workers: 0,
        }
    }
}

/// Per-seed configurations, generated exactly the way the challenge CLI
/// generates them, so the market environment is the repository's own baseline.
pub fn seed_configs(batch: &BatchConfig) -> Vec<SimulationConfig> {
    let variance = HyperparameterVariance::default();
    let base = SimulationConfig {
        n_steps: batch.steps,
        ..SimulationConfig::default()
    };
    (0..batch.simulations)
        .map(|i| {
            variance.apply(
                &base,
                batch
                    .seed_start
                    .wrapping_add((i as u64).wrapping_mul(batch.seed_stride)),
            )
        })
        .collect()
}

fn build_strategy_amm(strategy: &Strategy, config: &SimulationConfig) -> BpfAmm {
    let mut amm = BpfAmm::new_native(
        strategy.swap_fn(),
        strategy.after_swap_fn(),
        config.initial_x,
        config.initial_y,
        strategy.id.clone(),
    );
    amm.set_initial_storage(&strategy.initial_storage(
        config.initial_price,
        config.initial_x,
        config.initial_y,
    ));
    amm
}

fn build_competitor_amm(competitor: Competitor, config: &SimulationConfig) -> BpfAmm {
    match competitor {
        Competitor::Normalizer => {
            let mut amm = BpfAmm::new_native(
                normalizer_swap,
                None,
                config.initial_x * config.norm_liquidity_mult,
                config.initial_y * config.norm_liquidity_mult,
                "normalizer".to_string(),
            );
            amm.set_initial_storage(&config.norm_fee_bps.to_le_bytes());
            amm
        }
        Competitor::None => BpfAmm::new_native(
            curves::null_compute_swap,
            None,
            config.initial_x,
            config.initial_y,
            "null-competitor".to_string(),
        ),
    }
}

/// Run one strategy under one seed.
pub fn run_single(
    strategy: &Strategy,
    competitor: Competitor,
    config: &SimulationConfig,
) -> RunMetrics {
    run_single_traced(strategy, competitor, config, None)
}

/// Run one strategy under one seed, optionally recording the published oracle
/// prices (used by tests that assert every curve sees the same quantised price).
pub fn run_single_traced(
    strategy: &Strategy,
    competitor: Competitor,
    config: &SimulationConfig,
    mut oracle_trace: Option<&mut Vec<U256>>,
) -> RunMetrics {
    curves::reset_revert_count();
    // The V3 adapter accumulates capacity rejections in thread-local state, the
    // same way the revert counter does. `run_single` never yields, so a reset
    // here and a read at the end bracket exactly this run.
    univ3_curve::reset_capacity_stats();
    univ3_curve::clear_last_rejection();
    univ3_curve::set_caller(univ3_curve::Caller::Other);

    let mut amm_strategy = build_strategy_amm(strategy, config);
    let mut amm_competitor = build_competitor_amm(competitor, config);
    let track_univ3 = strategy.family.is_univ3();
    let mut univ3_metrics = Univ3RunMetrics::default();

    let mut price = GBMPriceProcess::new(
        config.initial_price,
        config.gbm_mu,
        config.gbm_sigma,
        config.gbm_dt,
        config.seed,
    );
    let mut retail = RetailTrader::new(
        config.retail_arrival_rate,
        config.retail_mean_size,
        config.retail_size_sigma,
        config.retail_buy_prob,
        config.seed.wrapping_add(1),
    );
    let mut arb = Arbitrageur::new(
        config.min_arb_profit,
        config.retail_mean_size,
        config.retail_size_sigma,
        config.seed.wrapping_add(2),
    );
    let router = OrderRouter::new();

    let mut retail_edge = 0.0_f64;
    let mut arbitrage_edge = 0.0_f64;
    let mut retail_notional = 0.0_f64;
    let mut competitor_retail_notional = 0.0_f64;
    let mut retail_trade_count = 0_u64;
    let mut arb_count = 0_u64;
    let mut arb_notional = 0.0_f64;
    let mut max_inventory_deviation = 0.0_f64;
    let mut fair_price = config.initial_price;

    for step in 0..config.n_steps {
        amm_strategy.set_current_step(step as u64);
        amm_competitor.set_current_step(step as u64);
        fair_price = price.step();

        // ---- research-only oracle publication ----
        // One quantisation per step, shared by every oracle-aware curve.
        if let Some(price_wad) = price_to_wad(fair_price) {
            if let Some(trace) = oracle_trace.as_deref_mut() {
                trace.push(price_wad);
            }
            if strategy.family.is_oracle_aware() {
                amm_strategy.set_initial_storage(&curves::oracle_publish_bytes(price_wad));
            }
        }

        // Sample range occupancy against the PUBLISHED FAIR PRICE, before any
        // trading this step. The pool's own tick only moves when somebody
        // trades, so a tick-derived measure would say a pool is "in range" while
        // the market has walked away from it.
        if track_univ3 {
            univ3_metrics.steps_sampled += 1;
            let storage = amm_strategy.storage();
            if let Some(price_wad) = price_to_wad(fair_price) {
                match univ3_curve::fair_price_in_range(storage, price_wad) {
                    Some(true) => {}
                    Some(false) => {
                        univ3_metrics.fair_price_out_of_range_steps += 1;
                        if univ3_metrics.first_out_of_range_step.is_none() {
                            univ3_metrics.first_out_of_range_step = Some(step as u64);
                        }
                    }
                    // The price could not be placed against the range at all.
                    // Counted in neither direction rather than guessed.
                    None => univ3_metrics.steps_sampled -= 1,
                }
            }
            if univ3_curve::active_liquidity(storage).is_some_and(|l| l > 0) {
                univ3_metrics.active_liquidity_steps += 1;
            }
        }

        univ3_curve::set_caller(univ3_curve::Caller::Arbitrage);
        if let Some(result) = arb.execute_arb(&mut amm_strategy, fair_price) {
            arb_count += 1;
            arbitrage_edge += result.edge;
            arb_notional += if result.amm_buys_x {
                result.amount_x * fair_price
            } else {
                result.amount_y
            };
        }
        arb.execute_arb(&mut amm_competitor, fair_price);
        univ3_curve::set_caller(univ3_curve::Caller::Other);

        let orders = retail.generate_orders();
        for order in &orders {
            // ---- canonical, search-independent order-level capacity probe ----
            //
            // Taken on the PRE-ROUTE state, at the FULL order size, exactly once
            // per order, and recording nothing in the adapter's counters. This
            // is deliberately not derived from what the router's search saw: the
            // search calls the curve many times per order and a refusal there
            // says only that one candidate size did not fit, which is compatible
            // with the order being filled in full a moment later.
            if track_univ3 {
                if let Some((side, input_nano)) = order_as_instruction(order, fair_price) {
                    univ3_metrics.retail_orders_probed += 1;
                    let probe =
                        univ3_curve::probe_capacity(amm_strategy.storage(), side, input_nano);
                    if !probe.filled {
                        univ3_metrics.retail_full_order_capacity_limited_count += 1;
                        // Marked to Y at THIS step's fair price, while it is the
                        // live price. Nothing is re-marked at the end of the run.
                        let shortfall = wad_to_f64(probe.canonical_unfilled_input());
                        univ3_metrics.retail_capacity_shortfall_notional_y += if side == 1 {
                            shortfall * fair_price
                        } else {
                            shortfall
                        };
                    }
                }
            }

            univ3_curve::set_caller(univ3_curve::Caller::Retail);
            let trades =
                router.route_order(order, &mut amm_strategy, &mut amm_competitor, fair_price);
            univ3_curve::set_caller(univ3_curve::Caller::Other);

            for trade in trades {
                let notional = if trade.amm_buys_x {
                    trade.amount_x * fair_price
                } else {
                    trade.amount_y
                };
                if trade.is_submission {
                    retail_edge += if trade.amm_buys_x {
                        trade.amount_x * fair_price - trade.amount_y
                    } else {
                        trade.amount_y - trade.amount_x * fair_price
                    };
                    retail_notional += notional;
                    retail_trade_count += 1;
                    if track_univ3 {
                        univ3_metrics.retail_notional_served += notional;
                    }
                } else {
                    competitor_retail_notional += notional;
                }
            }
        }

        let deviation = (amm_strategy.reserve_x - config.initial_x) / config.initial_x;
        if deviation.abs() > max_inventory_deviation {
            max_inventory_deviation = deviation.abs();
        }
    }

    if track_univ3 {
        // Search diagnostics. These are per-CANDIDATE-QUOTE, not per order, and
        // the amounts stay in the token they were offered in: the adapter has no
        // fair price at the moment it records them, and marking them here would
        // use the run's last price for events that happened at every other price
        // in the path.
        let stats = univ3_curve::capacity_stats();
        for caller in [
            univ3_curve::Caller::Retail,
            univ3_curve::Caller::Arbitrage,
            univ3_curve::Caller::Other,
        ] {
            for side in 0u8..2 {
                let bucket = stats.bucket(caller, side);
                if bucket.reject_count == 0 {
                    continue;
                }
                univ3_metrics.capacity_quote_reject_count += bucket.reject_count;
                let requested = wad_to_f64(bucket.requested_input);
                let fillable = wad_to_f64(bucket.fillable_input);
                let unfilled = wad_to_f64(bucket.canonical_unfilled_input());
                if side == 0 {
                    // side 0 spends Y to buy X.
                    univ3_metrics.quote_rejected_requested_y += requested;
                    univ3_metrics.quote_fillable_y += fillable;
                    univ3_metrics.quote_canonical_unfilled_y += unfilled;
                    univ3_metrics.quote_reject_count_buy_x += bucket.reject_count;
                } else {
                    // side 1 spends X to sell for Y.
                    univ3_metrics.quote_rejected_requested_x += requested;
                    univ3_metrics.quote_fillable_x += fillable;
                    univ3_metrics.quote_canonical_unfilled_x += unfilled;
                    univ3_metrics.quote_reject_count_sell_x += bucket.reject_count;
                }
                match caller {
                    univ3_curve::Caller::Retail => {
                        univ3_metrics.retail_quote_reject_count += bucket.reject_count
                    }
                    univ3_curve::Caller::Arbitrage => {
                        univ3_metrics.arb_quote_reject_count += bucket.reject_count
                    }
                    univ3_curve::Caller::Other => {}
                }
            }
        }
    }

    let total_retail_notional = retail_notional + competitor_retail_notional;
    RunMetrics {
        strategy_id: strategy.id.clone(),
        seed: config.seed,
        steps: config.n_steps,
        retail_edge,
        arbitrage_edge,
        arbitrage_loss: if arbitrage_edge < 0.0 {
            -arbitrage_edge
        } else {
            0.0
        },
        net_edge: retail_edge + arbitrage_edge,
        retail_notional,
        competitor_retail_notional,
        retail_flow_share: if total_retail_notional > 0.0 {
            retail_notional / total_retail_notional
        } else {
            f64::NAN
        },
        retail_trade_count,
        arb_count,
        arb_notional,
        final_inventory_deviation: (amm_strategy.reserve_x - config.initial_x) / config.initial_x,
        max_inventory_deviation,
        final_reserve_x: amm_strategy.reserve_x,
        final_reserve_y: amm_strategy.reserve_y,
        final_fair_price: fair_price,
        curve_revert_count: curves::revert_count(),
        univ3: track_univ3.then_some(univ3_metrics),
    }
}

/// A retail order as `(side, input_nano)`, matching the swap instruction.
///
/// `RetailOrder.size` is a Y notional in both directions; `OrderRouter` converts
/// a sell into `size / fair_price` units of X before quoting, and this reproduces
/// that conversion exactly so the probe asks about the same trade the router will.
///
/// Side follows the instruction encoding: `0` spends Y to buy X, `1` spends X to
/// sell for Y. Returns `None` for an order that quantises to nothing.
fn order_as_instruction(
    order: &prop_amm_sim::retail::RetailOrder,
    fair_price: f64,
) -> Option<(u8, u64)> {
    let (side, input) = if order.is_buy {
        (0u8, order.size)
    } else {
        (1u8, order.size / fair_price)
    };
    if !input.is_finite() || input <= 0.0 {
        return None;
    }
    let nano = prop_amm_shared::nano::f64_to_nano(input);
    (nano > 0).then_some((side, nano))
}

/// A WAD integer as an `f64` amount. Reporting only: no curve arithmetic passes
/// through here, and the value has already left the integer domain by the time
/// it is summed into a metric.
fn wad_to_f64(value: U256) -> f64 {
    let (whole, remainder) = value.div_rem_small(1_000_000_000_000_000_000);
    let whole = whole.as_u128().unwrap_or(u128::MAX) as f64;
    whole + remainder as f64 / 1e18
}

/// Run one strategy across the whole seed batch.
pub fn run_strategy_batch(
    strategy: &Strategy,
    batch: &BatchConfig,
    configs: &[SimulationConfig],
) -> Vec<RunMetrics> {
    configs
        .par_iter()
        .map(|config| run_single(strategy, batch.competitor, config))
        .collect()
}

/// Run every strategy across the same seed batch, in parallel over
/// `(strategy, seed)` pairs.
pub fn run_all(
    strategies: &[Strategy],
    batch: &BatchConfig,
) -> anyhow::Result<Vec<(Strategy, Vec<RunMetrics>)>> {
    let configs = seed_configs(batch);
    let workers = if batch.workers == 0 {
        rayon::current_num_threads()
    } else {
        batch.workers
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .build()?;

    let jobs: Vec<(usize, usize)> = (0..strategies.len())
        .flat_map(|s| (0..configs.len()).map(move |c| (s, c)))
        .collect();

    let mut results: Vec<Vec<Option<RunMetrics>>> = strategies
        .iter()
        .map(|_| vec![None; configs.len()])
        .collect();

    let computed: Vec<((usize, usize), RunMetrics)> = pool.install(|| {
        jobs.par_iter()
            .map(|(s, c)| {
                (
                    (*s, *c),
                    run_single(&strategies[*s], batch.competitor, &configs[*c]),
                )
            })
            .collect()
    });

    for ((s, c), metrics) in computed {
        results[s][c] = Some(metrics);
    }

    Ok(strategies
        .iter()
        .cloned()
        .zip(
            results
                .into_iter()
                .map(|row| row.into_iter().flatten().collect::<Vec<_>>()),
        )
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::{all_strategies, strategy_by_id, Family};

    fn small_batch(steps: u32) -> BatchConfig {
        BatchConfig {
            simulations: 3,
            steps,
            seed_start: 0,
            seed_stride: 1,
            competitor: Competitor::Normalizer,
            workers: 1,
        }
    }

    #[test]
    fn every_strategy_completes_a_short_run() {
        let batch = small_batch(200);
        let configs = seed_configs(&batch);
        for strategy in all_strategies() {
            let metrics = run_single(&strategy, Competitor::Normalizer, &configs[0]);
            assert_eq!(metrics.strategy_id, strategy.id);
            assert_eq!(metrics.steps, 200);
            assert!(
                metrics.final_reserve_x.is_finite() && metrics.final_reserve_y.is_finite(),
                "{} left non-finite reserves",
                strategy.id
            );
            assert!(
                metrics.net_edge.is_finite(),
                "{} produced a non-finite edge",
                strategy.id
            );
        }
    }

    #[test]
    fn oracle_price_sequence_is_identical_for_dodo_and_flashbots() {
        let batch = small_batch(300);
        let configs = seed_configs(&batch);
        let dodo = strategy_by_id("dodo-k100000000000000000").unwrap();
        let flashbots = strategy_by_id("flashbots-c10").unwrap();

        let mut dodo_trace = Vec::new();
        let mut flashbots_trace = Vec::new();
        run_single_traced(
            &dodo,
            Competitor::Normalizer,
            &configs[1],
            Some(&mut dodo_trace),
        );
        run_single_traced(
            &flashbots,
            Competitor::Normalizer,
            &configs[1],
            Some(&mut flashbots_trace),
        );

        assert_eq!(dodo_trace.len(), 300);
        assert_eq!(
            dodo_trace, flashbots_trace,
            "both curves must be published the same quantised price"
        );
    }

    #[test]
    fn runs_are_deterministic_for_a_fixed_seed() {
        let batch = small_batch(250);
        let configs = seed_configs(&batch);
        let strategy = strategy_by_id("dodo-k1000000000000000000").unwrap();
        let first = run_single(&strategy, Competitor::Normalizer, &configs[2]);
        let second = run_single(&strategy, Competitor::Normalizer, &configs[2]);
        assert_eq!(first.net_edge, second.net_edge);
        assert_eq!(first.arb_count, second.arb_count);
        assert_eq!(first.final_reserve_x, second.final_reserve_x);
    }

    #[test]
    fn solo_mode_routes_all_retail_flow_to_the_strategy() {
        let batch = small_batch(300);
        let configs = seed_configs(&batch);
        let strategy = strategy_by_id("univ2-zero-fee").unwrap();
        let metrics = run_single(&strategy, Competitor::None, &configs[0]);
        assert_eq!(
            metrics.competitor_retail_notional, 0.0,
            "the null competitor must never trade"
        );
        if metrics.retail_notional > 0.0 {
            assert_eq!(metrics.retail_flow_share, 1.0);
        }
    }

    #[test]
    fn univ2_ignores_the_oracle_by_construction() {
        let strategy = strategy_by_id("univ2-zero-fee").unwrap();
        assert!(!strategy.family.is_oracle_aware());
        assert_eq!(strategy.family, Family::UniV2);
    }

    #[test]
    fn the_univ3_arms_complete_a_run_and_report_their_own_metrics() {
        let batch = small_batch(300);
        let configs = seed_configs(&batch);
        for id in ["univ3-full-range-zero-fee", "univ3-conc-c100"] {
            let strategy = strategy_by_id(id).unwrap();
            let metrics = run_single(&strategy, Competitor::Normalizer, &configs[0]);
            let v3 = metrics
                .univ3
                .unwrap_or_else(|| panic!("{id} produced no V3 metrics"));
            assert_eq!(
                v3.steps_sampled, 300,
                "{id}: every step must be sampled for range occupancy"
            );
            assert!(
                metrics.net_edge.is_finite(),
                "{id} produced a non-finite edge"
            );
            // A capacity rejection is ordinary behaviour, a revert is not.
            assert_eq!(metrics.curve_revert_count, 0, "{id} reverted");
        }
    }

    /// The narrow arm must actually leave its range in this environment,
    /// otherwise the sensitivity experiment is measuring nothing.
    #[test]
    fn the_narrow_univ3_arm_leaves_its_range_and_says_when() {
        let batch = small_batch(400);
        let configs = seed_configs(&batch);
        let strategy = strategy_by_id("univ3-conc-c1000").unwrap();
        let metrics = run_single(&strategy, Competitor::Normalizer, &configs[0]);
        let v3 = metrics.univ3.unwrap();
        assert!(
            v3.fair_price_out_of_range_steps > 0,
            "a 20-tick range should not contain a 400-step GBM path"
        );
        assert!(
            v3.first_out_of_range_step.is_some(),
            "the first exit step must be recorded whenever any step is out"
        );
        assert!(
            v3.first_out_of_range_step.unwrap() < 400,
            "the recorded exit step must be inside the run"
        );
    }

    /// Non-V3 strategies must report `None`, never a zeroed struct that would
    /// read as "measured, and it was zero".
    #[test]
    fn only_the_univ3_arms_carry_univ3_metrics() {
        let batch = small_batch(120);
        let configs = seed_configs(&batch);
        for id in [
            "univ2-zero-fee",
            "dodo-k1000000000000000000",
            "flashbots-c1",
        ] {
            let strategy = strategy_by_id(id).unwrap();
            let metrics = run_single(&strategy, Competitor::Normalizer, &configs[0]);
            assert!(metrics.univ3.is_none(), "{id} should carry no V3 metrics");
        }
    }

    /// Refused candidate quotes must be attributable. This is about the search
    /// diagnostics, and deliberately says nothing about orders.
    #[test]
    fn refused_candidate_quotes_are_attributed_to_a_caller() {
        let batch = small_batch(500);
        let configs = seed_configs(&batch);
        let strategy = strategy_by_id("univ3-conc-c1000").unwrap();
        let mut saw_rejections = false;
        for config in &configs {
            let v3 = run_single(&strategy, Competitor::Normalizer, config)
                .univ3
                .unwrap();
            if v3.capacity_quote_reject_count == 0 {
                continue;
            }
            saw_rejections = true;
            assert_eq!(
                v3.retail_quote_reject_count + v3.arb_quote_reject_count,
                v3.capacity_quote_reject_count,
                "every refused quote must land in the retail or arbitrage bucket"
            );
            assert_eq!(
                v3.quote_reject_count_buy_x + v3.quote_reject_count_sell_x,
                v3.capacity_quote_reject_count,
                "every refused quote must land in a direction bucket"
            );
            assert!(
                v3.quote_canonical_unfilled_x >= 0.0
                    && v3.quote_canonical_unfilled_y >= 0.0
                    && v3.quote_fillable_x >= 0.0
                    && v3.quote_fillable_y >= 0.0,
                "capacity amounts must be non-negative"
            );
        }
        assert!(
            saw_rejections,
            "the narrowest arm refused no candidate quote across the batch; \
             the capacity path is then untested here"
        );
    }

    /// **The regression this test exists to prevent.**
    ///
    /// The router bisects towards a size that fits, so one retail order produces
    /// many refused candidate quotes. An order-level metric must not be inferred
    /// from "a refusal happened while the search was running", which is what a
    /// sticky `last_rejection` gave.
    ///
    /// The proof is a counting argument that needs no assumption about any
    /// individual order: the refused-quote count **exceeds the number of orders
    /// that existed**. A quantity larger than the total order count cannot be an
    /// order count, and the order-level metric is several times smaller.
    ///
    /// (Measured at the time of writing, `univ3-conc-c20` over 1 000 steps: 1 314
    /// refused quotes against 401 orders, of which 122 were capacity-limited.)
    #[test]
    fn refused_candidate_quotes_are_not_order_counts() {
        let batch = BatchConfig {
            simulations: 12,
            ..small_batch(1_000)
        };
        let configs = seed_configs(&batch);
        let strategy = strategy_by_id("univ3-conc-c20").unwrap();

        let mut saw_more_refusals_than_orders = false;
        let mut saw_order_count_below_quote_count = false;
        for config in &configs {
            let v3 = run_single(&strategy, Competitor::Normalizer, config)
                .univ3
                .unwrap();

            // An order-level count can never exceed the number of orders probed,
            // however many candidates the search burned through.
            assert!(
                v3.retail_full_order_capacity_limited_count <= v3.retail_orders_probed,
                "seed {}: capacity-limited orders {} exceeded orders probed {}",
                config.seed,
                v3.retail_full_order_capacity_limited_count,
                v3.retail_orders_probed
            );
            if v3.retail_quote_reject_count > v3.retail_orders_probed {
                saw_more_refusals_than_orders = true;
            }
            if v3.retail_quote_reject_count > 0 {
                assert!(
                    v3.retail_full_order_capacity_limited_count < v3.retail_quote_reject_count,
                    "seed {}: the order-level count {} matched the quote-level count {}, \
                     which is what contamination by the search would look like",
                    config.seed,
                    v3.retail_full_order_capacity_limited_count,
                    v3.retail_quote_reject_count
                );
                saw_order_count_below_quote_count = true;
            }
        }
        assert!(
            saw_more_refusals_than_orders,
            "no seed refused more candidate quotes than it had orders; the counting \
             argument this test relies on is not exercised by this configuration"
        );
        assert!(
            saw_order_count_below_quote_count,
            "no seed refused a candidate quote at all, so nothing was compared"
        );
    }

    /// The order-level probe is taken once per order and must not depend on how
    /// many candidates the router happened to try. Two runs of the same seed
    /// therefore agree exactly, and the count tracks orders, not search effort.
    #[test]
    fn the_order_level_capacity_probe_is_deterministic() {
        let batch = small_batch(300);
        let configs = seed_configs(&batch);
        let strategy = strategy_by_id("univ3-conc-c1000").unwrap();
        let first = run_single(&strategy, Competitor::Normalizer, &configs[0])
            .univ3
            .unwrap();
        let second = run_single(&strategy, Competitor::Normalizer, &configs[0])
            .univ3
            .unwrap();
        assert_eq!(
            first.retail_orders_probed, second.retail_orders_probed,
            "the probe must run once per order, deterministically"
        );
        assert_eq!(
            first.retail_full_order_capacity_limited_count,
            second.retail_full_order_capacity_limited_count
        );
        assert_eq!(
            first.retail_capacity_shortfall_notional_y,
            second.retail_capacity_shortfall_notional_y
        );
        assert!(
            first.retail_capacity_shortfall_notional_y >= 0.0,
            "a shortfall cannot be negative"
        );
    }

    /// A full-range position spans the whole tick domain, so it can never be
    /// capacity-limited. If it ever is, the range or the adapter is wrong.
    #[test]
    fn the_full_range_arm_is_never_capacity_limited() {
        let batch = small_batch(600);
        let configs = seed_configs(&batch);
        let strategy = strategy_by_id("univ3-full-range-zero-fee").unwrap();
        for config in &configs {
            let v3 = run_single(&strategy, Competitor::Normalizer, config)
                .univ3
                .unwrap();
            assert!(v3.retail_orders_probed > 0, "no orders were probed at all");
            assert_eq!(
                v3.retail_full_order_capacity_limited_count, 0,
                "seed {}: a full-range position was capacity-limited",
                config.seed
            );
            assert_eq!(v3.retail_capacity_shortfall_notional_y, 0.0);
            assert_eq!(
                v3.capacity_quote_reject_count, 0,
                "seed {}: a full-range position refused a candidate quote",
                config.seed
            );
        }
    }

    /// Pins the fill-or-kill artefact documented on
    /// [`Univ3RunMetrics::active_liquidity_steps`]: the pool ends one-sided but
    /// still reports active liquidity, because the order that would have crossed
    /// the boundary was refused. If this ever stops holding, that doc comment and
    /// the report's caveat are both wrong and must change with it.
    #[test]
    fn fill_or_kill_leaves_the_pool_one_sided_but_nominally_liquid() {
        let batch = small_batch(500);
        let configs = seed_configs(&batch);
        let strategy = strategy_by_id("univ3-conc-c1000").unwrap();
        let metrics = run_single(&strategy, Competitor::Normalizer, &configs[0]);
        let v3 = metrics.univ3.unwrap();

        assert!(
            v3.fair_price_out_of_range_steps > v3.steps_sampled / 2,
            "expected the narrow arm to spend most of the run out of range, got {}/{}",
            v3.fair_price_out_of_range_steps,
            v3.steps_sampled
        );
        assert_eq!(
            v3.active_liquidity_steps, v3.steps_sampled,
            "stored liquidity should never reach zero under fill-or-kill"
        );
        // One side is drained: the position converted almost entirely into X.
        let one_sided = metrics.final_reserve_y < 1.0 || metrics.final_reserve_x < 1.0;
        assert!(
            one_sided,
            "expected a one-sided pool, got x={} y={}",
            metrics.final_reserve_x, metrics.final_reserve_y
        );
    }

    #[test]
    fn batch_runner_returns_one_row_per_strategy_and_seed() {
        let batch = small_batch(120);
        let strategies = vec![
            strategy_by_id("univ2-zero-fee").unwrap(),
            strategy_by_id("flashbots-c1").unwrap(),
        ];
        let results = run_all(&strategies, &batch).expect("batch");
        assert_eq!(results.len(), 2);
        for (strategy, runs) in results {
            assert_eq!(runs.len(), 3, "{}", strategy.id);
            let seeds: Vec<u64> = runs.iter().map(|r| r.seed).collect();
            assert_eq!(seeds, vec![0, 1, 2], "seeds must stay aligned and ordered");
        }
    }
}
