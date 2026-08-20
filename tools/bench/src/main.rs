mod cli;
mod commands;
mod compile;
mod config;
mod fast_compile;
mod grid;
mod params;
mod regime;
mod report;
mod search;
mod stats;
mod telemetry;

fn main() -> anyhow::Result<()> {
    cli::run()
}
