use clap::{Parser, Subcommand};

use crate::commands::{anchor, ceiling, compare, estimator_probe, fit, fuzz, grid, l1, parity};

#[derive(Parser)]
#[command(name = "bench", about = "Prop AMM Challenge measurement layer")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

// Adding a new subcommand means adding a variant here plus a new `commands/<name>.rs` — no
// existing subcommand's file needs to change.
#[derive(Subcommand)]
enum Commands {
    /// Paired-by-seed comparison of a candidate against a reference.
    Compare(compare::CompareArgs),
    /// Cross-check bench's own numbers against a real `prop-amm run`.
    Anchor(anchor::AnchorArgs),
    /// Coarse-grid-then-coordinate-descent search over a strategy's PARAMS block.
    Fit(fit::FitArgs),
    /// Reproduce a strategy's committed point through `prop-amm validate`/`run` and diff
    /// against the fast path.
    Parity(parity::ParityArgs),
    /// The 27-cell fragility matrix over grid mode's own regime corners.
    Grid(grid::GridArgs),
    /// L1 observability: flow share and edge per unit volume for one candidate.
    L1(l1::L1Args),
    /// Pre-search shape-fuzz gate: hammer a candidate harder than `prop-amm validate` does,
    /// before a frozen search spends budget on it (WHI-1212).
    Fuzz(fuzz::FuzzArgs),
    /// WHI-1225's pre-registered "Probe A": replicate `005`'s two variance normalizations and
    /// `004`'s ewma_vol-vs-floor distribution from a real run, before any search spends
    /// budget.
    EstimatorProbe(estimator_probe::EstimatorProbeArgs),
    /// Out-of-competition ceiling lane (WHI-1247): host-side oracle re-anchor curve vs the
    /// 0-line — never submittable, never ranked (`ceilings/README.md`).
    Ceiling(ceiling::CeilingArgs),
}

pub fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Compare(args) => compare::run(args),
        Commands::Anchor(args) => anchor::run(args),
        Commands::Fit(args) => fit::run(args),
        Commands::Parity(args) => parity::run(args),
        Commands::Grid(args) => grid::run(args),
        Commands::L1(args) => l1::run(args),
        Commands::Fuzz(args) => fuzz::run(args),
        Commands::EstimatorProbe(args) => estimator_probe::run(args),
        Commands::Ceiling(args) => ceiling::run(args),
    }
}
