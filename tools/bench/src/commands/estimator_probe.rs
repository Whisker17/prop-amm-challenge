use std::path::Path;

use clap::Args;
use prop_amm_shared::config::{SimulationConfig, BASELINE_STEPS};

use crate::compile::{self, Slot};
use crate::config::{BenchConfig, EstimatorProbeConfig};
use crate::estimator_probe::{Ewma004ProbeSim, Vol005ProbeSim};
use crate::report::{self, ReportMeta, ReportSection, DEFAULT_REPORT_DIR};
use crate::stats;

const DEFAULT_005: &str = "strategies/005-vol-adaptive-cpmm-fee/lib.rs";
const DEFAULT_004: &str = "strategies/004-ewma-shock-decay-fee/lib.rs";
const STAGE: &str = "estimator-probe-005b";

/// `bench estimator-probe` — WHI-1225's pre-registered "Probe A": replicates `005`'s two
/// variance normalizations and `004`'s `ewma_vol`-vs-floor distribution from a real run's
/// `after_swap` payload, with zero submission changes and zero search budget spent. Evaluates
/// both of WHI-1225's own kill conditions and reports WHI-1223's own open floor-sweep
/// question as an "Added scope" addendum in the same run.
#[derive(Args, Debug)]
pub struct EstimatorProbeArgs {
    /// The `005`-family candidate to run the dual variance-estimator probe against.
    #[arg(long, default_value = DEFAULT_005)]
    candidate_005: String,
    /// The `004`-family candidate to run the EWMA-vs-floor probe against.
    #[arg(long, default_value = DEFAULT_004)]
    candidate_004: String,
    /// Steps per simulation. Defaults to the challenge's own baseline, not a locally
    /// hardcoded copy of it.
    #[arg(long, default_value_t = BASELINE_STEPS)]
    steps: u32,
}

/// A seed's position within the `screening` segment's own `true_sigma` distribution, split
/// into equal-count thirds (by rank, not by equal-width value range — `regime.rs`'s `Tier`
/// splits by value range over `HyperparameterVariance`'s declared min/max, which is the right
/// choice for grid-style reporting bins but not for "the low/high tercile of *this run's own
/// 200 seeds*" that WHI-1225's kill rules ask for).
fn tercile_indices(sigmas: &[f64]) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
    let mut order: Vec<usize> = (0..sigmas.len()).collect();
    order.sort_by(|&a, &b| sigmas[a].partial_cmp(&sigmas[b]).unwrap());
    let n = order.len();
    let low_end = n / 3;
    let high_start = n - n / 3;
    let low = order[..low_end].to_vec();
    let high = order[high_start..].to_vec();
    let mid = order[low_end..high_start].to_vec();
    (low, mid, high)
}

struct KillEvaluation {
    low_tercile_n: usize,
    high_tercile_n: usize,
    median_count_ratio_low: f64,
    kill_i_triggered: bool,
    dynamic_range_old: f64,
    dynamic_range_new: f64,
    decompression_pct: f64,
    spearman_old: Option<f64>,
    spearman_new: Option<f64>,
    kill_ii_triggered: bool,
}

