mod cli;
mod commands;
mod compile;
mod config;
mod grid;
mod regime;
mod report;
mod stats;
mod telemetry;

fn main() -> anyhow::Result<()> {
    cli::run()
}
