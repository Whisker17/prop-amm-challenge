use std::path::Path;
use std::process::Command;

use clap::Args;
use prop_amm_shared::config::SimulationConfig;

use crate::commands::note_if_not_decision_input;
use crate::compile::{self, Slot};
use crate::config::{BenchConfig, SegmentSelector};
use crate::report::{self, ReportMeta, ReportSection, DEFAULT_REPORT_DIR};

const DEFAULT_FILE: &str = "programs/starter/src/lib.rs";
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
    #[command(flatten)]
    segment_selector: SegmentSelector,
}

/// The upstream CLI only ever prints edge at 2 decimal places (`crates/cli/src/output.rs`),
/// so "agreement" below means bench's own full-precision number rounds to the same string —
/// the maximum the existing tooling can prove without editing upstream-owned code.
fn edges_agree(a: f64, b: f64) -> bool {
    format!("{a:.2}") == format!("{b:.2}")
}

struct SeedCheck {
    seed: u64,
    bench_edge: f64,
    cli_edge: f64,
    matches: bool,
}

pub fn run(args: AnchorArgs) -> anyhow::Result<()> {
    let bench_config = BenchConfig::load_default()?;
    let (segment_name, segment) = args.segment_selector.resolve(&bench_config)?;
    note_if_not_decision_input(segment_name, segment);

    let base = SimulationConfig::default();
    let configs = segment.sim_configs(&base);
    let seeds = segment.seeds();

    println!("Building {}...", args.file);
    let loaded = compile::build_and_load(&args.file, Slot::Zero)?;

    println!(
        "Running {} simulations ({} steps each) on segment `{segment_name}`...",
        configs.len(),
        base.n_steps,
    );
    let batch = loaded.run_batch(configs)?;
    println!(
        "Avg edge: {:.2}  Total edge: {:.2}",
        batch.avg_edge(),
        batch.total_edge
    );

    println!("Cross-checking aggregate against `prop-amm run`...");
    let cli_aggregate = run_prop_amm(
        &args.file,
        segment.count as u32,
        base.n_steps,
        segment.start,
        segment.stride,
    )?;
    let aggregate_matches = edges_agree(batch.avg_edge(), cli_aggregate.avg)
        && edges_agree(batch.total_edge, cli_aggregate.total);
    if !aggregate_matches {
        anyhow::bail!(
            "aggregate mismatch: bench avg={:.2} total={:.2} vs cli avg={:.2} total={:.2}",
            batch.avg_edge(),
            batch.total_edge,
            cli_aggregate.avg,
            cli_aggregate.total,
        );
    }
    println!(
        "Aggregate agreement: bench avg={:.2} total={:.2} matches cli avg={:.2} total={:.2}",
        batch.avg_edge(),
        batch.total_edge,
        cli_aggregate.avg,
        cli_aggregate.total,
    );

    println!("Spot-checking {SAMPLE_SEEDS} sampled seeds against `prop-amm run`...");
    let mut checks = Vec::with_capacity(SAMPLE_SEEDS);
    for seed in sample_seeds(&seeds, SAMPLE_SEEDS) {
        let bench_edge = batch
            .results
            .iter()
            .find(|r| r.seed == seed)
            .map(|r| r.submission_edge)
            .ok_or_else(|| anyhow::anyhow!("seed {seed} missing from batch results"))?;
        let cli_edge = run_prop_amm(&args.file, 1, base.n_steps, seed, 1)?.total;
        checks.push(SeedCheck {
            seed,
            bench_edge,
            cli_edge,
            matches: edges_agree(bench_edge, cli_edge),
        });
    }

    compile::cleanup(loaded.build_dir.as_deref());

    let mismatches: Vec<&SeedCheck> = checks.iter().filter(|c| !c.matches).collect();
    if !mismatches.is_empty() {
        anyhow::bail!(
            "{} of {SAMPLE_SEEDS} sampled seeds disagree with `prop-amm run`: {}",
            mismatches.len(),
            mismatches
                .iter()
                .map(|c| format!(
                    "(seed {}, bench={:.2}, cli={:.2})",
                    c.seed, c.bench_edge, c.cli_edge
                ))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    println!(
        "Per-seed agreement: {SAMPLE_SEEDS}/{SAMPLE_SEEDS} sampled seeds match `prop-amm run`."
    );

    let meta = ReportMeta {
        stage: "anchor".to_string(),
        segment: segment_name.to_string(),
        n_sims: batch.n_sims(),
        n_steps: base.n_steps,
        execution_path: "native".to_string(),
    };
    let sections = vec![
        ReportSection {
            heading: "Aggregate parity".to_string(),
            body: format!(
                "- Bench: avg edge {:.2}, total edge {:.2}, n={}\n- `prop-amm run`: avg edge {:.2}, total edge {:.2}\n",
                batch.avg_edge(), batch.total_edge, batch.n_sims(), cli_aggregate.avg, cli_aggregate.total,
            ),
        },
        ReportSection {
            heading: "Per-seed spot checks".to_string(),
            body: format_seed_checks(&args.file, &checks),
        },
    ];
    let path = report::write_report(Path::new(DEFAULT_REPORT_DIR), &meta, &sections)?;
    println!("Report written to {}", path.display());

    Ok(())
}

fn format_seed_checks(file: &str, checks: &[SeedCheck]) -> String {
    let mut body = format!(
        "Spot checks against `prop-amm run {file} --simulations 1 --seed-start <seed>`. \
         The upstream CLI only ever prints edge at 2 decimal places, so agreement here is \
         checked at that precision — the maximum the existing tooling can prove without \
         editing upstream-owned code.\n\n"
    );
    for check in checks {
        body.push_str(&format!(
            "- seed {}: bench={:.2} cli={:.2} {}\n",
            check.seed,
            check.bench_edge,
            check.cli_edge,
            if check.matches { "PASS" } else { "FAIL" },
        ));
    }
    body
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

struct CliEdgeReport {
    avg: f64,
    total: f64,
}

fn run_prop_amm(
    file: &str,
    simulations: u32,
    steps: u32,
    seed_start: u64,
    seed_stride: u64,
) -> anyhow::Result<CliEdgeReport> {
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
            &simulations.to_string(),
            "--steps",
            &steps.to_string(),
            "--seed-start",
            &seed_start.to_string(),
            "--seed-stride",
            &seed_stride.to_string(),
        ])
        .output()
        .map_err(|e| anyhow::anyhow!("failed to run `prop-amm run {file}`: {e}"))?;

    if !output.status.success() {
        anyhow::bail!(
            "`prop-amm run {file}` failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut avg = None;
    let mut total = None;
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("Avg edge:") {
            avg = Some(parse_edge(value)?);
        } else if let Some(value) = trimmed.strip_prefix("Total edge:") {
            total = Some(parse_edge(value)?);
        }
    }

    match (avg, total) {
        (Some(avg), Some(total)) => Ok(CliEdgeReport { avg, total }),
        _ => anyhow::bail!(
            "`prop-amm run {file}` did not print both `Avg edge:`/`Total edge:` lines:\n{stdout}"
        ),
    }
}

fn parse_edge(value: &str) -> anyhow::Result<f64> {
    value
        .trim()
        .parse::<f64>()
        .map_err(|e| anyhow::anyhow!("failed to parse edge `{value}`: {e}"))
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

    #[test]
    fn edges_agree_compares_at_cli_precision() {
        assert!(edges_agree(210.501, 210.499));
        assert!(!edges_agree(210.50, 210.51));
    }

    #[test]
    fn format_seed_checks_marks_pass_and_fail() {
        let checks = vec![
            SeedCheck {
                seed: 1,
                bench_edge: 1.0,
                cli_edge: 1.0,
                matches: true,
            },
            SeedCheck {
                seed: 2,
                bench_edge: 2.0,
                cli_edge: 3.0,
                matches: false,
            },
        ];
        let body = format_seed_checks("f.rs", &checks);
        assert!(body.contains("seed 1: bench=1.00 cli=1.00 PASS"));
        assert!(body.contains("seed 2: bench=2.00 cli=3.00 FAIL"));
    }
}
