use std::path::Path;

use clap::Args;
use prop_amm_shared::config::SimulationConfig;

use crate::commands::{
    edges_agree, note_if_not_decision_input, resolve_strategy_lib_path, run_prop_amm,
    run_prop_amm_validate,
};
use crate::compile::{self, Slot};
use crate::config::{BenchConfig, SegmentSelector};
use crate::fast_compile;
use crate::report::{self, ReportMeta, ReportSection, DEFAULT_REPORT_DIR};

/// The relative bound WHI-1194's acceptance criteria set between the fast path and the
/// reference path ("fast-path and CLI avg edge agree to `1e-9` relative on the same
/// segment") — docs/DESIGN.md §2.6 itself only requires the two paths match **per seed**,
/// without naming a tolerance; `1e-9` is this command's own choice of how tight "match"
/// means. Both sides are computed here as full-`f64` in-process `BatchResult`s — this bound
/// is *not* sourced from `prop-amm run`'s stdout, which prints only 2 decimal places; see
/// this module's doc comment on `run`.
const REL_TOL: f64 = 1e-9;

/// `bench parity` — the acceptance gate of docs/DESIGN.md §2.6: a strategy's committed
/// parameter point must be reproduced through `prop-amm validate` and `prop-amm run`, and
/// the fast path must agree with the reference path **per seed**, not merely in aggregate.
///
/// The 1e-9 per-seed check and the `prop-amm run` reproduction are two different things: the
/// former is only obtainable by comparing two full-precision in-process builds (fast path vs
/// reference path), since the CLI's own stdout is capped at 2 decimal places
/// (`docs/DEFERRED_ISSUES.md`'s existing note on `anchor.rs`'s equivalent bound); the latter
/// is what actually gets cited in a strategy's `NOTES.md`.
#[derive(Args, Debug)]
pub struct ParityArgs {
    /// Path to the strategy directory (e.g. `strategies/001-cpmm-fee`).
    #[arg(long)]
    strategy: String,
    #[command(flatten)]
    segment_selector: SegmentSelector,
}

