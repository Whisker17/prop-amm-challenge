use clap::{Parser, Subcommand};

use crate::commands::{anchor, compare};

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
}

pub fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Compare(args) => compare::run(args),
        Commands::Anchor(args) => anchor::run(args),
    }
}
