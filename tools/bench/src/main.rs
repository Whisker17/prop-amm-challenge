mod cli;
mod commands;
mod compile;
mod config;
mod report;
mod stats;

fn main() -> anyhow::Result<()> {
    cli::run()
}
