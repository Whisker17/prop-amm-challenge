use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicPtr, Ordering};

use prop_amm_executor::{AfterSwapFn, SwapFn};
use prop_amm_shared::config::SimulationConfig;
use prop_amm_shared::normalizer;
use prop_amm_shared::result::BatchResult;
use prop_amm_sim::runner;

// Must match crates/cli/src/commands/compile.rs's exported symbol names. tools/bench can't
// depend on that crate as a library (it's bin-only — `[[bin]]` only, no `[lib]`), so this is
// the one place the duplication the ticket accepts has to live (docs/DESIGN.md §2.6).
const NATIVE_SWAP_SYMBOL: &[u8] = b"__prop_amm_compute_swap_export";
const NATIVE_AFTER_SWAP_SYMBOL: &[u8] = b"__prop_amm_after_swap_export";

type FfiSwapFn = unsafe extern "C" fn(*const u8, usize) -> u64;
type FfiAfterSwapFn = unsafe extern "C" fn(*const u8, usize, *mut u8, usize);

// `SwapFn` is a plain `fn` pointer, not a closure (crates/executor/src/native.rs:4), so a
// loaded dylib's entry point can't carry any captured context. `compare` needs a candidate
// and a reference loaded at once, so there are two fixed slots (not one, as
// `crates/cli/src/commands/run.rs` uses) with two distinct trampolines each.
static LOADED_SWAP: [AtomicPtr<()>; 2] = [
    AtomicPtr::new(std::ptr::null_mut()),
    AtomicPtr::new(std::ptr::null_mut()),
];
static LOADED_AFTER_SWAP: [AtomicPtr<()>; 2] = [
    AtomicPtr::new(std::ptr::null_mut()),
    AtomicPtr::new(std::ptr::null_mut()),
];

fn call_swap(slot: usize, data: &[u8]) -> u64 {
    let ptr = LOADED_SWAP[slot].load(Ordering::Relaxed);
    let f: FfiSwapFn = unsafe { std::mem::transmute(ptr) };
    unsafe { f(data.as_ptr(), data.len()) }
}

fn swap_slot0(data: &[u8]) -> u64 {
    call_swap(0, data)
}

fn swap_slot1(data: &[u8]) -> u64 {
    call_swap(1, data)
}

const SWAP_TRAMPOLINES: [SwapFn; 2] = [swap_slot0, swap_slot1];

