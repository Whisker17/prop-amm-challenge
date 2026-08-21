//! The fast compile path (docs/DESIGN.md §2.6). Deliberately **not** unified with the
//! reference path (docs/DESIGN.md §4.3: "Deliberately not abstracted. It duplicates upstream
//! logic; the parity gate is the control, not an interface"). This module duplicates three
//! things from `crates/cli/src/commands/compile.rs` — **the authority; check there first if
//! anything here drifts** — because that crate is `[[bin]]`-only (no `[lib]`), so
//! `tools/bench` cannot depend on it as a library:
//!
//! - the submission `Cargo.toml` template (`compile.rs`'s `CARGO_TOML`),
//! - native shim injection (`compile.rs::native_shim_source`),
//! - `unsafe` rejection (`compile.rs::source_contains_unsafe_keyword`).
//!
//! The one deliberate difference from the reference path: `compile.rs::ensure_build_dir`
//! keys an isolated directory by source hash — a fresh `pinocchio`/`wincode`/`darling`/`syn`
//! build per parameter point, 7-10s. Here there is exactly one fixed directory,
//! `.build/fast/`, with a shared `target/`; only `src/lib.rs` changes between points, so
//! `cargo build` only relinks.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicPtr, Ordering};

use prop_amm_executor::{AfterSwapFn, SwapFn};
use prop_amm_shared::config::SimulationConfig;
use prop_amm_shared::normalizer;
use prop_amm_shared::result::BatchResult;
use prop_amm_sim::runner;

/// Must match `crates/cli/src/commands/compile.rs`'s exported symbol names (and
/// `tools/bench/src/compile.rs`'s own copy of them — every compile path agrees on this ABI).
const NATIVE_SWAP_SYMBOL: &[u8] = b"__prop_amm_compute_swap_export";
const NATIVE_AFTER_SWAP_SYMBOL: &[u8] = b"__prop_amm_after_swap_export";

/// The fast build directory, anchored at the workspace root via `CARGO_MANIFEST_DIR`
/// (resolved at compile time to `<root>/tools/bench`) rather than a bare relative path.
/// A bare `".build/fast"` resolves against `std::env::current_dir()` at *runtime* — fine
/// when `bench` is invoked from the workspace root, but wrong via `cargo test` (whose cwd
/// is this crate's own directory: the build dir would land at `tools/bench/.build/fast`
/// instead). This fixes *that* case (WHI-1205); it does not by itself fix a nested git
/// worktree (`.claude/worktrees/<name>/`) — the resolved path is still correct there, but
/// whether it's isolated from the *outer* workspace's `[profile.release]` is a separate
/// question `CARGO_TOML`'s own `[workspace]` table (below) answers unconditionally.
///
/// Caveat baked in by this fix: `CARGO_MANIFEST_DIR` is a compile-time constant, so a
/// `bench` binary is tied to the source tree it was built from — copying or installing
/// the binary elsewhere still writes to *that build's* `.build/fast`, not a directory
/// relative to wherever the binary now lives. Acceptable here: `bench` is always built
/// and run in place from within its own workspace checkout, never distributed.
fn fast_build_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.build/fast")
}

/// Identical to `crates/cli/src/commands/compile.rs`'s `CARGO_TOML`, except two things:
/// the `submission-sdk` path (relative to *this* fixed directory's depth —
/// `.build/fast/`, two segments, vs. `compile.rs`'s per-hash `.build/runs/<hash>/`,
/// three) and the empty `[workspace]` table below.
///
/// That table is load-bearing, not boilerplate (WHI-1205): without it, whether this
/// package is correctly isolated from the *outer* workspace's `[profile.release]`
/// (`lto=true`, `codegen-units=1`) depends on the enclosing workspace's own `exclude`
/// list actually covering wherever `.build/fast` happens to sit — true when
/// `fast_build_dir()` lands under the workspace root, **false** if it's nested one level
/// deeper (a git worktree at `.claude/worktrees/<name>/`, this repo's own mandated
/// layout, sitting inside the *primary clone*, whose `exclude = [".build"]` only matches
/// a direct child, not `.claude/worktrees/<name>/.build`). In that case cargo doesn't
/// silently apply the outer profile — it hard-errors trying to fold the package into the
/// outer workspace (`current package believes it's in a workspace when it's not`). An
/// empty `[workspace]` table makes this package its own workspace root unconditionally,
/// independent of nesting depth or any ancestor's `exclude` list — cargo's own suggested
/// fix for exactly this error.
const CARGO_TOML: &str = r#"[package]
name = "user_program"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "lib"]

