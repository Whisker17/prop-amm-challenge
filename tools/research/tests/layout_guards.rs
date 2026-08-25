//! Layout guards for the imported out-of-competition research crate (WHI-1274).
//!
//! These live here, not under `tools/bench`, so the verified measurement stack
//! is untouched. They encode the import's placement rules: the crate must not
//! land under upstream-owned `crates/`, its binary is `research` (not a
//! submission `lib.rs` that `prop-amm run` / `bench fit` could consume), and
//! committed snapshots declare `out_of_competition: true`.

use std::path::{Path, PathBuf};

/// Anchored at `CARGO_MANIFEST_DIR` (`tools/research`), not the test binary's
/// cwd — cargo runs a package's tests with cwd set to the package root.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn research_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// WHI-1274: the imported crate must not sit under upstream-owned `crates/`
/// (`docs/DESIGN.md` §3.2 / §11). A later PR that reintroduces
/// `crates/research/` fails this before the layout can be mistaken for
/// simulator code.
#[test]
fn crates_research_does_not_exist() {
    let forbidden = repo_root().join("crates/research");
    assert!(
        !forbidden.exists(),
        "crates/research/ must not exist; the research crate lives at \
         tools/research/ (WHI-1274). Found {}",
        forbidden.display()
    );
}

/// WHI-1274: the research binary is `research`, not a `prop-amm` / `bench`
/// stand-in. `src/lib.rs` is a library of ports, so this does **not** assert
/// that file is absent.
#[test]
fn research_binary_is_named_research() {
    let cargo_toml = std::fs::read_to_string(research_dir().join("Cargo.toml"))
        .expect("read tools/research/Cargo.toml");
    let bin_section = cargo_toml.split("[[bin]]").nth(1).unwrap_or_else(|| {
        panic!("tools/research/Cargo.toml must declare a [[bin]] target named research")
    });
    let name_line = bin_section
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("name"))
        .unwrap_or_else(|| panic!("[[bin]] section has no name = ... line:\n{bin_section}"));
    assert_eq!(
        name_line, "name = \"research\"",
        "research crate binary must be `research`, got `{name_line}`"
    );
}

/// WHI-1274: nothing under `tools/research/**` is a submission-shaped `lib.rs`
/// that could be passed to `prop-amm run` / `bench fit`. The crate's
/// `src/lib.rs` is allowed (it is a module tree of ports) but must not be a
/// `compute_swap` entrypoint.
#[test]
fn tools_research_has_no_submission_shaped_lib_rs() {
    let research = research_dir();
    let mut stack = vec![research.clone()];
    let mut seen_src_lib = false;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| {
            panic!("failed to read dir {}: {e}", dir.display());
        }) {
            let entry = entry.expect("failed to read dir entry");
            let path = entry.path();
            if path.is_dir() {
                // Workspace builds land in repo-root target/; a nested
                // CARGO_TARGET_DIR=tools/research/target must not trip the
                // "only src/lib.rs" assertion.
                if path.file_name().and_then(|n| n.to_str()) == Some("target") {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()) != Some("lib.rs") {
                continue;
            }
            let rel = path
                .strip_prefix(&research)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            assert_eq!(
                rel,
                "src/lib.rs",
                "unexpected lib.rs under tools/research/ at {} — a directory-root \
                 lib.rs is the submission shape `prop-amm run` / `bench fit` consume \
                 (WHI-1274)",
                path.display()
            );
            seen_src_lib = true;
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
                panic!("failed to read {}: {e}", path.display());
            });
            assert!(
                !text.contains("pinocchio"),
                "{} must not link pinocchio; it is not a submission",
                path.display()
            );
            assert!(
                !text.contains("entrypoint!"),
                "{} must not declare a BPF entrypoint",
                path.display()
            );
            assert!(
                !text.contains("pub fn compute_swap("),
                "{} is a library of ports, not a compute_swap entrypoint (WHI-1274)",
                path.display()
            );
        }
    }
    assert!(
        seen_src_lib,
        "expected tools/research/src/lib.rs to exist as the ports library"
    );
}

/// WHI-1274 step 5: committed snapshots declare they are out of competition,
/// matching the ceiling-lane guard's literal so a research report is never
/// mistaken for a ranked `results/compare-*.md`.
#[test]
fn research_out_readme_declares_out_of_competition() {
    let readme = repo_root().join("research-out/README.md");
    let text = std::fs::read_to_string(&readme).unwrap_or_else(|e| {
        panic!(
            "expected {} to exist (WHI-1274 step 5): {e}",
            readme.display()
        );
    });
    assert!(
        text.contains("out_of_competition: true"),
        "research-out/README.md must contain the literal `out_of_competition: true`"
    );
}
