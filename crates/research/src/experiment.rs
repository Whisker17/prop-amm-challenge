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
use crate::metrics::RunMetrics;
use crate::strategies::Strategy;
use crate::u256::U256;
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
    let mut amm_strategy = build_strategy_amm(strategy, config);
    let mut amm_competitor = build_competitor_amm(competitor, config);

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

        let orders = retail.generate_orders();
        for order in &orders {
            let trades =
                router.route_order(order, &mut amm_strategy, &mut amm_competitor, fair_price);
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
    }
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
