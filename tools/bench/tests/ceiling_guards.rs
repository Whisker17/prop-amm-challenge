//! Integration tests for `bench ceiling`'s WHI-1247 step 8 guards. `prop-amm-bench` has no
//! `[lib]` target (only `[[bin]] name = "bench"`), so these drive the real compiled binary
//! as a subprocess (`CARGO_BIN_EXE_bench`, the standard cargo hook for a package's own
//! `[[bin]]`) rather than calling `ceiling.rs`'s functions directly.
//!
//! All three guards are refusals that must happen before any real work (compiling a
//! strategy, running a simulation) — so every test here asserts a failing exit status and a
//! stderr message naming the guard, not a report file.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Anchored at `CARGO_MANIFEST_DIR` (this crate's `tools/bench`), not the test binary's own
/// cwd — same pattern `config.rs`/`compile.rs`/`fuzz.rs`'s tests already use, since cargo
/// runs a package's tests with cwd set to the package root, not the workspace root, and
/// this lane's own relative defaults (`strategies/...`, `config/bench.toml`) are only valid
/// from the workspace root.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bench_cmd() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_bench"));
    cmd.current_dir(repo_root());
    cmd
}

/// WHI-1247 step 9: `ceilings/` holds provenance docs (`README.md`,
/// `C-orbic-oracle/NOTES.md`) but never a `lib.rs` — nothing under it is a submission, and a
/// stray `lib.rs` would invite exactly that confusion. Checked as a repo invariant, not just
/// a one-time review note, since a future PR could add one without realizing why that's
/// wrong.
#[test]
fn ceilings_directory_never_contains_a_lib_rs() {
    let ceilings_dir = repo_root().join("ceilings");
    assert!(
        ceilings_dir.is_dir(),
        "expected {} to exist (WHI-1247 step 9)",
        ceilings_dir.display()
    );

    let mut stack = vec![ceilings_dir];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| {
            panic!("failed to read dir {}: {e}", dir.display());
        }) {
            let entry = entry.expect("failed to read dir entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().and_then(|n| n.to_str()) == Some("lib.rs") {
                panic!(
                    "found a lib.rs under ceilings/ at {} — nothing in this out-of-\
                     competition lane is submittable (WHI-1247 step 9)",
                    path.display()
                );
            }
        }
    }
}

/// WHI-1247 step 7 guard (a): a `--reference` that resolves to a strategy slug outside
/// `ALLOWED_REFERENCE_SLUGS` (`000-normalizer`, `001-cpmm-fee`) is refused before it is ever
/// compiled — proven here against a real, committed, non-allowlisted strategy directory
/// rather than a synthetic path, so this test breaks if that strategy is ever renamed away
/// out from under it.
#[test]
fn reference_allowlist_rejects_a_non_allowlisted_strategy() {
    let non_allowlisted = repo_root().join("strategies/003-piecewise-linear");
    assert!(
        non_allowlisted.is_dir(),
        "test fixture assumption broken: {} no longer exists",
        non_allowlisted.display()
    );

    let output = bench_cmd()
        .args([
            "ceiling",
            "--reference",
            "strategies/003-piecewise-linear",
            "--concentration",
            "1.0",
            "--spread-bps",
            "10",
            "--no-report",
        ])
        .output()
        .expect("failed to run bench ceiling");

    assert!(
        !output.status.success(),
        "expected `bench ceiling --reference strategies/003-piecewise-linear` to fail, got \
         success. stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("allowlist"),
        "expected the allowlist guard's message in stderr, got:\n{stderr}"
    );
}

/// WHI-1247 step 7 guard (b): `--segment test` is refused unconditionally, even alongside
/// `--i-am-spending-the-test-segment` — the flag that would normally unlock a single-use
/// segment for every other subcommand. Nothing in this out-of-competition lane has a
/// ranking claim for that flag to protect.
#[test]
fn segment_test_is_refused_even_with_the_spend_flag() {
    let output = bench_cmd()
        .args([
            "ceiling",
            "--segment",
            "test",
            "--i-am-spending-the-test-segment",
        ])
        .output()
        .expect("failed to run bench ceiling");

    assert!(
        !output.status.success(),
        "expected `bench ceiling --segment test --i-am-spending-the-test-segment` to fail, \
         got success. stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("never spends the `test` segment"),
        "expected the test-segment guard's message in stderr, got:\n{stderr}"
    );
}
