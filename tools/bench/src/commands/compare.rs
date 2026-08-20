use std::path::Path;

use clap::Args;
use prop_amm_shared::config::SimulationConfig;
use prop_amm_shared::normalizer;
use prop_amm_sim::runner;

use crate::compile::{self, Slot};
use crate::config::BenchConfig;
use crate::report::{self, ReportMeta, ReportSection};
use crate::stats;

const CONFIG_PATH: &str = "config/bench.toml";
const REPORT_DIR: &str = "results";

/// `bench compare` — the headline paired-by-seed comparison (docs/DESIGN.md §2.1, §2.6).
#[derive(Args, Debug)]
pub struct CompareArgs {
    /// Path to the candidate's .rs source file.
    #[arg(long)]
    candidate: String,
    /// Path to the reference's .rs source file.
    #[arg(long)]
    reference: String,
    /// Seed segment to evaluate on (train|validation|test|observation, per config/bench.toml).
    #[arg(long)]
    segment: String,
    /// Steps per simulation.
    #[arg(long, default_value_t = 10_000)]
    steps: u32,
    /// Required to select a single-use segment (the test segment).
    #[arg(long)]
    i_am_spending_the_test_segment: bool,
}

pub fn run(args: CompareArgs) -> anyhow::Result<()> {
    let bench_config = BenchConfig::load(Path::new(CONFIG_PATH))?;
    let segment = bench_config.segment(&args.segment)?;

    if segment.single_use && !args.i_am_spending_the_test_segment {
        anyhow::bail!(
            "segment `{}` is single-use; pass --i-am-spending-the-test-segment to spend it",
            args.segment
        );
    }
    if !segment.decision_input {
        println!(
            "Note: segment `{}` is not a decision input (docs/DESIGN.md §2.2) — reporting only.",
            args.segment
        );
    }

    let base = SimulationConfig { n_steps: args.steps, ..SimulationConfig::default() };
    let configs = segment.sim_configs(&base);

    println!("Building candidate: {}", args.candidate);
    let candidate = compile::build_and_load(&args.candidate, Slot::Zero)?;
    println!("Building reference: {}", args.reference);
    let reference = compile::build_and_load(&args.reference, Slot::One)?;

    println!(
        "Running {} simulations ({} steps each) on segment `{}`...",
        configs.len(),
        args.steps,
        args.segment
    );

    // Same `configs` reused for both runs (not regenerated) — that's what keeps the
    // comparison paired (docs/DESIGN.md §2.6's own note on this exact point).
    let candidate_result = runner::run_batch_native(
        candidate.swap_fn,
        candidate.after_swap_fn,
        normalizer::compute_swap,
        Some(normalizer::after_swap),
        configs.clone(),
        None,
    )?;
    let reference_result = runner::run_batch_native(
        reference.swap_fn,
        reference.after_swap_fn,
        normalizer::compute_swap,
        Some(normalizer::after_swap),
        configs,
        None,
    )?;

    let stat = stats::paired_stat(&candidate_result.results, &reference_result.results)?;

    println!(
        "Candidate avg edge: {:.2}  Reference avg edge: {:.2}",
        candidate_result.avg_edge(),
        reference_result.avg_edge()
    );
    println!(
        "Paired mean diff: {:.6}  95% CI [{:.6}, {:.6}]  n={}",
        stat.mean_diff, stat.ci_low, stat.ci_high, stat.n
    );

    let body = format!(
        "- Candidate: `{}` (avg edge {:.2})\n- Reference: `{}` (avg edge {:.2})\n- Paired mean difference: {:.6}\n- 95% CI: [{:.6}, {:.6}]\n",
        args.candidate,
        candidate_result.avg_edge(),
        args.reference,
        reference_result.avg_edge(),
        stat.mean_diff,
        stat.ci_low,
        stat.ci_high,
    );

    let meta = ReportMeta {
        stage: "compare".to_string(),
        segment: args.segment.clone(),
        n_sims: stat.n,
        n_steps: args.steps,
        execution_path: "native".to_string(),
    };
    let sections = vec![ReportSection { heading: "Paired comparison".to_string(), body }];
    let path = report::write_report(Path::new(REPORT_DIR), &meta, &sections)?;
    println!("Report written to {}", path.display());

    Ok(())
}
