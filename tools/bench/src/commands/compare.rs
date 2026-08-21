use std::path::Path;

use clap::Args;
use prop_amm_shared::config::{SimulationConfig, BASELINE_STEPS};

use crate::commands::{note_if_not_decision_input, slug_from_source_path};
use crate::compile::{self, Slot};
use crate::config::{BenchConfig, SegmentSelector};
use crate::regime;
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
    /// Steps per simulation. Defaults to the challenge's own baseline
    /// (`prop_amm_shared::config::BASELINE_STEPS`), not a locally-hardcoded copy of it, so an
    /// upstream sync that moves the baseline changes this default too.
    #[arg(long, default_value_t = BASELINE_STEPS)]
    steps: u32,
}

pub fn run(args: CompareArgs) -> anyhow::Result<()> {
    // WHI-1215: unlike `grid`/`l1`, `compare`'s two sides are both first-class comparison
    // subjects — its own report tables "Candidate" and "Reference" side by side as equal
    // participants, not one target measured against a fixed opponent — so naming only one
    // side would still collide the day two different `compare` runs share just a candidate
    // or just a reference. `compare-<candidate>-vs-<reference>` disambiguates on both
    // deterministically from the two paths already being passed, so no extra
    // `--stage-suffix` flag is needed the way an ambiguous case might require.
    let candidate_slug = slug_from_source_path(&args.candidate)?;
    let reference_slug = slug_from_source_path(&args.reference)?;
    let stage = format!("compare-{candidate_slug}-vs-{reference_slug}");

    // Fail fast, before any compiling/simulating, if today's report slot is already taken.
    report::ensure_report_slot_free(Path::new(DEFAULT_REPORT_DIR), &stage)?;

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

    // `candidate`/`reference`'s build dirs are removed on Drop, whenever this function
    // returns — success, an early `?`, or a `bail!` below (docs/DESIGN.md §3.4).

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

    // Regime slicing (docs/DESIGN.md §2.3, §5): reconstructs each result's regime via
    // HyperparameterVariance::apply(&base, seed) — SimResult only carries (seed, edge), not
    // the sampled config — and reports a paired difference per populated bin beside the
    // headline above.
    let slices =
        regime::slice_paired_stats(&base, &candidate_result.results, &reference_result.results)?;
    println!("Regime slices: {} bins populated", slices.len());
    let slice_body = format_regime_slices(&slices);

    let meta = ReportMeta {
        stage,
        segment: segment_name.to_string(),
        n_sims: stat.n,
        n_steps: args.steps,
        execution_path: "native".to_string(),
    };
    let sections = vec![
        ReportSection {
            heading: "Paired comparison".to_string(),
            body,
        },
        ReportSection {
            heading: "Regime slices (docs/DESIGN.md §2.3, §5)".to_string(),
            body: slice_body,
        },
    ];
    let path = report::write_report(Path::new(DEFAULT_REPORT_DIR), &meta, &sections)?;
    println!("Report written to {}", path.display());

    Ok(())
}

fn format_regime_slices(slices: &[(regime::Regime, stats::PairedStat)]) -> String {
    let pooled_n: usize = slices.iter().map(|(_, stat)| stat.n).sum();
    let pooled_mean: f64 = if pooled_n == 0 {
        0.0
    } else {
        slices
            .iter()
            .map(|(_, stat)| stat.mean_diff * stat.n as f64)
            .sum::<f64>()
            / pooled_n as f64
    };

    let mut body = format!(
        "Bins reconstructed via `HyperparameterVariance::apply(&base, seed)`, equal-width \
         thirds of each axis's own sampling range (a bench-level reporting choice, not a \
         docs/DESIGN.md-specified boundary). Pooling every bin below reproduces the headline \
         paired mean: pooled={pooled_mean:.6} vs headline (see \"Paired comparison\" above).\n\n\
         | regime | n | mean diff | 95% CI |\n\
         | --- | --- | --- | --- |\n"
    );
    for (regime, stat) in slices {
        body.push_str(&format!(
            "| {} | {} | {:.6} | [{:.6}, {:.6}] |\n",
            regime.label(),
            stat.n,
            stat.mean_diff,
            stat.ci_low,
            stat.ci_high,
        ));
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::tests::sample_stat;

    #[test]
    fn format_regime_slices_reports_pooled_mean_and_every_bin() {
        let a = regime::classify_seed(&SimulationConfig::default(), 1);
        let b = regime::classify_seed(&SimulationConfig::default(), 2);
        let slices = vec![(a, sample_stat(2, 4.0)), (b, sample_stat(2, 2.0))];
        let body = format_regime_slices(&slices);
        assert!(body.contains("pooled=3.000000"));
        assert!(body.contains(&a.label()));
        assert!(body.contains(&b.label()));
    }
}