[dependencies]
pinocchio = "0.7"
wincode = { version = "0.4", default-features = false, features = ["derive"] }
prop-amm-submission-sdk = { path = "../../crates/submission-sdk" }

[features]
no-entrypoint = []

[workspace]
"#;

type FfiSwapFn = unsafe extern "C" fn(*const u8, usize) -> u64;
type FfiAfterSwapFn = unsafe extern "C" fn(*const u8, usize, *mut u8, usize);

// One slot: the search never evaluates more than one candidate at a time. `bench parity`
// loads a fast-path build and a reference-path build concurrently, but the reference path's
// own two slots (`tools/bench/src/compile.rs`) are an independent static — this one is
// dedicated to the fast path so the two never collide.
static LOADED_SWAP: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static LOADED_AFTER_SWAP: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

fn fast_swap(data: &[u8]) -> u64 {
    let ptr = LOADED_SWAP.load(Ordering::Relaxed);
    let f: FfiSwapFn = unsafe { std::mem::transmute(ptr) };
    unsafe { f(data.as_ptr(), data.len()) }
}

fn fast_after_swap(data: &[u8], storage: &mut [u8]) {
    let ptr = LOADED_AFTER_SWAP.load(Ordering::Relaxed);
    let f: FfiAfterSwapFn = unsafe { std::mem::transmute(ptr) };
    unsafe {
        f(
            data.as_ptr(),
            data.len(),
            storage.as_mut_ptr(),
            storage.len(),
        )
    }
}

/// A fast-path build, loaded from a throwaway temp copy of `.build/fast`'s compiled dylib —
/// not the build directory's own `target/release/...` path, so a search reloading it
/// hundreds of times per run never has two things claiming to be "the current build" at
/// once. `Drop` unloads the library and deletes the temp copy; `.build/fast/` itself is
/// never touched by `Drop` — it is meant to persist and be reused by the next point.
pub struct LoadedFast {
    swap_fn: SwapFn,
    after_swap_fn: Option<AfterSwapFn>,
    // Declared in the order they must drop: unload before deleting the file it was mapped
    // from (harmless either way on POSIX, but this keeps the invariant explicit).
    _library: libloading::Library,
    _temp: tempfile::TempPath,
}

impl LoadedFast {
    /// Runs `configs` against this candidate and the fixed normalizer opponent — the same
    /// call shape as `tools/bench/src/compile.rs::LoadedNative::run_batch`.
    pub fn run_batch(&self, configs: Vec<SimulationConfig>) -> anyhow::Result<BatchResult> {
        runner::run_batch_native(
            self.swap_fn,
            self.after_swap_fn,
            normalizer::compute_swap,
            Some(normalizer::after_swap),
            configs,
            None,
        )
    }
}

fn native_ext() -> &'static str {
    if cfg!(target_os = "macos") {
        ".dylib"
    } else {
        ".so"
    }
}

/// Writes `.build/fast/{Cargo.toml,src/lib.rs}`, skipping a write whose content already
/// matches (mirrors `compile.rs::ensure_build_dir`'s idempotent-write, just without the hash
/// key — there is only ever one fast-path directory). `safe_source` must already be the
/// *safe* submission source — shim-injected, unsafe-checked (`make_safe_source`).
pub fn ensure_fast_build_dir(safe_source: &str) -> anyhow::Result<PathBuf> {
    ensure_fast_build_dir_at(&fast_build_dir(), safe_source)
}

