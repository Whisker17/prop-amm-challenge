use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicPtr, Ordering};

use prop_amm_executor::{AfterSwapFn, SwapFn};

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
static LOADED_SWAP: [AtomicPtr<()>; 2] =
    [AtomicPtr::new(std::ptr::null_mut()), AtomicPtr::new(std::ptr::null_mut())];
static LOADED_AFTER_SWAP: [AtomicPtr<()>; 2] =
    [AtomicPtr::new(std::ptr::null_mut()), AtomicPtr::new(std::ptr::null_mut())];

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

fn call_after_swap(slot: usize, data: &[u8], storage: &mut [u8]) {
    let ptr = LOADED_AFTER_SWAP[slot].load(Ordering::Relaxed);
    let f: FfiAfterSwapFn = unsafe { std::mem::transmute(ptr) };
    unsafe { f(data.as_ptr(), data.len(), storage.as_mut_ptr(), storage.len()) }
}

fn after_swap_slot0(data: &[u8], storage: &mut [u8]) {
    call_after_swap(0, data, storage)
}

fn after_swap_slot1(data: &[u8], storage: &mut [u8]) {
    call_after_swap(1, data, storage)
}

#[derive(Clone, Copy, Debug)]
pub enum Slot {
    Zero,
    One,
}

impl Slot {
    fn index(self) -> usize {
        match self {
            Slot::Zero => 0,
            Slot::One => 1,
        }
    }
}

pub struct LoadedNative {
    pub swap_fn: SwapFn,
    pub after_swap_fn: Option<AfterSwapFn>,
}

/// Builds `file` through the reference compile path (`prop-amm build`, native + BPF,
/// deliberately 7-10s — docs/DESIGN.md §2.6) and loads the resulting native dylib into the
/// given slot. The dylib is leaked so its symbols stay valid for the process lifetime,
/// matching `crates/cli/src/commands/run.rs`'s own pattern.
pub fn build_and_load(file: &str, slot: Slot) -> anyhow::Result<LoadedNative> {
    let native_path = build_native(file)?;
    load_native(&native_path, slot)
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

fn load_native(native_path: &Path, slot: Slot) -> anyhow::Result<LoadedNative> {
    let lib = Box::new(
        unsafe { libloading::Library::new(native_path) }
            .map_err(|e| anyhow::anyhow!("failed to load {}: {e}", native_path.display()))?,
    );
    let lib = Box::leak(lib);
    let idx = slot.index();

    let swap_symbol: libloading::Symbol<FfiSwapFn> = unsafe {
        lib.get(NATIVE_SWAP_SYMBOL).or_else(|_| lib.get(b"compute_swap_ffi"))
    }
    .map_err(|e| anyhow::anyhow!("missing native swap symbol in {}: {e}", native_path.display()))?;
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

    let swap_fn: SwapFn = match slot {
        Slot::Zero => swap_slot0,
        Slot::One => swap_slot1,
    };
    let after_swap_fn: Option<AfterSwapFn> = has_after_swap.then_some(match slot {
        Slot::Zero => after_swap_slot0,
        Slot::One => after_swap_slot1,
    });

    Ok(LoadedNative { swap_fn, after_swap_fn })
}
