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

/// Runs `bench` with `args`, asserts it refused (non-zero exit), and asserts `needle`
/// appears in stderr — the build-args/`.output()`/assert-refusal/assert-stderr shape every
/// guard test below needs. Round-3 review, standards finding #5: three tests used to repeat
/// this block verbatim; collapsing it here means a future guard test is one call, not
/// another ~20-line copy.
fn expect_refusal(args: &[&str], needle: &str) {
    let output = bench_cmd()
        .args(args)
        .output()
        .expect("failed to run bench ceiling");

    assert!(
        !output.status.success(),
        "expected `bench {}` to fail, got success. stdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(needle),
        "expected `{needle}` in stderr for `bench {}`, got:\n{stderr}",
        args.join(" ")
    );
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

    expect_refusal(
        &[
            "ceiling",
            "--reference",
            "strategies/003-piecewise-linear",
            "--concentration",
            "1.0",
            "--spread-bps",
            "10",
            "--no-report",
        ],
        "allowlist",
    );
}

/// WHI-1247 step 7 guard (a), the canonicalize half specifically (round-1 review of this
/// issue): a `--reference` whose final path component *matches* an allowlisted slug but
/// which does not actually resolve to `strategies/<slug>/lib.rs` must still be refused.
/// `reference_allowlist_rejects_a_non_allowlisted_strategy` above only proves the slug
/// check (a mismatched final component); this proves the second half — a directory that
/// shares the string `001-cpmm-fee` but lives somewhere else entirely, with its own,
/// different `lib.rs` — is still caught, not silently treated as the real 0-line. Round-2
/// review of this issue flagged that this half of guard (a) had no test at all.
#[test]
fn reference_allowlist_rejects_a_same_named_directory_outside_strategies() {
    let tmp = tempfile::tempdir().expect("failed to create tempdir");
    let decoy_dir = tmp.path().join("001-cpmm-fee");
    std::fs::create_dir(&decoy_dir).expect("failed to create decoy dir");
    std::fs::write(
        decoy_dir.join("lib.rs"),
        "// decoy lib.rs — not the real strategies/001-cpmm-fee/lib.rs\n",
    )
    .expect("failed to write decoy lib.rs");

    expect_refusal(
        &[
            "ceiling",
            "--reference",
            decoy_dir.to_str().expect("tempdir path must be UTF-8"),
            "--concentration",
            "1.0",
            "--spread-bps",
            "10",
            "--no-report",
        ],
        "is not the allowlisted",
    );
}

/// WHI-1247 step 7 guard (b): `--segment test` is refused unconditionally, even alongside
/// `--i-am-spending-the-test-segment` — the flag that would normally unlock a single-use
/// segment for every other subcommand. Nothing in this out-of-competition lane has a
/// ranking claim for that flag to protect.
#[test]
fn segment_test_is_refused_even_with_the_spend_flag() {
    expect_refusal(
        &[
            "ceiling",
            "--segment",
            "test",
            "--i-am-spending-the-test-segment",
        ],
        "never spends the `test` segment",
    );
}

/// WHI-1248 `validate_cursor_and_lag`, exercised black-box through the real compiled binary
/// (not just the in-process unit tests in `ceiling.rs`'s own `#[cfg(test)]` module): `--lag`
/// alongside `--cursor trade-triggered` (the default) is refused before any compile/simulate
/// work happens — `validate_cursor_and_lag` is `run()`'s very first line.
#[test]
fn cursor_trade_triggered_rejects_a_lag_flag() {
    expect_refusal(
        &[
            "ceiling",
            "--cursor",
            "trade-triggered",
            "--lag",
            "1",
            "--concentration",
            "1.0",
            "--spread-bps",
            "10",
            "--no-report",
        ],
        "only meaningful alongside",
    );
}

/// WHI-1248: `--cursor fingerprint` with no `--lag` is refused — the fingerprint rung has no
/// default lag, unlike trade-triggered's implicit lag-0-equivalent.
#[test]
fn cursor_fingerprint_requires_a_lag_flag() {
    expect_refusal(
        &[
            "ceiling",
            "--cursor",
            "fingerprint",
            "--concentration",
            "1.0",
            "--spread-bps",
            "10",
            "--no-report",
        ],
        "requires --lag",
    );
}

/// WHI-1248: `L in {5, 25}` is explicitly out of scope for this issue (deferred to a
/// follow-up unless the L=1-vs-trade-triggered gap is surprising) — `--lag 5` must be
/// refused by name, not silently accepted or misinterpreted as `--lag 1`.
#[test]
fn cursor_fingerprint_rejects_an_out_of_scope_lag_value() {
    expect_refusal(
        &[
            "ceiling",
            "--cursor",
            "fingerprint",
            "--lag",
            "5",
            "--concentration",
            "1.0",
            "--spread-bps",
            "10",
            "--no-report",
        ],
        "out of scope for WHI-1248",
    );
}
