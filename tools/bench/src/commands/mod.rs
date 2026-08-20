pub mod anchor;
pub mod compare;
pub mod fit;
pub mod grid;
pub mod l1;
pub mod parity;

use std::process::Command;

use crate::config::Segment;

/// Printed once when a resolved segment isn't a decision input (docs/DESIGN.md §2.2) —
/// shared so every segment-taking subcommand says the same thing instead of repeating it.
pub fn note_if_not_decision_input(name: &str, segment: &Segment) {
    if !segment.decision_input {
        println!(
            "Note: segment `{name}` is not a decision input (docs/DESIGN.md §2.2) — reporting only."
        );
    }
}

/// The upstream CLI only ever prints edge at 2 decimal places (`crates/cli/src/output.rs`),
/// so "agreement" at this precision means bench's own full-precision number rounds to the
/// same string — the maximum the existing tooling can prove without editing upstream-owned
/// code. Shared by `anchor` and `parity`, the two commands that cross-check against a real,
/// separately-invoked `prop-amm` process.
pub fn edges_agree(a: f64, b: f64) -> bool {
    format!("{a:.2}") == format!("{b:.2}")
}

pub struct CliEdgeReport {
    pub avg: f64,
    pub total: f64,
}

/// Shells out to `cargo run --release -p prop-amm -- run <file> ...` and parses its
/// `Avg edge:`/`Total edge:` stdout lines.
pub fn run_prop_amm(
    file: &str,
    simulations: u32,
    steps: u32,
    seed_start: u64,
    seed_stride: u64,
) -> anyhow::Result<CliEdgeReport> {
    let output = Command::new("cargo")
        .args([
            "run",
            "--release",
            "-p",
            "prop-amm",
            "--",
            "run",
            file,
            "--simulations",
            &simulations.to_string(),
            "--steps",
            &steps.to_string(),
            "--seed-start",
            &seed_start.to_string(),
            "--seed-stride",
            &seed_stride.to_string(),
        ])
        .output()
        .map_err(|e| anyhow::anyhow!("failed to run `prop-amm run {file}`: {e}"))?;

    if !output.status.success() {
        anyhow::bail!(
            "`prop-amm run {file}` failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut avg = None;
    let mut total = None;
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("Avg edge:") {
            avg = Some(parse_edge(value)?);
        } else if let Some(value) = trimmed.strip_prefix("Total edge:") {
            total = Some(parse_edge(value)?);
        }
    }

    match (avg, total) {
        (Some(avg), Some(total)) => Ok(CliEdgeReport { avg, total }),
        _ => anyhow::bail!(
            "`prop-amm run {file}` did not print both `Avg edge:`/`Total edge:` lines:\n{stdout}"
        ),
    }
}

fn parse_edge(value: &str) -> anyhow::Result<f64> {
    value
        .trim()
        .parse::<f64>()
        .map_err(|e| anyhow::anyhow!("failed to parse edge `{value}`: {e}"))
}

/// Shells out to `cargo run --release -p prop-amm -- validate <file>`, requiring success —
/// the "reproduced through `prop-amm validate`" half of the parity gate (docs/DESIGN.md
/// §2.6).
pub fn run_prop_amm_validate(file: &str) -> anyhow::Result<()> {
    let output = Command::new("cargo")
        .args(["run", "--release", "-p", "prop-amm", "--", "validate", file])
        .output()
        .map_err(|e| anyhow::anyhow!("failed to run `prop-amm validate {file}`: {e}"))?;

    if !output.status.success() {
        anyhow::bail!(
            "`prop-amm validate {file}` failed:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_agree_compares_at_cli_precision() {
        assert!(edges_agree(210.501, 210.499));
        assert!(!edges_agree(210.50, 210.51));
    }
}
