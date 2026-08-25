//! Embeds the git state **of the build** into the binary.
//!
//! Why this exists: `git rev-parse HEAD` at report-writing time records where
//! the repository was when the report was written, which is not where it was
//! when the binary was compiled. A long run that spans a commit therefore
//! stamps its output with a commit whose code never produced it — a provenance
//! claim that is not merely incomplete but false.
//!
//! The values here are baked in by `rustc` and cannot change afterwards. The
//! runtime captures in `provenance.rs` are compared against them.
//!
//! ## Cache invalidation
//!
//! Cargo caches build-script output, so this must declare what makes it stale:
//!
//! * `.git/HEAD` — moves on commit and on branch switch;
//! * the ref HEAD points at — moves on commit;
//! * `.git/index` — moves when anything is staged;
//! * `.git/packed-refs` — where a ref may live instead of a loose file.
//!
//! In a worktree `.git` is a file pointing elsewhere, so every path is resolved
//! through `git rev-parse --git-path` rather than assumed.
//!
//! One case is not detectable by watching files: an **unstaged** edit changes
//! the dirty state without touching any of the above. That is deliberate rather
//! than overlooked — forcing a rebuild on every source edit would be worse — and
//! it is caught at runtime instead: `publishable` requires the build-time and
//! both run-time dirty flags to agree and to be false, so a stale
//! `RESEARCH_BUILD_DIRTY=false` is contradicted by the run-time capture.

use std::path::PathBuf;
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// Absolute path for a file inside the git directory, or `None` outside a repo.
fn git_path(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(git(&["rev-parse", "--git-path", name])?);
    path.exists().then_some(path)
}

fn main() {
    // Re-run when the recorded state could have changed.
    for name in ["HEAD", "index", "packed-refs"] {
        if let Some(path) = git_path(name) {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    // The ref HEAD points at, when it is a loose file.
    if let Some(reference) = git(&["symbolic-ref", "--quiet", "HEAD"]) {
        if let Some(path) = git_path(&reference) {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    // An explicit escape hatch for forcing a re-stamp.
    println!("cargo:rerun-if-env-changed=RESEARCH_FORCE_PROVENANCE_REBUILD");

    let commit = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    // `--porcelain` is empty exactly when the tree is clean. Absence of git is
    // reported as dirty: unknown provenance must never read as clean.
    let dirty = match Command::new("git").args(["status", "--porcelain"]).output() {
        Ok(output) if output.status.success() => !output.stdout.is_empty(),
        _ => true,
    };

    println!("cargo:rustc-env=RESEARCH_BUILD_COMMIT={commit}");
    println!("cargo:rustc-env=RESEARCH_BUILD_DIRTY={dirty}");
}
