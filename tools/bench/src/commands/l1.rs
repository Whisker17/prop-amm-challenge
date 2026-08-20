use std::path::Path;

use clap::Args;
use prop_amm_shared::config::{SimulationConfig, BASELINE_STEPS};

use crate::commands::note_if_not_decision_input;
use crate::compile::{self, Slot};
use crate::config::{BenchConfig, SegmentSelector};
use crate::report::{self, ReportMeta, ReportSection, DEFAULT_REPORT_DIR};
use crate::telemetry::{self, L1Sim};

const STAGE: &str = "l1";
const DEFAULT_STARTER: &str = "programs/starter/src/lib.rs";
const DEFAULT_NORMALIZER_AS_SUBMISSION: &str = "strategies/000-normalizer/lib.rs";

/// `bench l1` — L1 observability (docs/DESIGN.md §2.7): flow share and edge per unit volume,
/// derived from non-invasive `after_swap` recorders on both AMMs. Single-candidate against
/// the fixed normalizer opponent (like `anchor`), not a paired comparison — flow share is a
/// property of one submission's competition with the router's fixed competitor, not a
/// diff between two submissions.
#[derive(Args, Debug)]
pub struct L1Args {
    /// Path(s) to the .rs source file(s) to measure. Repeatable. Defaults to both of
    /// docs/DESIGN.md §2.8's non-fitted baselines (the starter and the normalizer-as-
    /// submission self-check) so a single run produces "a `results/` snapshot covering the
    /// starter and the normalizer reference" in one report, without colliding with itself
    /// on `report.rs`'s one-report-per-stage-per-day slot.
    #[arg(long = "file", default_values = [DEFAULT_STARTER, DEFAULT_NORMALIZER_AS_SUBMISSION])]
    files: Vec<String>,
    #[command(flatten)]
    segment_selector: SegmentSelector,
    /// Steps per simulation. Defaults to the challenge's own baseline, not a locally
    /// hardcoded copy of it (same rationale as `compare`'s `--steps`).
    #[arg(long, default_value_t = BASELINE_STEPS)]
    steps: u32,
}

struct Aggregate {
    submission_edge_total: f64,
    submission_volume_total: f64,
    normalizer_volume_total: f64,
    submission_trades_total: u64,
    normalizer_trades_total: u64,
    per_sim_flow_shares: Vec<f64>,
    per_sim_edge_per_volumes: Vec<f64>,
    zero_volume_sims: usize,
}

fn aggregate(l1_sims: &[L1Sim]) -> Aggregate {
    let mut agg = Aggregate {
        submission_edge_total: 0.0,
        submission_volume_total: 0.0,
        normalizer_volume_total: 0.0,
        submission_trades_total: 0,
        normalizer_trades_total: 0,
        per_sim_flow_shares: Vec::new(),
        per_sim_edge_per_volumes: Vec::new(),
        zero_volume_sims: 0,
    };
    for sim in l1_sims {
        agg.submission_edge_total += sim.submission_edge;
        agg.submission_volume_total += sim.submission_volume;
        agg.normalizer_volume_total += sim.normalizer_volume;
        agg.submission_trades_total += sim.submission_trades;
        agg.normalizer_trades_total += sim.normalizer_trades;
        match telemetry::flow_share(sim.submission_volume, sim.normalizer_volume) {
            Some(share) => agg.per_sim_flow_shares.push(share),
            None => agg.zero_volume_sims += 1,
        }
        if let Some(epv) = telemetry::edge_per_volume(sim.submission_edge, sim.submission_volume) {
            agg.per_sim_edge_per_volumes.push(epv);
        }
    }
    agg
}

fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

fn format_section(file: &str, n_sims: usize, agg: &Aggregate) -> ReportSection {
    // `telemetry::flow_share` is a ratio of non-negative volumes, so any `Some` value it
    // returns is already guaranteed within [0,1] (proven in telemetry.rs's own tests) — no
    // separate bounds check needed here.
    let aggregate_flow_share =
        telemetry::flow_share(agg.submission_volume_total, agg.normalizer_volume_total);
    let aggregate_epv =
        telemetry::edge_per_volume(agg.submission_edge_total, agg.submission_volume_total);

    let body = format!(
        "- Simulations: {n_sims} (zero-volume: {})\n\
         - Submission: {} trades, {:.2} Y volume, {:.2} total edge\n\
         - Normalizer: {} trades, {:.2} Y volume\n\
         - Aggregate flow share (pooled volume): {}\n\
         - Mean per-simulation flow share: {}\n\
         - Aggregate edge per unit volume: {}\n\
         - Mean per-simulation edge per unit volume: {}\n",
        agg.zero_volume_sims,
        agg.submission_trades_total,
        agg.submission_volume_total,
        agg.submission_edge_total,
        agg.normalizer_trades_total,
        agg.normalizer_volume_total,
        fmt_opt(aggregate_flow_share),
        fmt_opt(mean(&agg.per_sim_flow_shares)),
        fmt_opt(aggregate_epv),
        fmt_opt(mean(&agg.per_sim_edge_per_volumes)),
    );

    ReportSection {
        heading: format!("L1 telemetry — `{file}`"),
        body,
    }
}

