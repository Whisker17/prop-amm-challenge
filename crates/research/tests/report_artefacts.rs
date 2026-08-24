//! End-to-end check that a `with-v3` run writes every artefact it promises, and
//! that the focused reports actually carry the Uniswap V3 baseline.
//!
//! These files are the deliverable. A regression that silently stops writing one
//! of them, or writes it without the V3 rows, is invisible in a unit test of the
//! formatter — the formatter would still be correct, it would just never be
//! called. So this drives the real pipeline: run a short batch, write into a
//! temporary directory, and read what lands there.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use prop_amm_research::experiment::{self, BatchConfig, Competitor};
use prop_amm_research::metrics::StrategySummary;
use prop_amm_research::paired;
use prop_amm_research::probe;
use prop_amm_research::report::{self, Artefacts, RunMeta};
use prop_amm_research::strategies::{self, StrategySet};

/// A scratch directory that removes itself, so a failing assertion cannot leave
/// output behind that the next run would read as fresh.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let mut path = std::env::temp_dir();
        path.push(format!("prop-amm-research-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch dir");
        Scratch(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run a short `with-v3` batch and write every artefact into `dir`.
fn run_and_write(dir: &Path) -> (Vec<PathBuf>, Vec<StrategySummary>) {
    let batch = BatchConfig {
        simulations: 4,
        steps: 300,
        seed_start: 0,
        seed_stride: 1,
        competitor: Competitor::Normalizer,
        workers: 1,
    };
    let strategies = strategies::strategies_for(StrategySet::WithV3);
    let results = experiment::run_all(&strategies, &batch).expect("batch");

    let mut summaries = Vec::new();
    let mut all_runs = Vec::new();
    for (strategy, runs) in &results {
        summaries.push(StrategySummary::from_runs(
            &strategy.id,
            strategy.family.as_str(),
            &strategy.parameter,
            runs,
        ));
        all_runs.extend(runs.iter().cloned());
    }

    let flashbots_minus_dodo = paired::flashbots_minus_dodo(&results);
    let versus_univ2 = paired::versus_univ2(&results);
    let attributions = paired::attribute_versus_univ2(&results);
    let univ3_attributions = paired::attribute_univ3_concentration(&results);
    let versus_univ3 = paired::versus_univ3_full_range(&results);
    let univ3_minus_univ2 = paired::univ3_full_range_minus_univ2(&results);
    let univ3_capacity_deltas = paired::univ3_capacity_deltas(&results);
    let sanity = paired::univ3_full_range_sanity(&results);

    let meta = RunMeta::from_batch_with_set(&batch, 1.0, StrategySet::WithV3);
    let quote_rows = probe::quote_matrix(
        &strategies,
        meta.initial_price,
        meta.initial_x,
        meta.initial_y,
        &probe::default_buy_sizes_y(),
        &probe::default_sell_sizes_x(),
    );

    let written = report::write_all(
        dir,
        &meta,
        &Artefacts {
            summaries: &summaries,
            runs: &all_runs,
            flashbots_minus_dodo: &flashbots_minus_dodo,
            versus_univ2: &versus_univ2,
            attributions: &attributions,
            univ3_attributions: &univ3_attributions,
            versus_univ3: &versus_univ3,
            univ3_minus_univ2: &univ3_minus_univ2,
            univ3_capacity_deltas: &univ3_capacity_deltas,
            quote_rows: &quote_rows,
            univ3_sanity: sanity.as_ref(),
        },
    )
    .expect("write_all");

    (written, summaries)
}

fn read(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name))
        .unwrap_or_else(|error| panic!("{name} was not written: {error}"))
}

#[test]
fn a_with_v3_run_writes_every_promised_artefact() {
    let scratch = Scratch::new("artefacts");
    let (written, _) = run_and_write(scratch.path());

    let names: HashSet<String> = written
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();

    for expected in [
        "summary.json",
        "summary.csv",
        "runs.csv",
        "paired-stats.json",
        "paired-deltas.csv",
        "attribution.csv",
        "REPORT.zh-CN.md",
        // the focused reports
        "REPORT-dodo-vs-flashbots.zh-CN.md",
        "dodo-vs-flashbots.csv",
        "dodo-vs-flashbots.json",
        "REPORT-vs-baselines.zh-CN.md",
        "versus-baselines.csv",
        "versus-baselines.json",
        // V3-only, present because this run included the V3 arms
        "univ3-capacity.csv",
        "univ3-attribution.csv",
    ] {
        assert!(
            names.contains(expected),
            "{expected} was not written; got {names:?}"
        );
        assert!(
            !read(scratch.path(), expected).trim().is_empty(),
            "{expected} was written empty"
        );
    }
}

#[test]
fn the_baselines_report_carries_the_univ3_baseline() {
    let scratch = Scratch::new("baselines");
    run_and_write(scratch.path());
    let markdown = read(scratch.path(), "REPORT-vs-baselines.zh-CN.md");

    assert!(
        markdown.contains("full-range Uniswap V3"),
        "the V3 baseline section is missing"
    );
    assert!(
        markdown.contains("univ3-full-range-zero-fee"),
        "the V3 baseline row uses the wrong strategy id"
    );
    assert!(
        markdown.contains("Sanity gate"),
        "the sanity gate section is missing"
    );
    // The gate must be stated as a derived tolerance, never as exact equality.
    assert!(
        markdown.contains("无任何拟合常数"),
        "the tolerance must be shown as derived"
    );
    assert!(
        !markdown.contains("期望值是 0"),
        "the report must not claim the residual is exactly zero"
    );

    let csv = read(scratch.path(), "versus-baselines.csv");
    assert!(
        csv.lines()
            .any(|line| line.contains("univ3-full-range-zero-fee")),
        "versus-baselines.csv has no full-range V3 row"
    );
    assert!(
        csv.lines()
            .any(|line| line.contains(",univ3-full-range-zero-fee,")),
        "versus-baselines.csv never uses the full-range V3 arm as a baseline"
    );
}

