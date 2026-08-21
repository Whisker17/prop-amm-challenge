use std::path::Path;

use clap::Args;
use prop_amm_shared::config::{SimulationConfig, BASELINE_STEPS};

use crate::commands::slug_from_source_path;
use crate::compile::{self, Slot};
use crate::config::BenchConfig;
use crate::grid::{self, GridCell};
use crate::report::{self, ReportMeta, ReportSection, DEFAULT_REPORT_DIR};
use crate::stats::{self, PairedStat};

/// `bench grid` — the 27-cell fragility matrix (docs/DESIGN.md §2.3). Paired by seed against
/// an operator-chosen reference, exactly like `compare`, but over grid mode's own balanced
/// factorial of regime corners (`config/bench.toml`'s `[grid]` table) rather than a sampled
/// segment.
#[derive(Args, Debug)]
pub struct GridArgs {
    /// Path to the candidate's .rs source file.
    #[arg(long)]
    candidate: String,
    /// Path to the reference's .rs source file.
    #[arg(long)]
    reference: String,
    /// Steps per simulation. Defaults to the challenge's own baseline, not a locally
    /// hardcoded copy of it (same rationale as `compare`'s `--steps`).
    #[arg(long, default_value_t = BASELINE_STEPS)]
    steps: u32,
}

struct CellResult {
    cell: GridCell,
    candidate_avg: f64,
    reference_avg: f64,
    stat: PairedStat,
}

pub fn run(args: GridArgs) -> anyhow::Result<()> {
    // WHI-1215: the stage names only the candidate under test, not the reference — unlike
    // `compare`'s two interchangeable comparison subjects, grid's own report already reads
    // as "one candidate's fragility, measured against an operator-chosen reference" (the
    // table below has one "candidate avg" column and one "reference avg" column, not a
    // symmetric pairing), and it's the candidate that varies day to day across strategies
    // (docs/DESIGN.md's own collision scenario: two different candidates gridded against
    // the same fixed reference on the same day).
    let candidate_slug = slug_from_source_path(&args.candidate)?;
    let stage = format!("grid-{candidate_slug}");

    // Fail fast, before any compiling/simulating, if today's report slot is already taken.
    report::ensure_report_slot_free(Path::new(DEFAULT_REPORT_DIR), &stage)?;

    let bench_config = BenchConfig::load_default()?;
    let grid_config = bench_config.grid()?;

    let base = SimulationConfig {
        n_steps: args.steps,
        ..SimulationConfig::default()
    };

    println!("Building candidate: {}", args.candidate);
    let candidate = compile::build_and_load(&args.candidate, Slot::Zero)?;
    println!("Building reference: {}", args.reference);
    let reference = compile::build_and_load(&args.reference, Slot::One)?;

    let cells = grid::cells(grid_config);
    println!(
        "Running {} cells x {} seeds ({} steps each)...",
        cells.len(),
        grid_config.seeds_per_cell,
        args.steps,
    );

    let mut results = Vec::with_capacity(cells.len());
    for cell in cells {
        let configs = cell.configs(&base, grid_config.seeds_per_cell);
        // Same `configs` reused for both runs (not regenerated) — keeps the per-cell
        // comparison paired, same rationale as `compare.rs`.
        let candidate_result = candidate.run_batch(configs.clone())?;
        let reference_result = reference.run_batch(configs)?;
        let stat = stats::paired_stat(&candidate_result.results, &reference_result.results)?;
        results.push(CellResult {
            cell,
            candidate_avg: candidate_result.avg_edge(),
            reference_avg: reference_result.avg_edge(),
            stat,
        });
    }

    // `candidate`/`reference`'s build dirs are removed on Drop, whenever this function
    // returns — success, an early `?`, or a `bail!` below (docs/DESIGN.md §3.4).

    let worst = results
        .iter()
        .min_by(|a, b| a.stat.mean_diff.total_cmp(&b.stat.mean_diff));
    if let Some(worst) = worst {
        println!(
            "Worst cell: {} mean diff {:.6} (candidate {:.2} vs reference {:.2})",
            worst.cell.label_axes(),
            worst.stat.mean_diff,
            worst.candidate_avg,
            worst.reference_avg,
        );
    }

    let body = format_cell_table(&args.candidate, &args.reference, &results);
    let meta = ReportMeta {
        stage,
        segment: "grid (not a config/bench.toml segment)".to_string(),
        n_sims: results.iter().map(|r| r.stat.n).sum(),
        n_steps: args.steps,
        execution_path: "native".to_string(),
    };
    let sections = vec![ReportSection {
        heading: "Grid fragility matrix — NOT comparable to a distribution-mode headline \
                  edge (docs/DESIGN.md §2.3)"
            .to_string(),
        body,
    }];
    let path = report::write_report(Path::new(DEFAULT_REPORT_DIR), &meta, &sections)?;
    println!("Report written to {}", path.display());

    Ok(())
}

fn format_cell_table(candidate: &str, reference: &str, results: &[CellResult]) -> String {
    let mut body = format!(
        "- Candidate: `{candidate}`\n- Reference: `{reference}`\n\n\
         Grid mode buys balanced coverage of regime corners a sampled distribution would \
         rarely visit (docs/DESIGN.md §2.3) — this table is a fragility check, not a ranking \
         input.\n\n\
         | cell | fee (bps) | liquidity mult | sigma | n | candidate avg | reference avg | \
         mean diff | 95% CI |\n\
         | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for r in results {
        body.push_str(&format!(
            "| {} | {} | {:.1} | {:.4} | {} | {:.2} | {:.2} | {:.4} | [{:.4}, {:.4}] |\n",
            r.cell.index,
            r.cell.norm_fee_bps,
            r.cell.norm_liquidity_mult,
            r.cell.gbm_sigma,
            r.stat.n,
            r.candidate_avg,
            r.reference_avg,
            r.stat.mean_diff,
            r.stat.ci_low,
            r.stat.ci_high,
        ));
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GridConfig;
    use crate::stats::tests::sample_stat;

    #[test]
    fn format_cell_table_names_files_and_includes_every_cell() {
        let grid_config = GridConfig {
            norm_fee_bps_levels: vec![30, 55, 80],
            norm_liquidity_mult_levels: vec![0.4, 1.0, 2.0],
            gbm_sigma_levels: vec![1e-4, 1e-3, 7e-3],
            seeds_per_cell: 40,
        };
        let cell = grid::cells(&grid_config)[0];
        let results = vec![CellResult {
            cell,
            candidate_avg: 1.0,
            reference_avg: 2.0,
            stat: sample_stat(40, -1.0),
        }];
        let body = format_cell_table("a.rs", "b.rs", &results);
        assert!(body.contains("Candidate: `a.rs`"));
        assert!(body.contains("Reference: `b.rs`"));
        assert!(body.contains("fragility check, not a ranking input"));
        assert!(body.contains(&format!("| {} |", cell.index)));
        assert!(body.contains("-1.0000"));
    }
}