fn fmt_opt(value: Option<f64>) -> String {
    match value {
        Some(v) => format!("{v:.6}"),
        None => "undefined (no recorded volume)".to_string(),
    }
}

pub fn run(args: L1Args) -> anyhow::Result<()> {
    if args.files.is_empty() {
        anyhow::bail!("bench l1 requires at least one --file");
    }

    // Fail fast, before any compiling/simulating, if today's report slot is already taken.
    report::ensure_report_slot_free(Path::new(DEFAULT_REPORT_DIR), STAGE)?;

    let bench_config = BenchConfig::load_default()?;
    let (segment_name, segment) = args.segment_selector.resolve(&bench_config)?;
    note_if_not_decision_input(segment_name, segment);

    let base = SimulationConfig {
        n_steps: args.steps,
        ..SimulationConfig::default()
    };

    // Built once: every file in this run shares the same segment, so the sampled configs
    // are identical for each — only cloned per file, not recomputed (each
    // `run_batch_with_l1` call consumes its `Vec<SimulationConfig>`).
    let configs = segment.sim_configs(&base);
    let mut sections = Vec::with_capacity(args.files.len());

    for file in &args.files {
        println!("Building {file}...");
        let loaded = compile::build_and_load(file, Slot::Zero)?;

        println!(
            "Running {} simulations ({} steps each) on segment `{segment_name}` with L1 \
             telemetry...",
            configs.len(),
            args.steps,
        );
        let (batch, l1_sims) = loaded.run_batch_with_l1(configs.clone())?;
        // `loaded`'s build dir is removed on Drop at the end of this iteration
        // (docs/DESIGN.md §3.4).

        let agg = aggregate(&l1_sims);
        println!(
            "{file}: avg edge {:.2}, aggregate flow share {}",
            batch.avg_edge(),
            fmt_opt(telemetry::flow_share(
                agg.submission_volume_total,
                agg.normalizer_volume_total
            )),
        );

        sections.push(format_section(file, batch.n_sims(), &agg));
    }

    // `n_sims` is the segment's own size (identical for every file measured above), not a
    // sum across files — a report covering N files still ran N independent measurements of
    // the same `segment_name`-sized segment, not one measurement N times as large
    // (docs/DESIGN.md §3.3's provenance rule: a reported number must state what it actually
    // measures).
    let meta = ReportMeta {
        stage: STAGE.to_string(),
        segment: segment_name.to_string(),
        n_sims: configs.len(),
        n_steps: args.steps,
        execution_path: "native".to_string(),
    };
    let path = report::write_report(Path::new(DEFAULT_REPORT_DIR), &meta, &sections)?;
    println!("Report written to {}", path.display());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l1(seed: u64, edge: f64, sub_vol: f64, norm_vol: f64) -> L1Sim {
        L1Sim {
            seed,
            submission_edge: edge,
            submission_trades: if sub_vol > 0.0 { 1 } else { 0 },
            submission_volume: sub_vol,
            normalizer_trades: if norm_vol > 0.0 { 1 } else { 0 },
            normalizer_volume: norm_vol,
        }
    }

    #[test]
    fn aggregate_sums_across_sims_and_flags_zero_volume() {
        let sims = vec![l1(1, 10.0, 5.0, 5.0), l1(2, 3.0, 0.0, 0.0)];
        let agg = aggregate(&sims);
        assert_eq!(agg.zero_volume_sims, 1);
        assert_eq!(agg.submission_volume_total, 5.0);
        assert_eq!(agg.normalizer_volume_total, 5.0);
        assert_eq!(agg.per_sim_flow_shares.len(), 1);
        assert_eq!(agg.per_sim_flow_shares[0], 0.5);
    }

    #[test]
    fn format_section_reports_flow_share_in_bounds() {
        let sims = vec![l1(1, 10.0, 3.0, 1.0)];
        let agg = aggregate(&sims);
        let section = format_section("test.rs", 1, &agg);
        assert!(section.body.contains("0.750000"));
    }
}
