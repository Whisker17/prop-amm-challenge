mod cli;
mod commands;
mod compile;
mod config;
mod fast_compile;
mod params;
mod report;
mod search;
mod stats;

fn main() -> anyhow::Result<()> {
    cli::run()
}
