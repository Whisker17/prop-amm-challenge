pub mod anchor;
pub mod ceiling;
pub mod compare;
pub mod estimator_probe;
pub mod fit;
pub mod fuzz;
pub mod grid;
pub mod l1;
pub mod parity;

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Segment;

/// Resolves a `--strategy <dir>` argument to `(slug, lib.rs path)` — shared by every
/// subcommand that takes a strategy directory (`parity`, `fit`, `fuzz`). Extracted once a
/// third call site repeated this verbatim; `docs/DEFERRED_ISSUES.md`'s `telemetry.rs` entry
/// sets the same bar this repo already uses: "two duplicates is a pattern worth naming,
/// three is worth extracting."
pub fn resolve_strategy_lib_path(strategy: &str) -> anyhow::Result<(String, PathBuf)> {
    let strategy_dir = Path::new(strategy);
    let slug = strategy_dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("`--strategy` must be a directory path, got `{strategy}`"))?
        .to_string();
    Ok((slug, strategy_dir.join("lib.rs")))
}

/// Uninformative path components [`slug_from_source_path`] climbs past, checked in this
/// order against whatever the path's current final component is — `lib.rs` first (the file
/// itself), then `src`, so `programs/<name>/src/lib.rs` climbs past both in the same call
/// (`lib.rs` first, landing on `src`, then `src` on the very next check).
const UNINFORMATIVE_PATH_COMPONENTS: [&str; 2] = ["lib.rs", "src"];

/// Derives a short identifier for a `.rs` source file path — the `grid`/`l1`/`compare`
/// equivalent of [`resolve_strategy_lib_path`]'s slug, for the subcommands that take the
/// source file directly (`--candidate`/`--reference`/`--file`) rather than a
/// `--strategy <dir>`. Those commands' own doc comments only ever promise "a path to the
/// candidate's `.rs` source file" (`grid.rs`/`compare.rs`), with no requirement that it live
/// under a strategy directory, so this only ever fails on a genuinely empty `path` — never
/// on a plain file with no informative parent (`lib.rs`, `src/lib.rs`) — since those were
/// valid `--candidate`/`--reference` values before this stage-naming existed and must stay
/// valid now.
///
/// Climbs past each of [`UNINFORMATIVE_PATH_COMPONENTS`] in turn — so both
/// `strategies/<slug>/lib.rs` and `programs/<name>/src/lib.rs` resolve to their meaningful
/// directory name (`<slug>`, `<name>`) instead of the uninformative `lib`/`src` a plain
/// `file_stem()`/immediate-parent lookup would give — but only ever climbs a level that
/// actually has a name of its own; a bare `lib.rs` or `src/lib.rs` with nothing more
/// informative above it just keeps its own last component (`lib`, `src`), with a trailing
/// `.rs` stripped so the result reads as an identifier rather than a filename.
pub fn slug_from_source_path(path: &str) -> anyhow::Result<String> {
    let mut p = Path::new(path);
    for component in UNINFORMATIVE_PATH_COMPONENTS {
        if p.file_name().and_then(|n| n.to_str()) == Some(component) {
            if let Some(parent) = p.parent().filter(|par| par.file_name().is_some()) {
                p = parent;
            }
        }
    }
    let name = p
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("could not derive a slug from source path `{path}`"))?;
    Ok(name.strip_suffix(".rs").unwrap_or(name).to_string())
}

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

    #[test]
    fn resolve_strategy_lib_path_derives_slug_and_lib_rs_path() {
        let (slug, file) = resolve_strategy_lib_path("strategies/001-cpmm-fee").unwrap();
        assert_eq!(slug, "001-cpmm-fee");
        assert_eq!(file, Path::new("strategies/001-cpmm-fee/lib.rs"));
    }

    #[test]
    fn resolve_strategy_lib_path_rejects_a_path_with_no_final_component() {
        assert!(resolve_strategy_lib_path("").is_err());
    }

    #[test]
    fn slug_from_source_path_strips_lib_rs_under_a_strategy_dir() {
        assert_eq!(
            slug_from_source_path("strategies/001-cpmm-fee/lib.rs").unwrap(),
            "001-cpmm-fee"
        );
        assert_eq!(
            slug_from_source_path("strategies/000-normalizer/lib.rs").unwrap(),
            "000-normalizer"
        );
    }

    #[test]
    fn slug_from_source_path_strips_src_lib_rs_under_a_program_dir() {
        assert_eq!(
            slug_from_source_path("programs/starter/src/lib.rs").unwrap(),
            "starter"
        );
    }

    #[test]
    fn slug_from_source_path_gives_distinct_slugs_for_distinct_strategies() {
        // The mechanism that actually prevents WHI-1215's collision: two different
        // candidates must never derive the same stage.
        let a = slug_from_source_path("strategies/001-cpmm-fee/lib.rs").unwrap();
        let b = slug_from_source_path("strategies/002-other/lib.rs").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn slug_from_source_path_gives_distinct_slugs_for_l1s_own_two_default_files() {
        // The two files `l1`'s own default `--file` list ships with (WHI-1195) — not that
        // they need distinct slugs (only the primary one is ever used for `l1`'s stage),
        // but proving the helper doesn't collapse the repo's two real, committed default
        // paths onto the same thing.
        let starter = slug_from_source_path("programs/starter/src/lib.rs").unwrap();
        let normalizer = slug_from_source_path("strategies/000-normalizer/lib.rs").unwrap();
        assert_ne!(starter, normalizer);
    }

    #[test]
    fn slug_from_source_path_keeps_a_flat_file_stripped_of_its_rs_extension() {
        // `--candidate`/`--reference`/`--file` only ever promise "a path to a `.rs` source
        // file" (`grid.rs`/`compare.rs`/`l1.rs`'s own doc comments) — nothing requires it
        // live under a strategy directory, e.g. `my_amm.rs` at the repo root (AGENTS.md's
        // own example submission filename).
        assert_eq!(slug_from_source_path("my_amm.rs").unwrap(), "my_amm");
    }

    #[test]
    fn slug_from_source_path_never_hard_errors_on_a_bare_lib_rs_or_src_lib_rs() {
        // These were valid `--candidate`/`--reference` values before this stage-naming
        // existed (nothing ever required a wrapping strategy directory) and must stay
        // valid — even though neither has a parent informative enough to climb to.
        assert_eq!(slug_from_source_path("lib.rs").unwrap(), "lib");
        assert_eq!(slug_from_source_path("src/lib.rs").unwrap(), "src");
    }

    #[test]
    fn slug_from_source_path_rejects_an_empty_path() {
        assert!(slug_from_source_path("").is_err());
    }
}
