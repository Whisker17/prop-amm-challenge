use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The header every report carries (docs/DESIGN.md §3.3): what was measured and where it
/// came from, so a number can never be cited without its provenance.
pub struct ReportMeta {
    pub stage: String,
    pub segment: String,
    pub n_sims: usize,
    pub n_steps: u32,
    pub execution_path: String,
}

/// One caller-contributed, pre-formatted block of the report. A new command (WHI-1194's
/// search curve, WHI-1195's grid matrix) is just another `ReportSection` appended by its own
/// file — this formatter never needs to change to accommodate it.
pub struct ReportSection {
    pub heading: String,
    pub body: String,
}

pub const DEFAULT_REPORT_DIR: &str = "results";

fn expected_path(dir: &Path, stage: &str) -> PathBuf {
    dir.join(format!("{}-{stage}.md", today()))
}

fn refuse_if_exists(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        anyhow::bail!(
            "{} already exists; results/ snapshots are committed evidence and are never \
             overwritten silently — move or remove it before re-running",
            path.display()
        );
    }
    Ok(())
}

/// Fails fast if `<dir>/<today>-<stage>.md` already exists. Call this **before** doing any
/// expensive measurement work (a `compare`/`anchor` run is minutes of compiling and
/// simulating) — `write_report`'s own check at the end would otherwise only discover the
/// conflict after all of that work was thrown away.
pub fn ensure_report_slot_free(dir: &Path, stage: &str) -> anyhow::Result<()> {
    refuse_if_exists(&expected_path(dir, stage))
}

/// Writes `<dir>/<date>-<stage>.md` and returns its path. Refuses to overwrite an existing
/// report — `results/` snapshots are committed evidence (docs/DESIGN.md §3.3), and a second
/// same-day run silently clobbering the first would destroy that evidence without anyone
/// noticing. Move or remove the existing file first if a re-run is really intended.
pub fn write_report(
    dir: &Path,
    meta: &ReportMeta,
    sections: &[ReportSection],
) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(dir)
        .map_err(|e| anyhow::anyhow!("failed to create report dir {}: {e}", dir.display()))?;
    let path = expected_path(dir, &meta.stage);
    refuse_if_exists(&path)?;
    let date = today();

    let mut out = String::new();
    out.push_str(&format!("# {} — {date}\n\n", meta.stage));
    out.push_str(&format!("- Commit: `{}`\n", commit_sha()));
    out.push_str(&format!("- Segment: `{}`\n", meta.segment));
    out.push_str(&format!("- Simulations: {}\n", meta.n_sims));
    out.push_str(&format!("- Steps: {}\n", meta.n_steps));
    out.push_str(&format!("- Execution path: {}\n", meta.execution_path));
    out.push('\n');

    for section in sections {
        out.push_str(&format!("## {}\n\n{}\n\n", section.heading, section.body));
    }

    fs::write(&path, out)
        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", path.display()))?;
    Ok(path)
}

fn commit_sha() -> String {
    let sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // `--untracked-files=no`: an untracked scratch file (a build artifact, a not-yet-added
    // report) shouldn't mark the *code* dirty — only uncommitted changes to tracked files
    // should, since that's what actually means "this measurement's code differs from HEAD".
    let dirty = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .ok()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    if dirty {
        format!("{sha}+dirty")
    } else {
        sha
    }
}

fn today() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant's `civil_from_days`: days-since-Unix-epoch -> (year, month, day). Used
/// instead of a date/time crate so one report-filename timestamp doesn't add a dependency.
/// https://howardhinnant.github.io/date_algorithms.html#civil_from_days
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_matches_known_epoch_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2026-08-20 is 20,685 days after the Unix epoch (verified independently via
        // `datetime.date(2026, 8, 20) - datetime.date(1970, 1, 1)`).
        assert_eq!(civil_from_days(20_685), (2026, 8, 20));
    }

    #[test]
    fn write_report_produces_expected_filename_and_header() {
        let tmp = tempfile::tempdir().unwrap();
        let meta = ReportMeta {
            stage: "unit-test-stage".to_string(),
            segment: "observation".to_string(),
            n_sims: 1000,
            n_steps: 10_000,
            execution_path: "native".to_string(),
        };
        let sections = [ReportSection {
            heading: "A section".to_string(),
            body: "some body text".to_string(),
        }];

        let path = write_report(tmp.path(), &meta, &sections).unwrap();

        let file_name = path.file_name().unwrap().to_str().unwrap();
        assert!(
            file_name.ends_with("-unit-test-stage.md"),
            "got {file_name}"
        );
        let date_part = &file_name[..10];
        assert_eq!(date_part.len(), 10);
        assert!(date_part.chars().nth(4) == Some('-') && date_part.chars().nth(7) == Some('-'));

        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("Segment: `observation`"));
        assert!(contents.contains("Simulations: 1000"));
        assert!(contents.contains("Steps: 10000"));
        assert!(contents.contains("Execution path: native"));
        assert!(contents.contains("## A section"));
        assert!(contents.contains("some body text"));
    }

    #[test]
    fn write_report_refuses_to_overwrite_an_existing_report() {
        let tmp = tempfile::tempdir().unwrap();
        let meta = ReportMeta {
            stage: "dup-stage".to_string(),
            segment: "observation".to_string(),
            n_sims: 1,
            n_steps: 1,
            execution_path: "native".to_string(),
        };
        let sections = [ReportSection {
            heading: "H".to_string(),
            body: "B".to_string(),
        }];

        write_report(tmp.path(), &meta, &sections).unwrap();
        let err = write_report(tmp.path(), &meta, &sections).unwrap_err();
        assert!(
            err.to_string().contains("already exists"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn ensure_report_slot_free_catches_a_conflict_before_any_expensive_work() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(ensure_report_slot_free(tmp.path(), "preflight-stage").is_ok());

        let meta = ReportMeta {
            stage: "preflight-stage".to_string(),
            segment: "observation".to_string(),
            n_sims: 1,
            n_steps: 1,
            execution_path: "native".to_string(),
        };
        write_report(tmp.path(), &meta, &[]).unwrap();

        let err = ensure_report_slot_free(tmp.path(), "preflight-stage").unwrap_err();
        assert!(
            err.to_string().contains("already exists"),
            "unexpected error: {err}"
        );
    }
}
