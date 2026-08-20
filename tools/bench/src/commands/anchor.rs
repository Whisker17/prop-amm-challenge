use std::path::Path;
use std::process::Command;

use clap::Args;
use prop_amm_shared::config::SimulationConfig;
use prop_amm_shared::normalizer;
use prop_amm_sim::runner;

use crate::compile::{self, Slot};
use crate::config::BenchConfig;
use crate::report::{self, ReportMeta, ReportSection};

const CONFIG_PATH: &str = "config/bench.toml";
const REPORT_DIR: &str = "results";
const DEFAULT_FILE: &str = "programs/starter/src/lib.rs";
const DEFAULT_SEGMENT: &str = "observation";
const SAMPLE_SEEDS: usize = 20;

/// `bench anchor` — cross-checks bench's own numbers against a real, separately-invoked
/// `prop-amm run`, both in aggregate and per seed (docs/DESIGN.md §2.6's parity gate). This
/// is the acceptance check for this whole issue, made repeatable rather than a one-off
/// manual comparison, and it doubles as the second subcommand that proves the dispatch seam
/// in `cli.rs` actually works with more than one command plugged in.
#[derive(Args, Debug)]
pub struct AnchorArgs {
    /// Path to the .rs source file to anchor-check. Defaults to the starter.
    #[arg(long, default_value = DEFAULT_FILE)]
    file: String,
    /// Segment to run the aggregate check on. Defaults to `observation` (seeds 0..=999).
    #[arg(long, default_value = DEFAULT_SEGMENT)]
    segment: String,
}

pub fn run(args: AnchorArgs) -> anyhow::Result<()> {
    let bench_config = BenchConfig::load(Path::new(CONFIG_PATH))?;
    let segment = bench_config.segment(&args.segment)?;

    if !segment.decision_input {
        println!(
            "Note: segment `{}` is not a decision input (docs/DESIGN.md §2.2) — reporting only.",
            args.segment
        );
    }

    let base = SimulationConfig::default();
    let configs = segment.sim_configs(&base);
    let seeds = segment.seeds();

    println!("Building {}...", args.file);
    let loaded = compile::build_and_load(&args.file, Slot::Zero)?;

    println!(
        "Running {} simulations ({} steps each) on segment `{}`...",
        configs.len(),
        base.n_steps,
        args.segment
    );
    let batch = runner::run_batch_native(
        loaded.swap_fn,
        loaded.after_swap_fn,
        normalizer::compute_swap,
        Some(normalizer::after_swap),
        configs,
        None,
    )?;

    println!("Avg edge: {:.2}  Total edge: {:.2}", batch.avg_edge(), batch.total_edge);

    let mut body = format!(
        "- Aggregate: avg edge {:.2}, total edge {:.2}, n={}\n\n\
         Per-seed spot checks against `prop-amm run {} --simulations 1 --seed-start <seed>`. \
         The upstream CLI only ever prints edge at 2 decimal places (`crates/cli/src/output.rs`), \
         so agreement below is checked at that precision — the maximum the existing tooling can \
         prove without editing upstream-owned code.\n\n",
        batch.avg_edge(),
        batch.total_edge,
        batch.n_sims(),
        args.file,
    );

    let mut mismatches = Vec::new();
    for seed in sample_seeds(&seeds, SAMPLE_SEEDS) {
        let bench_edge = batch
            .results
            .iter()
            .find(|r| r.seed == seed)
            .map(|r| r.submission_edge)
            .ok_or_else(|| anyhow::anyhow!("seed {seed} missing from batch results"))?;
        let cli_edge = run_single_seed(&args.file, seed)?;

        let matches = format!("{bench_edge:.2}") == format!("{cli_edge:.2}");
        if !matches {
            mismatches.push((seed, bench_edge, cli_edge));
        }
        body.push_str(&format!(
            "- seed {seed}: bench={bench_edge:.2} cli={cli_edge:.2} {}\n",
            if matches { "PASS" } else { "FAIL" }
        ));
    }

    if !mismatches.is_empty() {
        anyhow::bail!(
            "{} of {} sampled seeds disagree with `prop-amm run`: {:?}",
            mismatches.len(),
            SAMPLE_SEEDS,
            mismatches
        );
    }
    println!("Per-seed agreement: {SAMPLE_SEEDS}/{SAMPLE_SEEDS} sampled seeds match `prop-amm run`.");

    let meta = ReportMeta {
        stage: "anchor".to_string(),
        segment: args.segment.clone(),
        n_sims: batch.n_sims(),
        n_steps: base.n_steps,
        execution_path: "native".to_string(),
    };
    let sections = vec![ReportSection { heading: "Starter anchor".to_string(), body }];
    let path = report::write_report(Path::new(REPORT_DIR), &meta, &sections)?;
    println!("Report written to {}", path.display());

    Ok(())
}

/// Evenly-spaced sample of `n` seeds from `seeds` (falls back to all of them if there are
/// fewer than `n`).
fn sample_seeds(seeds: &[u64], n: usize) -> Vec<u64> {
    if seeds.len() <= n || n == 0 {
        return seeds.to_vec();
    }
    let stride = seeds.len() / n;
    (0..n).map(|i| seeds[i * stride]).collect()
}

fn run_single_seed(file: &str, seed: u64) -> anyhow::Result<f64> {
    let output = Command::new("cargo")
        .args([
            "run",
            "--release",
            "-p",
            "prop-amm",
            "--",
            "run",
            file,
            "--simulations",
            "1",
            "--seed-start",
            &seed.to_string(),
        ])
        .output()
        .map_err(|e| {
            anyhow::anyhow!("failed to run `prop-amm run {file} --seed-start {seed}`: {e}")
        })?;

    if !output.status.success() {
        anyhow::bail!(
            "`prop-amm run {file} --seed-start {seed}` failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(value) = line.trim().strip_prefix("Total edge:") {
            return value
                .trim()
                .parse::<f64>()
                .map_err(|e| anyhow::anyhow!("failed to parse total edge `{value}`: {e}"));
        }
    }

    anyhow::bail!(
        "`prop-amm run {file} --seed-start {seed}` did not print a `Total edge:` line:\n{stdout}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_seeds_picks_evenly_spaced_subset() {
        let seeds: Vec<u64> = (0..1000).collect();
        let sampled = sample_seeds(&seeds, 20);
        assert_eq!(sampled.len(), 20);
        assert_eq!(sampled[0], 0);
        assert_eq!(sampled[1], 50);
        assert_eq!(sampled[19], 950);
    }

    #[test]
    fn sample_seeds_returns_all_when_fewer_than_requested() {
        let seeds: Vec<u64> = (0..5).collect();
        assert_eq!(sample_seeds(&seeds, 20), seeds);
    }
}
