use std::path::Path;

use clap::Args;
use prop_amm_shared::config::SimulationConfig;

use crate::commands::note_if_not_decision_input;
use crate::compile::{self, Slot};
use crate::config::{BenchConfig, SegmentSelector};
use crate::report::{self, ReportMeta, ReportSection, DEFAULT_REPORT_DIR};
use crate::stats;

/// `bench compare` — the headline paired-by-seed comparison (docs/DESIGN.md §2.1, §2.6).
#[derive(Args, Debug)]
pub struct CompareArgs {
    /// Path to the candidate's .rs source file.
    #[arg(long)]
    candidate: String,
    /// Path to the reference's .rs source file.
    #[arg(long)]
    reference: String,
    #[command(flatten)]
    segment_selector: SegmentSelector,
    /// Steps per simulation.
    #[arg(long, default_value_t = 10_000)]
    steps: u32,
}

pub fn run(args: CompareArgs) -> anyhow::Result<()> {
    let bench_config = BenchConfig::load_default()?;
    let (segment_name, segment) = args.segment_selector.resolve(&bench_config)?;
    note_if_not_decision_input(segment_name, segment);

    let base = SimulationConfig {
        n_steps: args.steps,
        ..SimulationConfig::default()
    };
    let configs = segment.sim_configs(&base);

    println!("Building candidate: {}", args.candidate);
    let candidate = compile::build_and_load(&args.candidate, Slot::Zero)?;
    println!("Building reference: {}", args.reference);
    let reference = compile::build_and_load(&args.reference, Slot::One)?;

    println!(
        "Running {} simulations ({} steps each) on segment `{segment_name}`...",
        configs.len(),
        args.steps,
    );

    // Same `configs` reused for both runs (not regenerated) — that's what keeps the
    // comparison paired (docs/DESIGN.md §2.6's own note on this exact point).
    let candidate_result = candidate.run_batch(configs.clone())?;
    let reference_result = reference.run_batch(configs)?;

    compile::cleanup(candidate.build_dir.as_deref());
    compile::cleanup(reference.build_dir.as_deref());

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
        segment: segment_name.to_string(),
        n_sims: stat.n,
        n_steps: args.steps,
        execution_path: "native".to_string(),
    };
    let sections = vec![ReportSection {
        heading: "Paired comparison".to_string(),
        body,
    }];
    let path = report::write_report(Path::new(DEFAULT_REPORT_DIR), &meta, &sections)?;
    println!("Report written to {}", path.display());

    Ok(())
}
