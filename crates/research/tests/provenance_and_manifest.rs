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

/// **A commit mismatch is not a dirty tree.**
///
/// The binary was built at A, the run happened at B, and every working tree was
/// clean. An earlier version rendered `!provenanceMatch` as "运行时工作区有未提交
/// 改动" / "工作区不干净", which is simply false here and sends a reader looking
/// for uncommitted edits that do not exist. It also emitted a single
/// `workingTreeDirty` JSON field carrying that same conflation.
#[test]
fn a_commit_mismatch_is_never_reported_as_a_dirty_tree() {
    use prop_amm_research::experiment::{BatchConfig, Competitor};
    use prop_amm_research::json::Json;
    use prop_amm_research::metrics::StrategySummary;
    use prop_amm_research::report::{self, RunMeta};
    use prop_amm_research::report_focus;
    use prop_amm_research::strategies::StrategySet;

    let binary = "a".repeat(40);
    let ran_at = "b".repeat(40);
    let provenance = Provenance {
        binary_commit: binary.clone(),
        binary_dirty: false,
        run_start: GitState {
            commit: ran_at.clone(),
            dirty: false,
        },
        run_end: GitState {
            commit: ran_at.clone(),
            dirty: false,
        },
    };

    assert!(
        !provenance.matches(),
        "a binary older than HEAD must not be treated as a match"
    );
    let reasons = provenance.mismatch_reasons();
    let joined = reasons.join("; ");
    assert!(
        joined.contains("the running code is not the code at HEAD"),
        "the reason must name the commit mismatch: {joined}"
    );
    assert!(
        !joined.to_lowercase().contains("dirty"),
        "no reason may mention dirtiness when every tree was clean: {joined}"
    );

    let batch = BatchConfig {
        simulations: 1,
        steps: 10,
        seed_start: 0,
        seed_stride: 1,
        competitor: Competitor::Normalizer,
        workers: 1,
    };
    let meta =
        RunMeta::from_batch_with_provenance(&batch, 1.0, StrategySet::Legacy, provenance.clone());
    let summaries: Vec<StrategySummary> = Vec::new();

    // ---- Markdown must not invent a dirty working tree ----
    let line = report::provenance_line_zh(&meta);
    for forbidden in ["运行时工作区有未提交改动", "工作区不干净", "工作区有未提交"]
    {
        assert!(
            !line.contains(forbidden),
            "the provenance line still claims a dirty tree: {forbidden}"
        );
    }
    assert!(
        line.contains("provenance 不一致"),
        "the line must say provenance disagrees: {line}"
    );
    assert!(
        line.contains("不可发布"),
        "the line must say the result is not publishable: {line}"
    );
    // And it must name the binary's commit, not HEAD.
    assert!(line.contains(&binary), "the binary commit must be shown");

    // ---- every document that carries provenance ----
    let documents = [
        (
            "REPORT-dodo-vs-flashbots",
            report_focus::dodo_vs_flashbots(&meta, &summaries, &[], &[]),
        ),
        (
            "REPORT-vs-baselines",
            report_focus::vs_baselines(&meta, &summaries, &[], &[], &[], None, &[]),
        ),
    ];
    for (name, markdown) in documents {
        for forbidden in ["运行时工作区有未提交改动", "工作区不干净"] {
            assert!(
                !markdown.contains(forbidden),
                "{name} still claims a dirty tree: {forbidden}"
            );
        }
        assert!(
            markdown.contains("provenance 不一致"),
            "{name} does not report the provenance failure"
        );
    }

    // ---- JSON must carry the six fields, and no ambiguous one ----
    for (name, body) in [
        (
            "dodo-vs-flashbots.json",
            report_focus::dodo_vs_flashbots_json(&meta, &summaries, &[]),
        ),
        (
            "versus-baselines.json",
            report_focus::vs_baselines_json(&meta, &summaries, &[], &[]),
        ),
    ] {
        let json = Json::parse(&body).unwrap_or_else(|_| panic!("{name} is not valid JSON"));
        assert!(
            json.get("workingTreeDirty").is_none(),
            "{name} still emits the ambiguous workingTreeDirty field"
        );
        for field in [
            "binaryCommit",
            "binaryWorkingTreeDirty",
            "runStartCommit",
            "runStartWorkingTreeDirty",
            "runEndCommit",
            "runEndWorkingTreeDirty",
            "provenanceMatch",
            "provenanceMismatchReasons",
            "publishable",
            "gates",
        ] {
            assert!(json.get(field).is_some(), "{name} is missing {field}");
        }
        assert_eq!(
            json.get("binaryCommit").unwrap().as_str(),
            Some(binary.as_str()),
            "{name}: benchmark provenance must name the binary's commit"
        );
        assert_eq!(
            json.get("runStartCommit").unwrap().as_str(),
            Some(ran_at.as_str()),
            "{name}"
        );
        assert_eq!(
            json.get("provenanceMatch").unwrap().as_bool(),
            Some(false),
            "{name}"
        );
        assert_eq!(
            json.get("publishable").unwrap().as_bool(),
            Some(false),
            "{name}: a provenance mismatch cannot be publishable"
        );
        // All three dirty flags are false, and are reported as such.
        for field in [
            "binaryWorkingTreeDirty",
            "runStartWorkingTreeDirty",
            "runEndWorkingTreeDirty",
        ] {
            assert_eq!(
                json.get(field).unwrap().as_bool(),
                Some(false),
                "{name}: {field} must be false, the trees were clean"
            );
        }
    }
}
