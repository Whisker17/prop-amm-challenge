//! Where a result set actually came from.
//!
//! A benchmark run takes tens of minutes. If the repository moves during it —
//! someone commits, or edits a file — then asking git at the end produces a
//! commit that did not build the running binary. That is not a small
//! inaccuracy: the output claims a provenance it does not have, which is worse
//! than claiming none.
//!
//! So three captures are recorded and compared, never merged:
//!
//! 1. [`binary_commit`] / [`binary_dirty`] — baked in by `build.rs` at compile
//!    time. These identify the code that is actually running and cannot be
//!    changed by anything that happens afterwards.
//! 2. **run start** — taken before the first simulation, so a mismatch is
//!    reported in seconds rather than after the run.
//! 3. **run end** — taken just before the report is written.
//!
//! [`Provenance::matches`] requires all three commits to agree and all three
//! dirty flags to be false. Anything less and the result set is provisional:
//! usable for looking at, not for publishing.

use std::process::Command;

/// The commit the binary was compiled from. Compile-time constant.
pub const BINARY_COMMIT: &str = env!("RESEARCH_BUILD_COMMIT");

/// Whether the tree was dirty when the binary was compiled, as a string because
/// `env!` yields one. Use [`binary_dirty`].
pub const BINARY_DIRTY_RAW: &str = env!("RESEARCH_BUILD_DIRTY");

pub fn binary_commit() -> &'static str {
    BINARY_COMMIT
}

/// Anything other than a literal `false` counts as dirty, so a malformed or
/// missing value never reads as clean.
pub fn binary_dirty() -> bool {
    BINARY_DIRTY_RAW != "false"
}

/// Git state at one instant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitState {
    pub commit: String,
    pub dirty: bool,
}

impl GitState {
    /// Ask git now. Outside a checkout this reports `unknown` and dirty, so an
    /// unidentifiable build can never be mistaken for a clean one.
    pub fn capture() -> GitState {
        let commit = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|commit| !commit.is_empty())
            .unwrap_or_else(|| "unknown".to_string());
        let dirty = match Command::new("git").args(["status", "--porcelain"]).output() {
            Ok(output) if output.status.success() => !output.stdout.is_empty(),
            _ => true,
        };
        GitState { commit, dirty }
    }

    pub fn short(&self) -> String {
        self.commit.chars().take(12).collect()
    }
}

/// The three captures, plus whether they agree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub binary_commit: String,
    pub binary_dirty: bool,
    pub run_start: GitState,
    pub run_end: GitState,
}

impl Provenance {
    /// Start a run: bake in the compile-time values and capture the start state.
    ///
    /// `run_end` is initialised to the start state and replaced by
    /// [`Self::finish`]; a `Provenance` that was never finished therefore
    /// reports the start state twice rather than a hole.
    pub fn begin() -> Provenance {
        let run_start = GitState::capture();
        Provenance {
            binary_commit: BINARY_COMMIT.to_string(),
            binary_dirty: binary_dirty(),
            run_end: run_start.clone(),
            run_start,
        }
    }

    /// Capture the end state, just before writing the report.
    pub fn finish(&mut self) {
        self.run_end = GitState::capture();
    }

    /// The commit a result set may claim as its origin: the one that built the
    /// binary. Never the repository's HEAD at report time.
    pub fn benchmark_commit(&self) -> &str {
        &self.binary_commit
    }

    /// All three commits agree and none of the three states is dirty.
    pub fn matches(&self) -> bool {
        self.mismatch_reasons().is_empty()
    }

    /// Reasons known **at run start**, before the end state exists.
    ///
    /// Separate from [`Self::mismatch_reasons`] so the fail-fast message cannot
    /// claim the tree was dirty "when the report was written" before any report
    /// has been written — `begin` seeds `run_end` from `run_start`, and saying
    /// so would be an invented observation.
    pub fn start_mismatch_reasons(&self) -> Vec<String> {
        let mut reasons = Vec::new();
        if self.binary_commit == "unknown" {
            reasons.push("the binary was built outside a git checkout".to_string());
        }
        if self.binary_dirty {
            reasons.push(format!(
                "the working tree was dirty when the binary was built ({})",
                short(&self.binary_commit)
            ));
        }
        if self.run_start.dirty {
            reasons.push("the working tree is dirty now, at run start".to_string());
        }
        if self.binary_commit != self.run_start.commit {
            reasons.push(format!(
                "the binary was built from {} but HEAD is at {} \
                 — the running code is not the code at HEAD",
                short(&self.binary_commit),
                self.run_start.short()
            ));
        }
        reasons
    }

    /// Every reason the provenance is not clean, in a form fit for an error
    /// message. Empty when [`Self::matches`].
    pub fn mismatch_reasons(&self) -> Vec<String> {
        let mut reasons = Vec::new();
        if self.binary_commit == "unknown" {
            reasons.push("the binary was built outside a git checkout".to_string());
        }
        if self.binary_dirty {
            reasons.push(format!(
                "the working tree was dirty when the binary was built ({})",
                short(&self.binary_commit)
            ));
        }
        if self.run_start.dirty {
            reasons.push("the working tree was dirty when the run started".to_string());
        }
        if self.run_end.dirty {
            reasons.push("the working tree was dirty when the report was written".to_string());
        }
        if self.binary_commit != self.run_start.commit {
            reasons.push(format!(
                "the binary was built from {} but the run started at {} \
                 — the running code is not the code at HEAD",
                short(&self.binary_commit),
                self.run_start.short()
            ));
        }
        if self.run_start.commit != self.run_end.commit {
            reasons.push(format!(
                "HEAD moved during the run, from {} to {} \
                 — something was committed while the benchmark was in flight",
                self.run_start.short(),
                self.run_end.short()
            ));
        }
        reasons
    }