/// Evaluates WHI-1225's two pre-registered kill conditions against a completed Probe A run.
/// Pure function of the measured per-seed data (plus the configured thresholds) — no I/O, so
/// it's directly unit-testable against synthetic `Vol005ProbeSim` fixtures.
fn evaluate_kill_conditions(
    sims: &[Vol005ProbeSim],
    probe_config: &EstimatorProbeConfig,
) -> KillEvaluation {
    let true_sigmas: Vec<f64> = sims.iter().map(|s| s.true_sigma).collect();
    let (low, _mid, high) = tercile_indices(&true_sigmas);

    let count_ratios_low: Vec<f64> = low
        .iter()
        .map(|&i| sims[i].count as f64 / sims[i].n_steps as f64)
        .collect();
    let median_count_ratio_low = stats::median(&count_ratios_low).unwrap_or(f64::NAN);
    // Kill (i): median count/n_steps exceeds the configured threshold on the low-sigma tercile.
    let kill_i_triggered =
        median_count_ratio_low > probe_config.low_sigma_count_ratio_kill_threshold;

    let mean_of = |indices: &[usize], f: fn(&Vol005ProbeSim) -> f64| -> f64 {
        let sum: f64 = indices.iter().map(|&i| f(&sims[i])).sum();
        sum / indices.len() as f64
    };
    let old_low = mean_of(&low, |s| s.sigma_hat_old);
    let old_high = mean_of(&high, |s| s.sigma_hat_old);
    let new_low = mean_of(&low, |s| s.sigma_hat_new);
    let new_high = mean_of(&high, |s| s.sigma_hat_new);
    let dynamic_range_old = if old_low > 0.0 {
        old_high / old_low
    } else {
        f64::INFINITY
    };
    let dynamic_range_new = if new_low > 0.0 {
        new_high / new_low
    } else {
        f64::INFINITY
    };
    let decompression_pct = if dynamic_range_old.is_finite() && dynamic_range_old > 0.0 {
        (dynamic_range_new / dynamic_range_old - 1.0) * 100.0
    } else {
        f64::NAN
    };

    let sigma_hat_old: Vec<f64> = sims.iter().map(|s| s.sigma_hat_old).collect();
    let sigma_hat_new: Vec<f64> = sims.iter().map(|s| s.sigma_hat_new).collect();
    let spearman_old = stats::spearman_rank_correlation(&sigma_hat_old, &true_sigmas);
    let spearman_new = stats::spearman_rank_correlation(&sigma_hat_new, &true_sigmas);
    let rank_correlation_improved = match (spearman_old, spearman_new) {
        (Some(o), Some(n)) => n > o,
        // An undefined old correlation (e.g. a degenerate all-equal old estimate) that
        // becomes defined under the fix is itself an improvement.
        (None, Some(_)) => true,
        _ => false,
    };
    // Kill (ii): decompression under the configured threshold AND no rank-correlation
    // improvement.
    let kill_ii_triggered = decompression_pct < probe_config.decompression_kill_threshold_pct
        && !rank_correlation_improved;

    KillEvaluation {
        low_tercile_n: low.len(),
        high_tercile_n: high.len(),
        median_count_ratio_low,
        kill_i_triggered,
        dynamic_range_old,
        dynamic_range_new,
        decompression_pct,
        spearman_old,
        spearman_new,
        kill_ii_triggered,
    }
}

fn kill_evaluation_section(
    eval: &KillEvaluation,
    probe_config: &EstimatorProbeConfig,
) -> ReportSection {
    let verdict = if eval.kill_i_triggered {
        "KILL (i) TRIGGERED — the divisor fix is arithmetically a <=5% sigma-hat change, \
         dead on arrival."
    } else if eval.kill_ii_triggered {
        "KILL (ii) TRIGGERED — dynamic-range decompression under the configured threshold with \
         no rank-correlation improvement; the shape thesis is dead even if the level shifts."
    } else {
        "NEITHER KILL CONDITION TRIGGERED — Probe A does not close this lane; proceed to \
         Probe B (docs/DESIGN.md's own scratch degenerate-range method)."
    };

    ReportSection {
        heading: "Probe A — kill-condition evaluation (WHI-1225)".to_string(),
        body: format!(
            "- Low-sigma tercile: {} seeds; high-sigma tercile: {} seeds.\n\
             - **Kill (i)**: median `count/n_steps` on the low-sigma tercile = {:.6} \
             (threshold, `config/bench.toml` `[estimator_probe]`: > {}) -> {}\n\
             - **Kill (ii)** inputs: dynamic range old = {:.6}, new = {:.6}, decompression = \
             {:.2}% (threshold: < {}%); Spearman(sigma_hat_old, true_sigma) = {}, \
             Spearman(sigma_hat_new, true_sigma) = {} -> {}\n\n\
             ### Verdict\n\n{verdict}\n",
            eval.low_tercile_n,
            eval.high_tercile_n,
            eval.median_count_ratio_low,
            probe_config.low_sigma_count_ratio_kill_threshold,
            if eval.kill_i_triggered {
                "TRIGGERED"
            } else {
                "not triggered"
            },
            eval.dynamic_range_old,
            eval.dynamic_range_new,
            eval.decompression_pct,
            probe_config.decompression_kill_threshold_pct,
            fmt_opt(eval.spearman_old),
            fmt_opt(eval.spearman_new),
            if eval.kill_ii_triggered {
                "TRIGGERED"
            } else {
                "not triggered"
            },
        ),
    }
}

