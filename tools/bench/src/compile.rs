use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicPtr, Ordering};

use prop_amm_executor::{AfterSwapFn, SwapFn};
use prop_amm_shared::config::SimulationConfig;
use prop_amm_shared::normalizer;
use prop_amm_shared::result::BatchResult;
use prop_amm_sim::runner;

use crate::estimator_probe;
use crate::telemetry::{self, L1Sim};

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

    /// Like `run_batch`, but wraps both AMMs' `after_swap` in pass-through recorders and
    /// returns each simulation's L1 telemetry alongside the usual edge numbers
    /// (docs/DESIGN.md §2.7). Kept as a separate method rather than a flag on `run_batch` so
    /// the non-telemetry path stays exactly what it was — the fastest way to prove the two
    /// are behaviourally identical is to call two different, independently-readable methods.
    pub fn run_batch_with_l1(
        &self,
        configs: Vec<SimulationConfig>,
    ) -> anyhow::Result<(BatchResult, Vec<L1Sim>)> {
        telemetry::run_batch_native_with_l1(self.swap_fn, self.after_swap_fn, configs)
    }

    /// WHI-1225's Probe A: runs this candidate (expected to be `005-vol-adaptive-cpmm-fee`)
    /// alongside a shadow accumulator that replicates both variance normalizations from the
    /// same `after_swap` payload — see `estimator_probe.rs`.
    pub fn run_batch_with_005_estimator_probe(
        &self,
        configs: &[SimulationConfig],
    ) -> anyhow::Result<Vec<estimator_probe::Vol005ProbeSim>> {
        estimator_probe::run_005_dual_estimator_probe(self.swap_fn, self.after_swap_fn, configs)
    }

    /// WHI-1225's Probe A "Added scope": runs this candidate (expected to be
    /// `004-ewma-shock-decay-fee`) alongside a shadow accumulator that replicates its own
    /// `ewma_vol` EWMA and records its floor-sweep distribution — see `estimator_probe.rs`.
    pub fn run_batch_with_004_floor_probe(
        &self,
        configs: &[SimulationConfig],
    ) -> anyhow::Result<Vec<estimator_probe::Ewma004ProbeSim>> {
        estimator_probe::run_004_floor_probe(self.swap_fn, self.after_swap_fn, configs)
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

    /// The non-invasiveness acceptance criterion's literal wording — "the same **candidate**
    /// over the same **segment**" — rather than the fast, hermetic proxy the default test
    /// suite uses (a synthetic swap fn, `telemetry.rs::telemetry_is_bit_identical_to_no_
    /// telemetry`). Ignored by default: it shells out to `prop-amm build` for a real compile
    /// plus 1000 sims at the segment's full 10,000 steps, ~190s measured — disproportionate
    /// for every `cargo test --workspace` run given the property is already covered
    /// hermetically. Run explicitly with `cargo test -p prop-amm-bench --release --
    /// --ignored`.
    ///
    /// Cargo runs test binaries with the *crate's own* manifest directory as the working
    /// directory. `set_current_dir` below relocates the process to the repo root so this test
    /// can use ordinary relative paths like the real `bench` binary would — but that alone
    /// does **not** make this test runnable from every location: `build_and_load` (via
    /// upstream's `ensure_build_dir`) creates an isolated build package with no `[workspace]`
    /// table of its own, and if the repo root it lands under is itself nested inside another
    /// git worktree of the same repo (as `.claude/worktrees/<name>` is — the location
    /// `AGENTS.md` mandates for issue work), `cargo`'s ancestor search walks past that
    /// worktree's own `Cargo.toml` and resolves the *primary clone's* workspace instead,
    /// which then refuses the isolated package as an unexcluded member. This test therefore
    /// only passes run from a location that is not nested under another checkout of this
    /// repo (e.g. a detached scratch worktree created with `git worktree add --detach
    /// /tmp/<name> <sha>`, not `.claude/worktrees/<name>`) — verified passing that way.
    #[test]
    #[ignore = "compiles the real starter program and runs 1000 sims, ~190s; only passes \
                from a location not nested under another worktree of this repo (see the \
                doc comment above); run explicitly to reproduce the literal \
                non-invasiveness proof against a real candidate over a declared segment"]
    fn starter_over_observation_segment_is_bit_identical_with_and_without_telemetry() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        std::env::set_current_dir(&repo_root).expect("chdir to repo root");

        let bench_config = crate::config::BenchConfig::load_default().expect("load bench config");
        let segment = bench_config
            .segment("observation")
            .expect("observation segment is declared");
        let base = SimulationConfig::default();
        let configs = segment.sim_configs(&base);

        let loaded = build_and_load("programs/starter/src/lib.rs", Slot::Zero)
            .expect("build the real starter program");

        let without_telemetry = loaded
            .run_batch(configs.clone())
            .expect("run_batch (no telemetry)");
        let (with_telemetry, _l1_sims) = loaded
            .run_batch_with_l1(configs)
            .expect("run_batch_with_l1 (telemetry on)");

        assert_eq!(without_telemetry.total_edge, with_telemetry.total_edge);
        for (a, b) in without_telemetry
            .results
            .iter()
            .zip(with_telemetry.results.iter())
        {
            assert_eq!(a.seed, b.seed);
            assert_eq!(a.submission_edge, b.submission_edge);
        }
    }
}
