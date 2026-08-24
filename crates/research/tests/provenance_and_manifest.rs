//! Provenance and manifest checks that need a real git repository or the real
//! system `shasum`, and therefore cannot live in a unit test.
//!
//! The provenance tests work by pointing `GIT_DIR` / `GIT_WORK_TREE` at a
//! scratch repository. `GitState::capture` shells out to git, so it follows
//! those variables, while `BINARY_COMMIT` is baked in by `build.rs` and cannot.
//! That asymmetry is exactly the property under test, and it is what makes the
//! test possible without touching this repository's own history.
//!
//! Environment variables are process-global, so the git-manipulating tests are
//! serialised behind a mutex rather than left to race.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use prop_amm_research::manifest::{self, MANIFEST_NAME};
use prop_amm_research::provenance::{self, GitState, Provenance};

fn git_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let mut path = std::env::temp_dir();
        path.push(format!("prop-amm-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch dir");
        Scratch(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        // Isolate from the caller's identity and hooks.
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .unwrap_or_else(|error| panic!("git {args:?}: {error}"));
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// A scratch repository with one commit. Returns its HEAD.
fn init_repo(dir: &Path) -> String {
    run_git(dir, &["init", "--quiet", "--initial-branch=main", "."]);
    std::fs::write(dir.join("file.txt"), b"one").unwrap();
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "--quiet", "-m", "one"]);
    run_git(dir, &["rev-parse", "HEAD"])
}

/// Run `body` with git pointed at `dir`, restoring the environment afterwards.
fn with_git_dir<T>(dir: &Path, body: impl FnOnce() -> T) -> T {
    let guard = git_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let previous_dir = std::env::var("GIT_DIR").ok();
    let previous_tree = std::env::var("GIT_WORK_TREE").ok();
    std::env::set_var("GIT_DIR", dir.join(".git"));
    std::env::set_var("GIT_WORK_TREE", dir);

    let result = body();

    match previous_dir {
        Some(value) => std::env::set_var("GIT_DIR", value),
        None => std::env::remove_var("GIT_DIR"),
    }
    match previous_tree {
        Some(value) => std::env::set_var("GIT_WORK_TREE", value),
        None => std::env::remove_var("GIT_WORK_TREE"),
    }
    drop(guard);
    result
}

/// **The property the whole design rests on.** `BINARY_COMMIT` is written into
/// the binary by `build.rs`, so moving HEAD afterwards cannot change it — while
/// a runtime capture follows HEAD immediately. If these two ever move together,
/// the build script has stopped embedding and provenance is being read at run
/// time again, which is the bug this module exists to prevent.
#[test]
fn the_embedded_commit_survives_a_head_change_that_the_runtime_capture_follows() {
    let scratch = Scratch::new("provenance-head");
    let first = init_repo(scratch.path());

    let embedded_before = provenance::binary_commit().to_string();
    let captured_before = with_git_dir(scratch.path(), GitState::capture);
    assert_eq!(
        captured_before.commit, first,
        "the runtime capture should follow GIT_DIR"
    );

    // Move HEAD.
    std::fs::write(scratch.path().join("file.txt"), b"two").unwrap();
    run_git(scratch.path(), &["add", "."]);
    run_git(scratch.path(), &["commit", "--quiet", "-m", "two"]);
    let second = run_git(scratch.path(), &["rev-parse", "HEAD"]);
    assert_ne!(first, second, "the scratch repo did not advance");

    let embedded_after = provenance::binary_commit().to_string();
    let captured_after = with_git_dir(scratch.path(), GitState::capture);

    assert_eq!(
        embedded_before, embedded_after,
        "the embedded commit changed when HEAD moved — build.rs is no longer \
         embedding at compile time"
    );
    assert_eq!(
        captured_after.commit, second,
        "the runtime capture did not follow the new HEAD"
    );
    assert_ne!(
        captured_before.commit, captured_after.commit,
        "the two captures are indistinguishable, so this test proves nothing"
    );
}