fn fmt_opt(value: Option<f64>) -> String {
    match value {
        Some(v) => format!("{v:.6}"),
        None => "undefined (constant series)".to_string(),
    }
}

fn per_seed_005_section(sims: &[Vol005ProbeSim]) -> ReportSection {
    let mut body = String::from(
        "Per-seed final estimator state, both normalizations, committed as raw evidence \
         (WHI-1225's own acceptance criterion: \"the per-seed data committed\").\n\n\
         | seed | true_sigma | count | elapsed_sum | n_steps | count/n_steps | sigma_hat_old \
         | sigma_hat_new |\n\
         | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for s in sims {
        body.push_str(&format!(
            "| {} | {:.6} | {} | {} | {} | {:.6} | {:.6} | {:.6} |\n",
            s.seed,
            s.true_sigma,
            s.count,
            s.elapsed_sum,
            s.n_steps,
            s.count as f64 / s.n_steps as f64,
            s.sigma_hat_old,
            s.sigma_hat_new,
        ));
    }
    ReportSection {
        heading: "Probe A — per-seed data (005, screening segment)".to_string(),
        body,
    }
}

fn floor_sweep_section(sims: &[Ewma004ProbeSim], floor_bps: &[u64]) -> ReportSection {
    let true_sigmas: Vec<f64> = sims.iter().map(|s| s.true_sigma).collect();
    let (low, mid, high) = tercile_indices(&true_sigmas);
    let terciles: [(&str, &[usize]); 3] = [("Low", &low), ("Mid", &mid), ("High", &high)];

    let mut body = String::from(
        "WHI-1223's own open question, answered per WHI-1225's \"Added scope\": `004`'s \
         `ewma_vol` fraction of **executed submission trades** at-or-below each floor (the \
         full range the `004b` issue froze, `0..=60` bps, from `config/bench.toml`'s \
         `[estimator_probe]` table), bucketed by this run's own `true_sigma` tercile, both by \
         trade count and by executed Y-volume. `004` updates `ewma_vol` on every executed \
         submission trade with no per-simulation-step dedup, so this is a fraction of trades, \
         **not** of simulation steps — a step where the router sent the submission no flow \
         contributes to neither the numerator nor the denominator.\n\n\
         | sigma tercile | n seeds | floor (bps) | fraction of trades <= floor | fraction of \
         volume <= floor |\n\
         | --- | --- | --- | --- | --- |\n",
    );
    for (label, indices) in terciles {
        if indices.is_empty() {
            continue;
        }
        for (i, &floor) in floor_bps.iter().enumerate() {
            let total_trades: u64 = indices.iter().map(|&idx| sims[idx].total_steps).sum();
            let below_trades: u64 = indices
                .iter()
                .map(|&idx| sims[idx].below_floor_steps[i])
                .sum();
            let total_volume: f64 = indices.iter().map(|&idx| sims[idx].total_volume).sum();
            let below_volume: f64 = indices
                .iter()
                .map(|&idx| sims[idx].below_floor_volume[i])
                .sum();
            let trade_frac = if total_trades > 0 {
                below_trades as f64 / total_trades as f64
            } else {
                f64::NAN
            };
            let volume_frac = if total_volume > 0.0 {
                below_volume / total_volume
            } else {
                f64::NAN
            };
            body.push_str(&format!(
                "| {label} | {} | {floor} | {trade_frac:.4} | {volume_frac:.4} |\n",
                indices.len(),
            ));
        }
    }
    ReportSection {
        heading: "Added scope — 004's ewma_vol vs. floor sweep (WHI-1223)".to_string(),
        body,
    }
}