fn call_after_swap(slot: usize, data: &[u8], storage: &mut [u8]) {
    let ptr = LOADED_AFTER_SWAP[slot].load(Ordering::Relaxed);
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

fn after_swap_slot0(data: &[u8], storage: &mut [u8]) {
    call_after_swap(0, data, storage)
}

fn after_swap_slot1(data: &[u8], storage: &mut [u8]) {
    call_after_swap(1, data, storage)
}

const AFTER_SWAP_TRAMPOLINES: [AfterSwapFn; 2] = [after_swap_slot0, after_swap_slot1];

/// Which of the two loaded-dylib slots a build targets. `#[repr(usize)]` so a slot converts
/// straight to an array index (`slot as usize`) — no match arm needed at any call site.
#[derive(Clone, Copy, Debug)]
#[repr(usize)]
pub enum Slot {
    Zero = 0,
    One = 1,
}

pub struct LoadedNative {
    swap_fn: SwapFn,
    after_swap_fn: Option<AfterSwapFn>,
    /// The isolated `.build/runs/<hash>` directory this dylib was built in. Removed on
    /// `Drop` (see below) so it's cleaned up on *every* exit path — success, an early `?`,
    /// or a `bail!` — not just the happy path a manual cleanup call after the last use would
    /// miss.
    build_dir: Option<PathBuf>,
}

impl LoadedNative {
    /// Runs `configs` against this candidate/reference and the fixed normalizer opponent
    /// (docs/DESIGN.md §2.6, §2.8) — the one call shape every command needs.
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

impl Drop for LoadedNative {
    fn drop(&mut self) {
        cleanup(self.build_dir.as_deref());
    }
}

/// Builds `file` through the reference compile path (`prop-amm build`, native + BPF,
/// deliberately 7-10s — docs/DESIGN.md §2.6) and loads the resulting native dylib into the
/// given slot. The dylib is leaked so its symbols stay valid for the process lifetime,
/// matching `crates/cli/src/commands/run.rs`'s own pattern.
pub fn build_and_load(file: &str, slot: Slot) -> anyhow::Result<LoadedNative> {
    let native_path = build_native(file)?;
    load_native(&native_path, slot)
}

/// Best-effort removal of a build directory returned in `LoadedNative::build_dir`. Never
/// errors the caller's command — a failed cleanup shouldn't invalidate an otherwise-good
/// measurement, it's just a warning to stderr (docs/DESIGN.md §3.4: the reference path's
/// isolated directories should be cleaned after use).
fn cleanup(build_dir: Option<&Path>) {
    let Some(dir) = build_dir else { return };
    if let Err(e) = std::fs::remove_dir_all(dir) {
        // `compare` loads two builds that can share one build dir (candidate == reference,
        // e.g. a self-comparison sanity check) — the second cleanup call then legitimately
        // finds it already gone. That's not a failure worth a warning; anything else is.
        if e.kind() == std::io::ErrorKind::NotFound {
            return;
        }
        eprintln!(
            "warning: failed to clean up build dir {}: {e}",
            dir.display()
        );
    }
}

fn build_native(file: &str) -> anyhow::Result<PathBuf> {
    if !Path::new(file).exists() {
        anyhow::bail!("file not found: {file}");
    }

    let output = Command::new("cargo")
        .args(["run", "--release", "-p", "prop-amm", "--", "build", file])
        .output()
        .map_err(|e| anyhow::anyhow!("failed to run `prop-amm build {file}`: {e}"))?;

    if !output.status.success() {
        anyhow::bail!(
            "`prop-amm build {file}` failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(path) = line.trim().strip_prefix("Native: ") {
            return Ok(PathBuf::from(path.trim()));
        }
    }

    anyhow::bail!("`prop-amm build {file}` did not print a `Native: <path>` line:\n{stdout}")
}

/// `<build_dir>/target/release/lib*.{dylib,so}` -> `<build_dir>` (three levels up). `None`
/// if the printed path is shallower than expected — cleanup is then skipped, not guessed at.
/// Validates the derived directory is actually shaped like `.build/runs/<hash>`
/// (`crates/cli/src/commands/compile.rs`'s own `BUILD_RUNS_DIR`) before returning it as
/// something `cleanup` is allowed to `remove_dir_all` — a `None` here means cleanup is
/// skipped rather than deleting a directory whose shape wasn't actually confirmed.
fn build_dir_from_native_path(native_path: &Path) -> Option<PathBuf> {
    let build_dir = native_path.ancestors().nth(3)?;
    let runs = build_dir.parent()?;
    let build_root = runs.parent()?;
    if runs.file_name()? != std::ffi::OsStr::new("runs")
        || build_root.file_name()? != std::ffi::OsStr::new(".build")
    {
        return None;
    }
    Some(build_dir.to_path_buf())
}

fn load_native(native_path: &Path, slot: Slot) -> anyhow::Result<LoadedNative> {
    let lib = Box::new(
        unsafe { libloading::Library::new(native_path) }
            .map_err(|e| anyhow::anyhow!("failed to load {}: {e}", native_path.display()))?,
    );
    let lib = Box::leak(lib);
    let idx = slot as usize;

    let swap_symbol: libloading::Symbol<FfiSwapFn> = unsafe {
        lib.get(NATIVE_SWAP_SYMBOL)
            .or_else(|_| lib.get(b"compute_swap_ffi"))
    }
    .map_err(|e| {
        anyhow::anyhow!(
            "missing native swap symbol in {}: {e}",
            native_path.display()
        )
    })?;
    LOADED_SWAP[idx].store(*swap_symbol as *mut (), Ordering::Relaxed);

    let has_after_swap = if let Ok(after_symbol) = unsafe {
        lib.get::<FfiAfterSwapFn>(NATIVE_AFTER_SWAP_SYMBOL)
            .or_else(|_| lib.get::<FfiAfterSwapFn>(b"after_swap_ffi"))
    } {
        LOADED_AFTER_SWAP[idx].store(*after_symbol as *mut (), Ordering::Relaxed);
        true
    } else {
        false
    };

    Ok(LoadedNative {
        swap_fn: SWAP_TRAMPOLINES[idx],
        after_swap_fn: has_after_swap.then_some(AFTER_SWAP_TRAMPOLINES[idx]),
        build_dir: build_dir_from_native_path(native_path),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_dir_from_native_path_accepts_the_expected_shape() {
        let native = Path::new(".build/runs/abc123/target/release/libuser_program.dylib");
        assert_eq!(
            build_dir_from_native_path(native),
            Some(PathBuf::from(".build/runs/abc123"))
        );
    }

    #[test]
    fn build_dir_from_native_path_rejects_an_unexpected_shape() {
        let native = Path::new("/tmp/some/other/place/target/release/libuser_program.dylib");
        assert_eq!(build_dir_from_native_path(native), None);
    }

    #[test]
    fn build_dir_from_native_path_rejects_a_shallow_path() {
        let native = Path::new("libuser_program.dylib");
        assert_eq!(build_dir_from_native_path(native), None);
    }
}