/// Whether `.build/fast/target/` already exists. A caller measuring compile time (`bench
/// fit`'s timing report) should check this **before** the first compile of a run: if it's
/// already `true`, even that first sample is a genuinely warm compile, not a one-time
/// dependency build — a fact about the directory's prior state, not something to be guessed
/// from a sample's position in the sequence.
pub fn fast_build_dir_is_warm() -> bool {
    fast_build_dir().join("target").exists()
}

fn ensure_fast_build_dir_at(build_dir: &Path, safe_source: &str) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(build_dir.join("src"))?;

    let cargo_path = build_dir.join("Cargo.toml");
    let should_write_cargo = match std::fs::read_to_string(&cargo_path) {
        Ok(existing) => existing != CARGO_TOML,
        Err(_) => true,
    };
    if should_write_cargo {
        std::fs::write(&cargo_path, CARGO_TOML)?;
    }

    let source_path = build_dir.join("src/lib.rs");
    let source_bytes = safe_source.as_bytes();
    let should_write_source = match std::fs::read(&source_path) {
        Ok(existing) => existing != source_bytes,
        Err(_) => true,
    };
    if should_write_source {
        std::fs::write(&source_path, source_bytes)?;
    }

    Ok(build_dir.to_path_buf())
}

/// Builds `.build/fast` and loads the resulting dylib. `source` must already be the *safe*
/// submission source (`make_safe_source`) — this function does not re-check safety.
pub fn compile_and_load_fast(safe_source: &str) -> anyhow::Result<LoadedFast> {
    let build_dir = ensure_fast_build_dir(safe_source)?;

    let status = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .arg("--manifest-path")
        .arg(build_dir.join("Cargo.toml"))
        .arg("--features")
        .arg("no-entrypoint")
        .status()
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to run `cargo build` in {}: {e}",
                build_dir.display()
            )
        })?;

    if !status.success() {
        anyhow::bail!("fast-path build failed in {}", build_dir.display());
    }

    let native_path = find_native_lib(&build_dir)?;
    load_fast(&native_path)
}

fn find_native_lib(build_dir: &Path) -> anyhow::Result<PathBuf> {
    let release_dir = build_dir.join("target").join("release");
    let ext = if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    };

    if let Ok(entries) = std::fs::read_dir(&release_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("lib") && name.ends_with(ext) {
                return Ok(entry.path());
            }
        }
    }

    anyhow::bail!(
        "no native library found in {}/target/release/",
        build_dir.display()
    )
}

fn load_fast(native_path: &Path) -> anyhow::Result<LoadedFast> {
    // Copy to a fresh temp file before loading: a search reuses this exact output path
    // hundreds of times in one process, and reopening the same path repeatedly risks stale
    // caching by inode/mtime on some platforms. A unique temp copy per load sidesteps the
    // question entirely, at the cost of one cheap file copy per point.
    let temp = tempfile::Builder::new()
        .prefix("prop-amm-fast-")
        .suffix(native_ext())
        .tempfile()
        .map_err(|e| anyhow::anyhow!("failed to create temp file for fast-path dylib: {e}"))?;
    std::fs::copy(native_path, temp.path()).map_err(|e| {
        anyhow::anyhow!("failed to copy {} to temp file: {e}", native_path.display())
    })?;
    let temp = temp.into_temp_path();

    let lib = unsafe { libloading::Library::new(&temp) }
        .map_err(|e| anyhow::anyhow!("failed to load {}: {e}", temp.display()))?;

    let swap_symbol: libloading::Symbol<FfiSwapFn> = unsafe {
        lib.get(NATIVE_SWAP_SYMBOL)
            .or_else(|_| lib.get(b"compute_swap_ffi"))
    }
    .map_err(|e| anyhow::anyhow!("missing native swap symbol in {}: {e}", temp.display()))?;
    LOADED_SWAP.store(*swap_symbol as *mut (), Ordering::Relaxed);

    let has_after_swap = if let Ok(after_symbol) = unsafe {
        lib.get::<FfiAfterSwapFn>(NATIVE_AFTER_SWAP_SYMBOL)
            .or_else(|_| lib.get::<FfiAfterSwapFn>(b"after_swap_ffi"))
    } {
        LOADED_AFTER_SWAP.store(*after_symbol as *mut (), Ordering::Relaxed);
        true
    } else {
        false
    };

    Ok(LoadedFast {
        swap_fn: fast_swap,
        after_swap_fn: has_after_swap.then_some(fast_after_swap),
        _library: lib,
        _temp: temp,
    })
}

