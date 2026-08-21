use std::path::Path;

use clap::Args;

use crate::config::BenchConfig;
use crate::fast_compile;
use crate::fuzz::{self, Violation};
use crate::report::{self, ReportMeta, ReportSection, DEFAULT_REPORT_DIR};

const STAGE_PREFIX: &str = "fuzz";

/// `bench fuzz` (WHI-1212, docs/DESIGN.md §2.9) — the pre-search shape-fuzz gate. Hammers a
/// candidate with dense sweeps and golden-section-shaped sample sets, across every
/// `config/bench.toml` `[grid]` regime corner plus states reachable only after a
/// full-length GBM drift, and against a zeroed and a random-byte storage variant of each —
/// meant to run before a `bench fit` search ever spends paired-seed budget on this
/// candidate, since `prop-amm validate`'s own 10-point, single-state probe is far weaker
/// than what a real 1000-sim run exercises.
#[derive(Args, Debug)]
pub struct FuzzArgs {
    /// Path to the strategy directory (e.g. `strategies/001-cpmm-fee`).
    #[arg(long)]
    strategy: String,
}

pub fn run(args: FuzzArgs) -> anyhow::Result<()> {
    let strategy_dir = Path::new(&args.strategy);
    let slug = strategy_dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "`--strategy` must be a directory path, got `{}`",
                args.strategy
            )
        })?;
    let stage = format!("{STAGE_PREFIX}-{slug}");
    let file = strategy_dir.join("lib.rs");
    let file_str = file.to_string_lossy().to_string();

    // Fail fast, before any compiling/fuzzing, if today's report slot is already taken.
    report::ensure_report_slot_free(Path::new(DEFAULT_REPORT_DIR), &stage)?;

    let bench_config = BenchConfig::load_default()?;
    let grid_config = bench_config.grid()?;
    let fuzz_config = bench_config.fuzz()?;

    let source = std::fs::read_to_string(&file)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", file.display()))?;
    let safe_source = fast_compile::make_safe_source(&source)?;

    println!("Building {file_str} via the fast path...");
    let loaded = fast_compile::compile_and_load_fast(&safe_source)?;

    let states = fuzz::build_states(grid_config, fuzz_config);
    println!(
        "Fuzzing {file_str} across {} states x 2 sides ({} dense-sweep points/grid, {} \
         golden-section fair-price multipliers)...",
        states.len(),
        fuzz_config.dense_sweep_points,
        fuzz_config.golden_price_multipliers.len(),
    );

    let violation = fuzz::run_fuzz(&loaded, &states, fuzz_config);

    // `loaded`'s temp dylib copy is removed on Drop whenever this function returns —
    // success, an early `?`, or the `bail!` below (docs/DESIGN.md §3.4).

    let Some(Violation {
        state_label,
        side_label,
        sample_kind,
        message,
    }) = violation
    else {
        println!(
            "  [PASS] Zero shape violations across {} states x 2 sides.",
            states.len()
        );
        let meta = ReportMeta {
            stage: stage.clone(),
            segment: "fuzz (not a config/bench.toml segment)".to_string(),
            n_sims: 0,
            n_steps: 0,
            execution_path: "native (fast path)".to_string(),
        };
        let sections = vec![ReportSection {
            heading: "bench fuzz — pre-search shape-fuzz gate (WHI-1212)".to_string(),
            body: format!(
                "- Candidate: `{file_str}`\n- States probed: {}\n- Sides probed: buy X, sell X\n\
                 - Dense-sweep points per grid: {}\n- Golden-section fair-price multipliers: {:?}\n\
                 - Result: zero shape violations\n",
                states.len(),
                fuzz_config.dense_sweep_points,
                fuzz_config.golden_price_multipliers,
            ),
        }];
        let path = report::write_report(Path::new(DEFAULT_REPORT_DIR), &meta, &sections)?;
        println!("Report written to {}", path.display());
        return Ok(());
    };

    anyhow::bail!(
        "shape violation found — this is a blocker, not a note (docs/DESIGN.md §2.9): \
         state [{state_label}], side [{side_label}], sample kind [{sample_kind}]: {message}"
    );
}
