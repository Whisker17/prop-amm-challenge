//! `bench fuzz` (WHI-1212, docs/DESIGN.md §2.9) — the pre-search shape-fuzz gate. Hammers a
//! candidate with dense sweeps and golden-section-shaped sample sets, across every
//! `config/bench.toml` `[grid]` regime corner plus states reachable only after a
//! full-length GBM drift, and against a zeroed and a random-byte storage variant of each —
//! meant to run before a `bench fit` search ever spends paired-seed budget on this
//! candidate, since `prop-amm validate`'s own 10-point, single-state probe is far weaker
//! than what a real 1000-sim run exercises.
//!
//! Unlike `bench parity`/`bench grid`, a **PASS** run writes no `results/` report — this
//! gate is meant to run before *every* frozen search, possibly several times a day on the
//! same strategy, and `report.rs`'s one-report-per-`(day, stage)` rule would otherwise make
//! the second same-day PASS run fail outright on a stale slot. A **violation**, by contrast,
//! is exactly the evidence issue WHI-1212 asks to commit ("a failing state is the evidence a
//! `wontfix` needs under §2.9") — so only the failure path writes a report.

use std::path::Path;

use clap::Args;

use crate::config::BenchConfig;
use crate::fast_compile;
use crate::fuzz::{self, Violation};
use crate::report::{self, ReportMeta, ReportSection, DEFAULT_REPORT_DIR};

const STAGE_PREFIX: &str = "fuzz";

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
        return Ok(());
    };

    println!(
        "  [FAIL] shape violation: state [{state_label}], side [{side_label}], sample kind \
         [{sample_kind}]: {message}"
    );

    // Commit the violating state as evidence (WHI-1212 item 6, docs/DESIGN.md §2.9): "a
    // failing state is the evidence a `wontfix` needs". A same-day report slot already
    // taken (e.g. by an earlier violation found today) is not itself a reason to swallow
    // *this* violation — note it and move on to the `bail!` below, which is the actual
    // failure this command reports.
    let meta = ReportMeta {
        stage,
        segment: "fuzz (not a config/bench.toml segment)".to_string(),
        n_sims: states.len() * 2,
        n_steps: 0,
        execution_path: "native (fast path)".to_string(),
    };
    let sections = vec![ReportSection {
        heading: "bench fuzz — shape violation found (WHI-1212)".to_string(),
        body: format!(
            "- Candidate: `{file_str}`\n- State: {state_label}\n- Side: {side_label}\n\
             - Sample kind: {sample_kind}\n- Message: {message}\n",
        ),
    }];
    match report::write_report(Path::new(DEFAULT_REPORT_DIR), &meta, &sections) {
        Ok(path) => println!("Violation evidence written to {}", path.display()),
        Err(e) => println!("(violation evidence not written: {e})"),
    }

    anyhow::bail!(
        "shape violation found — this is a blocker, not a note (docs/DESIGN.md §2.9): \
         state [{state_label}], side [{side_label}], sample kind [{sample_kind}]: {message}"
    );
}