pub fn run(args: ParityArgs) -> anyhow::Result<()> {
    let (slug, file) = resolve_strategy_lib_path(&args.strategy)?;
    let stage = format!("parity-{slug}");
    let file_str = file.to_string_lossy().to_string();

    // Fail fast, before any compiling/simulating, if today's report slot is already taken.
    report::ensure_report_slot_free(Path::new(DEFAULT_REPORT_DIR), &stage)?;

    let bench_config = BenchConfig::load_default()?;
    let (segment_name, segment) = args.segment_selector.resolve(&bench_config)?;
    note_if_not_decision_input(segment_name, segment);

    println!("Validating {file_str} via `prop-amm validate`...");
    run_prop_amm_validate(&file_str)?;
    println!("  [PASS] prop-amm validate");

    let source = std::fs::read_to_string(&file)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", file.display()))?;
    let safe_source = fast_compile::make_safe_source(&source)?;

    let base = SimulationConfig::default();
    let configs = segment.sim_configs(&base);

    println!("Building via the fast path...");
    let fast = fast_compile::compile_and_load_fast(&safe_source)?;
    println!("Building via the reference path...");
    let reference = compile::build_and_load(&file_str, Slot::Zero)?;

    println!(
        "Running {} simulations ({} steps each) on segment `{segment_name}` through both paths...",
        configs.len(),
        base.n_steps,
    );
    let fast_batch = fast.run_batch(configs.clone())?;
    let reference_batch = reference.run_batch(configs)?;

    // `reference`'s build dir is removed on Drop, whenever this function returns — success,
    // an early `?`, or a `bail!` below (docs/DESIGN.md §3.4). `.build/fast/` is never
    // removed — it is meant to persist and be reused by the next `fit`/`parity` run.

    // `zip` silently truncates to the shorter side, which would hide a batch that came back
    // short rather than merely misaligned — check equal length explicitly first (mirrors
    // `stats::paired_stat`'s own guard for the same reason).
    if fast_batch.results.len() != reference_batch.results.len() {
        anyhow::bail!(
            "fast-path/reference-path batches have different lengths: {} vs {}",
            fast_batch.results.len(),
            reference_batch.results.len()
        );
    }

    let mut max_rel_diff = 0.0_f64;
    let mut mismatches = Vec::new();
    for (f, r) in fast_batch
        .results
        .iter()
        .zip(reference_batch.results.iter())
    {
        if f.seed != r.seed {
            anyhow::bail!(
                "fast-path/reference-path batches are unpaired: seed {} vs {}",
                f.seed,
                r.seed
            );
        }
        let diff = (f.submission_edge - r.submission_edge).abs();
        let scale = r.submission_edge.abs().max(1e-12);
        let rel_diff = diff / scale;
        max_rel_diff = max_rel_diff.max(rel_diff);
        if rel_diff > REL_TOL {
            mismatches.push((f.seed, f.submission_edge, r.submission_edge, rel_diff));
        }
    }

    if !mismatches.is_empty() {
        anyhow::bail!(
            "{} of {} seeds disagree between fast path and reference path beyond {REL_TOL:e} \
             relative — this is a blocker, not a note (docs/DESIGN.md §2.6): {}",
            mismatches.len(),
            fast_batch.n_sims(),
            mismatches
                .iter()
                .take(10)
                .map(|(seed, f, r, rel)| format!(
                    "(seed {seed}, fast={f:.9}, reference={r:.9}, rel_diff={rel:e})"
                ))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    println!(
        "Per-seed parity: {}/{} seeds agree within {REL_TOL:e} relative (max observed {max_rel_diff:e}).",
        fast_batch.n_sims(),
        fast_batch.n_sims(),
    );

    println!("Cross-checking aggregate against `prop-amm run`...");
    let segment_count_u32 = u32::try_from(segment.count).map_err(|_| {
        anyhow::anyhow!(
            "segment `{segment_name}` has count {} which doesn't fit in the CLI's --simulations u32",
            segment.count
        )
    })?;
    let cli_aggregate = run_prop_amm(
        &file_str,
        segment_count_u32,
        base.n_steps,
        segment.start,
        segment.stride,
    )?;
    if !edges_agree(fast_batch.avg_edge(), cli_aggregate.avg) {
        anyhow::bail!(
            "aggregate mismatch: fast-path avg={:.2} vs `prop-amm run` avg={:.2}",
            fast_batch.avg_edge(),
            cli_aggregate.avg,
        );
    }
    println!(
        "Aggregate agreement: fast-path avg={:.2} matches `prop-amm run` avg={:.2}",
        fast_batch.avg_edge(),
        cli_aggregate.avg,
    );

    let meta = ReportMeta {
        stage: stage.clone(),
        segment: segment_name.to_string(),
        n_sims: fast_batch.n_sims(),
        n_steps: base.n_steps,
        execution_path: "native (fast path vs reference path)".to_string(),
    };
    let sections = vec![
        ReportSection {
            heading: "prop-amm validate".to_string(),
            body: "PASS".to_string(),
        },
        ReportSection {
            heading: "Per-seed parity (fast path vs reference path)".to_string(),
            body: format!(
                "- Segment: `{segment_name}`, n={}\n- Max observed relative diff: {max_rel_diff:e}\n- Tolerance: {REL_TOL:e}\n- Result: all seeds agree\n",
                fast_batch.n_sims(),
            ),
        },
        ReportSection {
            heading: "Aggregate parity (fast path vs `prop-amm run`)".to_string(),
            body: format!(
                "- Fast path: avg edge {:.2}\n- `prop-amm run`: avg edge {:.2}, total edge {:.2}\n",
                fast_batch.avg_edge(),
                cli_aggregate.avg,
                cli_aggregate.total,
            ),
        },
    ];
    let path = report::write_report(Path::new(DEFAULT_REPORT_DIR), &meta, &sections)?;
    println!("Report written to {}", path.display());

    Ok(())
}