    /// One line for stdout at run start, before any work is done.
    pub fn start_line(&self) -> String {
        format!(
            "provenance: binary {}{} | run start {}{}",
            short(&self.binary_commit),
            if self.binary_dirty { " (dirty)" } else { "" },
            self.run_start.short(),
            if self.run_start.dirty { " (dirty)" } else { "" },
        )
    }
}

fn short(commit: &str) -> String {
    commit.chars().take(12).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(commit: &str, dirty: bool) -> GitState {
        GitState {
            commit: commit.to_string(),
            dirty,
        }
    }

    fn clean(commit: &str) -> Provenance {
        Provenance {
            binary_commit: commit.to_string(),
            binary_dirty: false,
            run_start: state(commit, false),
            run_end: state(commit, false),
        }
    }

    #[test]
    fn three_matching_clean_captures_are_publishable() {
        let provenance = clean("a".repeat(40).as_str());
        assert!(provenance.matches());
        assert!(provenance.mismatch_reasons().is_empty());
    }

    /// The exact failure that produced a mis-stamped result set: the binary was
    /// built at one commit, then HEAD moved before the report was written.
    #[test]
    fn a_commit_during_the_run_is_detected() {
        let mut provenance = clean(&"a".repeat(40));
        provenance.run_end = state(&"b".repeat(40), false);
        assert!(!provenance.matches());
        let reasons = provenance.mismatch_reasons().join("; ");
        assert!(reasons.contains("HEAD moved during the run"), "{reasons}");
    }

    #[test]
    fn a_binary_older_than_head_is_detected_at_start() {
        let mut provenance = clean(&"a".repeat(40));
        provenance.run_start = state(&"b".repeat(40), false);
        provenance.run_end = state(&"b".repeat(40), false);
        let reasons = provenance.mismatch_reasons().join("; ");
        assert!(
            reasons.contains("the running code is not the code at HEAD"),
            "{reasons}"
        );
    }

    #[test]
    fn every_dirty_flag_is_reported_separately() {
        for (label, mutate) in [
            (
                "built",
                Box::new(|p: &mut Provenance| p.binary_dirty = true)
                    as Box<dyn Fn(&mut Provenance)>,
            ),
            (
                "started",
                Box::new(|p: &mut Provenance| p.run_start.dirty = true),
            ),
            (
                "written",
                Box::new(|p: &mut Provenance| p.run_end.dirty = true),
            ),
        ] {
            let mut provenance = clean(&"a".repeat(40));
            mutate(&mut provenance);
            assert!(!provenance.matches(), "{label}: should not match");
            assert_eq!(
                provenance.mismatch_reasons().len(),
                1,
                "{label}: exactly one reason expected"
            );
        }
    }

    #[test]
    fn an_unknown_binary_commit_is_never_clean() {
        let provenance = clean("unknown");
        assert!(!provenance.matches());
    }

    /// A malformed or absent build-time flag must read as dirty, never clean.
    #[test]
    fn a_malformed_dirty_flag_reads_as_dirty() {
        // `binary_dirty` is `raw != "false"`, so anything unexpected is dirty.
        assert!(BINARY_DIRTY_RAW == "true" || BINARY_DIRTY_RAW == "false");
        assert_eq!(binary_dirty(), BINARY_DIRTY_RAW != "false");
    }

    /// The embedded commit is a compile-time constant, so it is a `&'static str`
    /// and nothing at runtime can change it. The shape check catches a build
    /// script that silently produced garbage.
    #[test]
    fn the_binary_commit_is_a_compile_time_constant() {
        let commit: &'static str = BINARY_COMMIT;
        assert!(
            commit == "unknown"
                || (commit.len() == 40 && commit.chars().all(|c| c.is_ascii_hexdigit())),
            "unexpected embedded commit {commit:?}"
        );
        // Two reads of a const are the same read; this documents intent.
        assert_eq!(commit, binary_commit());
    }

    /// The fail-fast message must not mention the report, which has not been
    /// written yet.
    #[test]
    fn start_reasons_never_mention_the_report() {
        let mut provenance = clean(&"a".repeat(40));
        provenance.run_start.dirty = true;
        provenance.run_end.dirty = true;
        let start = provenance.start_mismatch_reasons().join("; ");
        assert!(start.contains("at run start"), "{start}");
        assert!(
            !start.contains("report was written"),
            "the start message claims something about the report: {start}"
        );
        // The full set, used at report time, does mention it.
        let full = provenance.mismatch_reasons().join("; ");
        assert!(full.contains("report was written"), "{full}");
    }

    #[test]
    fn benchmark_commit_is_the_binary_not_the_head() {
        let mut provenance = clean(&"a".repeat(40));
        provenance.run_end = state(&"b".repeat(40), false);
        assert_eq!(provenance.benchmark_commit(), "a".repeat(40));
    }
}