pub fn run(args: EstimatorProbeArgs) -> anyhow::Result<()> {
    report::ensure_report_slot_free(Path::new(DEFAULT_REPORT_DIR), STAGE)?;

    let bench_config = BenchConfig::load_default()?;
    let screening = bench_config.segment("screening")?;
    let probe_config = bench_config.estimator_probe()?;
    let base = SimulationConfig {
        n_steps: args.steps,
        ..SimulationConfig::default()
    };
    let configs = screening.sim_configs(&base);

    println!(
        "Building {} (005 dual-estimator probe)...",
        args.candidate_005
    );
    let loaded_005 = compile::build_and_load(&args.candidate_005, Slot::Zero)?;
    println!(
        "Running {} simulations ({} steps each) on segment `screening` with the dual \
         variance-estimator probe installed...",
        configs.len(),
        args.steps,
    );
    let sims_005 = loaded_005.run_batch_with_005_estimator_probe(&configs)?;

    println!("Building {} (004 floor probe)...", args.candidate_004);
    let loaded_004 = compile::build_and_load(&args.candidate_004, Slot::Zero)?;
    println!(
        "Running {} simulations ({} steps each) on segment `screening` with the ewma_vol \
         floor-sweep probe installed...",
        configs.len(),
        args.steps,
    );
    let sims_004 =
        loaded_004.run_batch_with_004_floor_probe(&configs, &probe_config.floor_bps_sweep)?;

    let eval = evaluate_kill_conditions(&sims_005, probe_config);
    println!(
        "Kill (i): median count/n_steps (low tercile) = {:.6} -> {}",
        eval.median_count_ratio_low,
        if eval.kill_i_triggered {
            "TRIGGERED"
        } else {
            "not triggered"
        },
    );
    println!(
        "Kill (ii): decompression = {:.2}% -> {}",
        eval.decompression_pct,
        if eval.kill_ii_triggered {
            "TRIGGERED"
        } else {
            "not triggered"
        },
    );

    let meta = ReportMeta {
        stage: STAGE.to_string(),
        segment: "screening".to_string(),
        n_sims: configs.len(),
        n_steps: args.steps,
        execution_path: "native".to_string(),
    };
    let sections = vec![
        kill_evaluation_section(&eval, probe_config),
        floor_sweep_section(&sims_004, &probe_config.floor_bps_sweep),
        per_seed_005_section(&sims_005),
    ];
    let path = report::write_report(Path::new(DEFAULT_REPORT_DIR), &meta, &sections)?;
    println!("Report written to {}", path.display());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The thresholds this file used before they moved to `config/bench.toml` — kept as the
    /// test fixture so the existing test expectations (written against those exact numbers)
    /// don't need rederiving.
    fn test_probe_config() -> EstimatorProbeConfig {
        EstimatorProbeConfig {
            low_sigma_count_ratio_kill_threshold: 0.9,
            decompression_kill_threshold_pct: 15.0,
            floor_bps_sweep: vec![0, 10, 20, 25, 30, 40, 60],
        }
    }

    /// Test fixture constructor. `sigma_hat_old`/`sigma_hat_new` are passed explicitly rather
    /// than derived from `count`/`elapsed_sum` — these tests exercise the report/kill-
    /// condition logic in isolation, not the real `var_sum/count` arithmetic (which
    /// `estimator_probe::tests` already covers against a real simulation run).
    fn sim(
        seed: u64,
        true_sigma: f64,
        count: u64,
        elapsed_sum: u64,
        sigma_hat_old: f64,
        sigma_hat_new: f64,
    ) -> Vol005ProbeSim {
        Vol005ProbeSim {
            seed,
            true_sigma,
            n_steps: 10_000,
            count,
            elapsed_sum,
            sigma_hat_old,
            sigma_hat_new,
        }
    }

    #[test]
    fn tercile_indices_splits_by_rank_not_by_value_range() {
        let sigmas = vec![0.001, 0.007, 0.002, 0.006, 0.003, 0.005];
        let (low, mid, high) = tercile_indices(&sigmas);
        assert_eq!(low.len(), 2);
        assert_eq!(mid.len(), 2);
        assert_eq!(high.len(), 2);
        // The two lowest values are at indices 0 (0.001) and 2 (0.002).
        let mut low_sorted = low.clone();
        low_sorted.sort_unstable();
        assert_eq!(low_sorted, vec![0, 2]);
    }

    #[test]
    fn kill_i_triggers_when_median_count_ratio_exceeds_threshold_on_low_tercile() {
        // 6 seeds so the low tercile is exactly 2 — both near-saturated (ratio > 0.9).
        // sigma_hat values are non-degenerate placeholders; kill (i) doesn't read them.
        let sims: Vec<Vol005ProbeSim> = vec![
            sim(1, 0.0001, 9_900, 9_900, 10.0, 10.0),
            sim(2, 0.0002, 9_950, 9_950, 11.0, 11.0),
            sim(3, 0.003, 5_000, 8_000, 30.0, 24.0),
            sim(4, 0.004, 5_000, 8_000, 31.0, 25.0),
            sim(5, 0.006, 3_000, 3_100, 50.0, 49.0),
            sim(6, 0.007, 3_000, 3_100, 51.0, 50.0),
        ];
        let eval = evaluate_kill_conditions(&sims, &test_probe_config());
        assert!(eval.kill_i_triggered, "expected kill (i) to trigger");
    }

    #[test]
    fn kill_i_does_not_trigger_when_low_tercile_undersamples() {
        let sims: Vec<Vol005ProbeSim> = vec![
            sim(1, 0.0001, 4_000, 9_900, 10.0, 6.0),
            sim(2, 0.0002, 4_100, 9_950, 11.0, 7.0),
            sim(3, 0.003, 5_000, 8_000, 30.0, 24.0),
            sim(4, 0.004, 5_000, 8_000, 31.0, 25.0),
            sim(5, 0.006, 8_000, 8_100, 50.0, 49.0),
            sim(6, 0.007, 8_500, 8_600, 51.0, 50.0),
        ];
        let eval = evaluate_kill_conditions(&sims, &test_probe_config());
        assert!(!eval.kill_i_triggered, "did not expect kill (i) to trigger");
        assert!(eval.median_count_ratio_low < 0.9);
    }

    #[test]
    fn kill_ii_triggers_when_decompression_is_small_and_no_rank_improvement() {
        // sigma_hat_old and sigma_hat_new are identical, strictly increasing in true_sigma —
        // the fix changes nothing: decompression = 0%, and neither rank correlation improves
        // on the other (both are already perfect).
        let sims: Vec<Vol005ProbeSim> = (0..12u64)
            .map(|i| {
                let sigma = 0.0001 + (i as f64) * 0.0006;
                let sigma_hat = 10.0 + i as f64 * 4.0;
                sim(i, sigma, 100 + i * 50, 100 + i * 50, sigma_hat, sigma_hat)
            })
            .collect();
        let eval = evaluate_kill_conditions(&sims, &test_probe_config());
        assert!((eval.decompression_pct).abs() < 1e-9);
        assert!(eval.kill_ii_triggered, "expected kill (ii) to trigger");
    }

    #[test]
    fn kill_ii_does_not_trigger_when_decompression_is_large() {
        // Old estimate is nearly flat across the sigma range (dynamic range ~1.1); the new
        // estimate spreads much further (dynamic range ~3.0) -> decompression well over 15%.
        let sims: Vec<Vol005ProbeSim> = vec![
            sim(1, 0.0001, 100, 900, 50.0, 20.0),
            sim(2, 0.00015, 110, 950, 51.0, 21.0),
            sim(3, 0.0002, 120, 1_000, 52.0, 22.0),
            sim(4, 0.0035, 5_000, 5_500, 53.0, 40.0),
            sim(5, 0.0038, 5_100, 5_600, 54.0, 41.0),
            sim(6, 0.004, 5_200, 5_700, 55.0, 42.0),
            sim(7, 0.0068, 9_800, 9_850, 56.0, 59.0),
            sim(8, 0.0069, 9_850, 9_900, 57.0, 60.0),
            sim(9, 0.007, 9_900, 9_950, 58.0, 61.0),
        ];
        let eval = evaluate_kill_conditions(&sims, &test_probe_config());
        assert!(eval.decompression_pct > 15.0);
        assert!(
            !eval.kill_ii_triggered,
            "did not expect kill (ii) to trigger"
        );
    }

    #[test]
    fn floor_sweep_section_reports_every_tercile_and_floor() {
        let floor_bps = test_probe_config().floor_bps_sweep;
        let sims: Vec<Ewma004ProbeSim> = (0..9u64)
            .map(|i| {
                let sigma = 0.0001 + (i as f64) * 0.0008;
                Ewma004ProbeSim {
                    seed: i,
                    true_sigma: sigma,
                    below_floor_steps: vec![10, 20, 30, 35, 40, 50, 60],
                    total_steps: 100,
                    below_floor_volume: vec![100.0, 200.0, 300.0, 350.0, 400.0, 500.0, 600.0],
                    total_volume: 1_000.0,
                }
            })
            .collect();
        let section = floor_sweep_section(&sims, &floor_bps);
        assert!(section.body.contains("Low"));
        assert!(section.body.contains("Mid"));
        assert!(section.body.contains("High"));
        for floor in floor_bps {
            assert!(section.body.contains(&floor.to_string()));
        }
    }
}