#[test]
fn the_dodo_vs_flashbots_report_is_driven_by_paired_differences() {
    let scratch = Scratch::new("head-to-head");
    run_and_write(scratch.path());
    let markdown = read(scratch.path(), "REPORT-dodo-vs-flashbots.zh-CN.md");

    assert!(markdown.contains("DODO"));
    assert!(markdown.contains("Flashbots"));
    let json = read(scratch.path(), "dodo-vs-flashbots.json");
    assert!(json.contains("\"deltas\""), "the JSON carries no deltas");

    let csv = read(scratch.path(), "dodo-vs-flashbots.csv");
    assert!(
        csv.lines().count() > 1,
        "dodo-vs-flashbots.csv has only a header"
    );
}

/// The capacity CSV must name its quote-probe columns as such, so a reader
/// cannot mistake a search diagnostic for an order count.
#[test]
fn the_capacity_csv_separates_quote_probes_from_order_counts() {
    let scratch = Scratch::new("capacity");
    run_and_write(scratch.path());
    let csv = read(scratch.path(), "univ3-capacity.csv");
    let header = csv.lines().next().expect("header");

    for column in [
        "retail_orders_probed",
        "retail_full_order_capacity_limited_count",
        "retail_capacity_shortfall_notional_y_mean",
        "retail_notional_served_mean",
        "quote_capacity_reject_count",
        "quote_rejected_requested_y_mean",
        "quote_rejected_requested_x_mean",
    ] {
        assert!(header.contains(column), "missing column {column}");
    }
    // The old, wrong names must not come back.
    assert!(!header.contains("unfilled_retail_notional"));
    assert!(
        !header.contains(",capacity_reject_count"),
        "the unqualified reject count reads as an order count"
    );

    // X and Y sums stay in separate columns and are never combined.
    assert!(header.contains("quote_canonical_unfilled_y_mean"));
    assert!(header.contains("quote_canonical_unfilled_x_mean"));

    // Every row must be well formed even though `parameter` contains commas.
    let columns = header.split(',').count();
    for (index, line) in csv.lines().enumerate().skip(1) {
        let fields = csv_columns(line);
        assert_eq!(fields, columns, "row {index} has {fields} of {columns}");
    }
}

/// RFC 4180 field count: commas inside a quoted field do not split it.
fn csv_columns(line: &str) -> usize {
    let mut count = 1;
    let mut quoted = false;
    for character in line.chars() {
        match character {
            '"' => quoted = !quoted,
            ',' if !quoted => count += 1,
            _ => {}
        }
    }
    count
}

/// The full-range V3 arm must never be capacity-limited, and the gate must say
/// so rather than leaving it implicit.
#[test]
fn the_sanity_gate_passes_on_a_short_run() {
    let batch = BatchConfig {
        simulations: 4,
        steps: 300,
        seed_start: 0,
        seed_stride: 1,
        competitor: Competitor::Normalizer,
        workers: 1,
    };
    let strategies = strategies::strategies_for(StrategySet::WithV3);
    let results = experiment::run_all(&strategies, &batch).expect("batch");
    let gate = paired::univ3_full_range_sanity(&results).expect("gate");

    assert_eq!(
        gate.full_range_capacity_limited_orders, 0,
        "a full-range position cannot run out of room"
    );
    assert_eq!(gate.samples, 4, "every seed must be paired");
    assert!(
        gate.tolerance > 0.0,
        "the derived tolerance must be a real bound, not zero"
    );
    assert!(
        gate.passed(),
        "{} (residual distribution: mean {}, min {}, max {})",
        gate.summary_line(),
        gate.residuals.mean,
        gate.residuals.min,
        gate.residuals.max
    );
}

/// The tolerance mirrors two private constants in `prop_amm_sim`. If either
/// changes, the derivation silently stops describing the simulation. There is no
/// way to read a private const, so this pins the *consequence*: the search terms
/// must dominate the quantisation term by orders of magnitude, which is only
/// true while the mirrored values are right.
#[test]
fn the_sanity_tolerance_terms_are_ordered_as_derived() {
    use prop_amm_research::paired::{tolerance_terms, ARB_INPUT_REL_TOL, NANO, ROUTER_ALPHA_TOL};

    let batch = BatchConfig {
        simulations: 4,
        steps: 500,
        seed_start: 0,
        seed_stride: 1,
        competitor: Competitor::Normalizer,
        workers: 1,
    };
    let strategy = strategies::strategy_by_id("univ3-full-range-zero-fee").unwrap();
    let configs = experiment::seed_configs(&batch);
    let run = experiment::run_single(&strategy, Competitor::Normalizer, &configs[0]);
    let terms = tolerance_terms(&run);

    assert!(terms.quantisation > 0.0 && terms.arb_search > 0.0);
    assert!(
        terms.arb_search > terms.quantisation * 100.0,
        "the arb search term ({}) should dominate quantisation ({}) by orders of \
         magnitude; if it no longer does, ARB_INPUT_REL_TOL={ARB_INPUT_REL_TOL} or \
         NANO={NANO} has drifted from prop_amm_sim",
        terms.arb_search,
        terms.quantisation
    );
    assert!(
        terms.arb_search > terms.router_search,
        "the arbitrageur searches coarser than the router \
         ({ARB_INPUT_REL_TOL} vs {ROUTER_ALPHA_TOL}), so its term must be larger"
    );
    assert!(
        (terms.total() - (terms.quantisation + terms.arb_search + terms.router_search)).abs()
            < f64::EPSILON,
        "total must be the sum of the terms"
    );
}
