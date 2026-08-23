mod cli;
mod commands;
mod compile;
mod config;
mod curve_checks;
mod estimator_probe;
mod fast_compile;
mod fuzz;
mod grid;
mod oracle;
mod params;
mod regime;
mod report;
mod search;
mod stats;
mod telemetry;

fn main() -> anyhow::Result<()> {
    cli::run()
}