/// Wraps `source` with the native FFI shim and rejects unsafe code — the exact checks
/// `crates/cli/src/commands/compile.rs::make_safe_submission_source` performs. Call this
/// once per candidate source before `compile_and_load_fast`.
pub fn make_safe_source(source: &str) -> anyhow::Result<String> {
    if source_contains_unsafe_keyword(source)? {
        anyhow::bail!(
            "unsafe Rust is not allowed in submissions; remove all `unsafe` blocks/functions/keywords"
        );
    }

    let analysis = analyze_source(source)?;
    if !analysis.has_compute_swap {
        anyhow::bail!("submission must define `fn compute_swap(data: &[u8]) -> u64`");
    }

    let mut safe_source = source.to_string();
    safe_source.push('\n');
    safe_source.push('\n');
    safe_source.push_str(&native_shim_source(analysis.has_after_swap));
    Ok(safe_source)
}

#[derive(Clone, Copy)]
struct SourceAnalysis {
    has_compute_swap: bool,
    has_after_swap: bool,
}

fn analyze_source(source: &str) -> anyhow::Result<SourceAnalysis> {
    let parsed = syn::parse_file(source)
        .map_err(|e| anyhow::anyhow!("failed to parse source for function checks: {e}"))?;

    let mut has_compute_swap = false;
    let mut has_after_swap = false;
    for item in parsed.items {
        if let syn::Item::Fn(item_fn) = item {
            let name = item_fn.sig.ident.to_string();
            if name == "compute_swap" {
                has_compute_swap = true;
            } else if name == "after_swap" {
                has_after_swap = true;
            }
        }
    }

    Ok(SourceAnalysis {
        has_compute_swap,
        has_after_swap,
    })
}

fn native_shim_source(has_after_swap: bool) -> String {
    let after_swap_target = if has_after_swap {
        "after_swap"
    } else {
        "__prop_amm_after_swap_noop"
    };

    format!(
        r#"#[cfg(not(target_os = "solana"))]
#[inline]
fn __prop_amm_after_swap_noop(_data: &[u8], _storage: &mut [u8]) {{}}

#[cfg(not(target_os = "solana"))]
#[no_mangle]
pub extern "C" fn __prop_amm_compute_swap_export(data: *const u8, len: usize) -> u64 {{
    prop_amm_submission_sdk::ffi_compute_swap(data, len, compute_swap)
}}

#[cfg(not(target_os = "solana"))]
#[no_mangle]
pub extern "C" fn __prop_amm_after_swap_export(
    data: *const u8,
    data_len: usize,
    storage: *mut u8,
    storage_len: usize,
) {{
    prop_amm_submission_sdk::ffi_after_swap(
        data,
        data_len,
        storage,
        storage_len,
        {},
    );
}}
"#,
        after_swap_target
    )
}

fn source_contains_unsafe_keyword(source: &str) -> anyhow::Result<bool> {
    let stream: proc_macro2::TokenStream = source
        .parse()
        .map_err(|e| anyhow::anyhow!("failed to parse source for safety checks: {e}"))?;
    Ok(token_stream_contains_unsafe(stream))
}

fn token_stream_contains_unsafe(stream: proc_macro2::TokenStream) -> bool {
    stream.into_iter().any(token_tree_contains_unsafe)
}

