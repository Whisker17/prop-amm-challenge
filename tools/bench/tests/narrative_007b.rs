//! Structural check that `strategies/007b-grounded-k-anchor/NOTES.md` still carries
//! the WHI-1273 joint-fit narrative: process, committed numbers, the stale-mid `1/k`
//! mechanism, and the production-oracle bound. Drives the shipped NOTES file, not a
//! copy of its claims.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn notes() -> String {
    let path = repo_root().join("strategies/007b-grounded-k-anchor/NOTES.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("failed to read {}: {e}", path.display());
    })
}

#[test]
fn notes_recounts_joint_fit_process_and_committed_numbers() {
    let text = notes();
    for needle in [
        "Containment",
        "Unconditional 300-point",
        "k-sweep",
        "401.28",
        "170/300",
        "K_BPS=10_000, FEE_BPS=66",
        "1.401555",
        "segment was not read",
        "results/2026-08-25-fit-007b-grounded-k-anchor.md",
        "results/2026-08-25-ksweep-007b-grounded-k-anchor.md",
        "results/2026-08-25-compare-007b-grounded-k-anchor-vs-001-cpmm-fee.md",
    ] {
        assert!(
            text.contains(needle),
            "007b NOTES.md must name `{needle}` (joint-fit process/results)"
        );
    }
}

#[test]
fn notes_explains_stale_mid_one_over_k_arb_loss_not_runaway() {
    let text = notes();
    for needle in [
        "1/k",
        "stale",
        "1.000",
        "1_000_231",
        "recursive-runaway",
        "extractable size",
    ] {
        assert!(
            text.contains(needle),
            "007b NOTES.md must name `{needle}` (stale-mid 1/k mechanism)"
        );
    }
}

#[test]
fn notes_qualifies_production_oracle_as_specified_not_measured() {
    let text = notes();
    for needle in [
        "not measured",
        "DPPOracle",
        "pre-arbitrage",
        "honesty constraint",
        "IOracle",
    ] {
        assert!(
            text.contains(needle),
            "007b NOTES.md must name `{needle}` (production bound)"
        );
    }
    // Citation ban: colleague snapshot headlines must not appear in strategies/**
    // (`research-out/README.md`). A pointer at DESIGN.md §10/§11 is allowed.
    for banned in ["netEdge", "−399", "-399"] {
        assert!(
            !text.contains(banned),
            "007b NOTES.md must not paste research-out headline `{banned}`"
        );
    }
}