/// A commit landing between the start and end captures must be detected. This
/// is the failure that mis-stamped a 40-minute run.
#[test]
fn a_commit_during_a_run_is_detected_by_the_two_captures() {
    let scratch = Scratch::new("provenance-midrun");
    let first = init_repo(scratch.path());

    let run_start = with_git_dir(scratch.path(), GitState::capture);
    assert_eq!(run_start.commit, first);
    assert!(!run_start.dirty, "a fresh commit leaves a clean tree");

    // ... the run proceeds, and somebody commits ...
    std::fs::write(scratch.path().join("file.txt"), b"changed mid-run").unwrap();
    run_git(scratch.path(), &["add", "."]);
    run_git(scratch.path(), &["commit", "--quiet", "-m", "mid-run"]);

    let run_end = with_git_dir(scratch.path(), GitState::capture);
    assert_ne!(run_start.commit, run_end.commit);

    let provenance = Provenance {
        binary_commit: first.clone(),
        binary_dirty: false,
        run_start,
        run_end,
    };
    assert!(!provenance.matches(), "the mid-run commit was not detected");
    let reasons = provenance.mismatch_reasons().join("; ");
    assert!(reasons.contains("HEAD moved during the run"), "{reasons}");
    // The claimed commit must still be the binary's, not the new HEAD.
    assert_eq!(provenance.benchmark_commit(), first);
}

/// An uncommitted edit must be seen as dirty by the runtime capture.
#[test]
fn an_uncommitted_edit_is_seen_as_dirty() {
    let scratch = Scratch::new("provenance-dirty");
    init_repo(scratch.path());
    let clean = with_git_dir(scratch.path(), GitState::capture);
    assert!(!clean.dirty);

    std::fs::write(scratch.path().join("file.txt"), b"edited but not committed").unwrap();
    let dirty = with_git_dir(scratch.path(), GitState::capture);
    assert!(dirty.dirty, "an unstaged edit must read as dirty");
    assert_eq!(
        dirty.commit, clean.commit,
        "an unstaged edit does not move HEAD"
    );
}

/// The manifest must be byte-identical to what `shasum -a 256` produces, since
/// that is the tool a reader will verify it with.
#[test]
fn the_manifest_matches_the_system_shasum() {
    let scratch = Scratch::new("manifest-shasum");
    // Content chosen to span block boundaries and include non-ASCII bytes.
    std::fs::write(scratch.path().join("a.csv"), b"header\n1,2,3\n").unwrap();
    std::fs::write(scratch.path().join("b.json"), vec![b'x'; 1000]).unwrap();
    std::fs::write(scratch.path().join("c.md"), "标题\n内容\n".as_bytes()).unwrap();
    std::fs::write(scratch.path().join("d.empty"), b"").unwrap();

    let ours = manifest::render(scratch.path()).unwrap();

    // `shasum -a 256 ./a.csv ./b.json ...` in the same sorted order.
    let output = Command::new("shasum")
        .args(["-a", "256", "./a.csv", "./b.json", "./c.md", "./d.empty"])
        .current_dir(scratch.path())
        .output()
        .expect("shasum must be available");
    assert!(output.status.success(), "shasum failed");
    let theirs = String::from_utf8_lossy(&output.stdout).into_owned();

    assert_eq!(ours, theirs, "our manifest differs from shasum -a 256");

    // And the written file must verify with `shasum -c`.
    manifest::write(scratch.path()).unwrap();
    let check = Command::new("shasum")
        .args(["-a", "256", "-c", MANIFEST_NAME])
        .current_dir(scratch.path())
        .output()
        .expect("shasum -c");
    assert!(
        check.status.success(),
        "shasum -c rejected our manifest: {}{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );

    // Corrupting a file must make verification fail — otherwise the manifest
    // proves nothing.
    std::fs::write(scratch.path().join("a.csv"), b"header\n1,2,4\n").unwrap();
    let corrupted = Command::new("shasum")
        .args(["-a", "256", "-c", MANIFEST_NAME])
        .current_dir(scratch.path())
        .output()
        .expect("shasum -c");
    assert!(
        !corrupted.status.success(),
        "a corrupted file still passed verification"
    );
}

/// Sorting must not depend on the order the filesystem returns entries in.
#[test]
fn the_manifest_order_is_stable_regardless_of_creation_order() {
    let forward = Scratch::new("manifest-forward");
    let reverse = Scratch::new("manifest-reverse");
    let names = ["alpha.csv", "beta.json", "gamma.md", "delta.txt"];

    for name in names {
        std::fs::write(forward.path().join(name), name.as_bytes()).unwrap();
    }
    for name in names.iter().rev() {
        std::fs::write(reverse.path().join(name), name.as_bytes()).unwrap();
    }

    assert_eq!(
        manifest::render(forward.path()).unwrap(),
        manifest::render(reverse.path()).unwrap(),
        "the manifest depends on creation order"
    );
}