fn token_tree_contains_unsafe(tree: proc_macro2::TokenTree) -> bool {
    match tree {
        proc_macro2::TokenTree::Ident(ident) => ident == "unsafe",
        proc_macro2::TokenTree::Group(group) => token_stream_contains_unsafe(group.stream()),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_SUBMISSION: &str = "pub fn compute_swap(_data: &[u8]) -> u64 { 0 }";

    #[test]
    fn cargo_toml_points_at_the_fast_build_dirs_own_depth() {
        assert!(CARGO_TOML.contains(r#"path = "../../crates/submission-sdk""#));
    }

    #[test]
    fn cargo_toml_declares_its_own_workspace() {
        // WHI-1205: without this, isolation from the outer workspace's [profile.release]
        // depends on the outer `exclude` list actually covering wherever `.build/fast`
        // happens to be nested — see
        // `a_bare_relative_build_dir_gets_folded_into_an_outer_workspace_when_nested`.
        assert!(CARGO_TOML.contains("[workspace]"));
    }

    #[test]
    fn fast_build_dir_resolves_to_the_real_workspace_root() {
        // A regression guard against reverting to a bare relative literal (which
        // `std::env::current_dir()` would make relative, not absolute) — `env!` is a
        // compile-time constant, so this is absolute regardless of runtime cwd.
        let resolved = fast_build_dir();
        assert!(
            resolved.is_absolute(),
            "expected an absolute path anchored at CARGO_MANIFEST_DIR, got {}",
            resolved.display()
        );
        // Not just a formula match: the grandparent must be the *actual* workspace root
        // on disk (verified by its own root Cargo.toml declaring `[workspace]`), proving
        // `fast_build_dir()` really points where it claims to, not merely at some path
        // shaped like it.
        let workspace_root = resolved
            .parent()
            .and_then(Path::parent)
            .expect("`.build/fast` should have a grandparent directory");
        let root_manifest = std::fs::read_to_string(workspace_root.join("Cargo.toml"))
            .expect("workspace root should have its own Cargo.toml");
        assert!(
            root_manifest.contains("[workspace]"),
            "expected {} to be the workspace root",
            workspace_root.display()
        );
    }

    #[test]
    fn a_bare_relative_build_dir_gets_folded_into_an_outer_workspace_when_nested() {
        // Reproduces WHI-1205's discovered failure mode directly (not just asserted from
        // reading cargo's docs): a package excluded from its *immediate* parent workspace
        // but nested two levels under an *outer* workspace whose `exclude` list only
        // matches a direct child (exactly this repo's root `exclude = [".build"]` against
        // `.claude/worktrees/<name>/.build/fast`) gets folded into that outer workspace
        // instead of standing alone — a hard `cargo build` error, not a silent profile
        // leak. No real dependencies needed to reproduce this; it's a workspace-discovery
        // question, not a compilation one.
        let tmp = tempfile::tempdir().unwrap();
        let outer = tmp.path();
        std::fs::write(
            outer.join("Cargo.toml"),
            "[workspace]\nmembers = []\nexclude = [\".build\"]\n",
        )
        .unwrap();

        // The "worktree" — same exclude shape as the real repo's own root Cargo.toml.
        let inner = outer.join("nested-worktree");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(
            inner.join("Cargo.toml"),
            "[workspace]\nmembers = []\nexclude = [\".build\"]\n",
        )
        .unwrap();

        // The package, nested two levels under `inner` — like `.build/fast` under
        // `.claude/worktrees/<name>/`. No `[workspace]` table: reproduces the bug.
        let pkg_dir = inner.join(".build").join("fast");
        std::fs::create_dir_all(pkg_dir.join("src")).unwrap();
        std::fs::write(pkg_dir.join("src/lib.rs"), "pub fn ping() -> u32 { 1 }\n").unwrap();
        std::fs::write(
            pkg_dir.join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();

        let output = Command::new("cargo")
            .arg("build")
            .arg("--manifest-path")
            .arg(pkg_dir.join("Cargo.toml"))
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "expected the bare-relative-style package (no [workspace] table) nested two \
             levels deep to fail exactly like `.build/fast` used to"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("believes it's in a workspace when it's not"),
            "expected cargo's specific workspace-folding error, got: {stderr}"
        );

        // The fix: an empty `[workspace]` table (exactly what `CARGO_TOML` declares)
        // makes the same package its own root regardless of nesting depth.
        std::fs::write(
            pkg_dir.join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
        )
        .unwrap();
        let status = Command::new("cargo")
            .arg("build")
            .arg("--manifest-path")
            .arg(pkg_dir.join("Cargo.toml"))
            .status()
            .unwrap();
        assert!(
            status.success(),
            "an empty [workspace] table should isolate the package regardless of nesting"
        );
    }

    #[test]
    fn ensure_fast_build_dir_writes_cargo_toml_and_source() {
        let tmp = tempfile::tempdir().unwrap();
        let build_dir = ensure_fast_build_dir_at(tmp.path(), "fn compute_swap() {}").unwrap();
        assert_eq!(
            std::fs::read_to_string(build_dir.join("Cargo.toml")).unwrap(),
            CARGO_TOML
        );
        assert_eq!(
            std::fs::read_to_string(build_dir.join("src/lib.rs")).unwrap(),
            "fn compute_swap() {}"
        );
    }

    #[test]
    fn ensure_fast_build_dir_reuses_the_same_directory_across_points() {
        let tmp = tempfile::tempdir().unwrap();
        ensure_fast_build_dir_at(tmp.path(), "const A: u128 = 1;").unwrap();
        ensure_fast_build_dir_at(tmp.path(), "const A: u128 = 2;").unwrap();

        // Only ever one `src/`, one `Cargo.toml` — never a second directory per point.
        let entries: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(
            entries.len(),
            2,
            "expected exactly Cargo.toml and src/, got {entries:?}"
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("src/lib.rs")).unwrap(),
            "const A: u128 = 2;"
        );
    }

    #[test]
    fn make_safe_source_rejects_unsafe() {
        let err = make_safe_source("pub fn compute_swap(_data: &[u8]) -> u64 { unsafe { 0 } }")
            .unwrap_err();
        assert!(err.to_string().contains("unsafe Rust is not allowed"));
    }

    #[test]
    fn make_safe_source_requires_compute_swap() {
        let err = make_safe_source("pub fn not_compute_swap() {}").unwrap_err();
        assert!(err.to_string().contains("must define `fn compute_swap"));
    }

    #[test]
    fn make_safe_source_injects_the_noop_after_swap_shim_when_absent() {
        let safe = make_safe_source(VALID_SUBMISSION).unwrap();
        assert!(safe.contains("__prop_amm_after_swap_noop"));
        assert!(safe.contains("__prop_amm_compute_swap_export"));
    }

    #[test]
    fn make_safe_source_wires_a_real_after_swap_when_present() {
        let source = format!(
            "{VALID_SUBMISSION}\npub fn after_swap(_data: &[u8], _storage: &mut [u8]) {{}}"
        );
        let safe = make_safe_source(&source).unwrap();
        assert!(safe.contains("prop_amm_submission_sdk::ffi_after_swap(\n        data,\n        data_len,\n        storage,\n        storage_len,\n        after_swap,\n    );"));
    }

    #[test]
    fn find_native_lib_locates_the_platform_dylib() {
        let tmp = tempfile::tempdir().unwrap();
        let release_dir = tmp.path().join("target/release");
        std::fs::create_dir_all(&release_dir).unwrap();
        let ext = if cfg!(target_os = "macos") {
            "dylib"
        } else {
            "so"
        };
        let lib_path = release_dir.join(format!("libuser_program.{ext}"));
        std::fs::write(&lib_path, b"not a real dylib").unwrap();

        assert_eq!(find_native_lib(tmp.path()).unwrap(), lib_path);
    }

    #[test]
    fn find_native_lib_errors_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(find_native_lib(tmp.path()).is_err());
    }
}
