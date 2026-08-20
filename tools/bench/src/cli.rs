use clap::{Parser, Subcommand};

use crate::commands::{anchor, compare, grid, l1};

#[derive(Parser)]
#[command(name = "bench", about = "Prop AMM Challenge measurement layer")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

// Adding a new subcommand (`fit`, `grid`, ...) means adding a variant here plus a new
// `commands/<name>.rs` — no existing subcommand's file needs to change.
#[derive(Subcommand)]
enum Commands {
    /// Paired-by-seed comparison of a candidate against a reference.
    Compare(compare::CompareArgs),
    /// Cross-check bench's own numbers against a real `prop-amm run`.
    Anchor(anchor::AnchorArgs),
    /// The 27-cell fragility matrix over grid mode's own regime corners.
    Grid(grid::GridArgs),
    /// L1 observability: flow share and edge per unit volume for one candidate.
    L1(l1::L1Args),
}

pub fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Compare(args) => compare::run(args),
        Commands::Anchor(args) => anchor::run(args),
        Commands::Grid(args) => grid::run(args),
        Commands::L1(args) => l1::run(args),
    }
}
