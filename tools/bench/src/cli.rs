use clap::{Parser, Subcommand};

use crate::commands::{anchor, compare, fit, parity};

#[derive(Parser)]
#[command(name = "bench", about = "Prop AMM Challenge measurement layer")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

// Adding a new subcommand (`grid`, ...) means adding a variant here plus a new
// `commands/<name>.rs` — no existing subcommand's file needs to change.
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
}

pub fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Compare(args) => compare::run(args),
        Commands::Anchor(args) => anchor::run(args),
        Commands::Fit(args) => fit::run(args),
        Commands::Parity(args) => parity::run(args),
    }
}
