# 008 — Lagging-VWAP Directional Fee + Profile Ensemble

| Field | Value |
| --- | --- |
| **Name** | (source's own submission name — not carried into our registry; see `strategies/008-lagging-vwap-fee/NOTES.md` § Provenance) |
| **Source form** | source (Rust, direct `pinocchio` submission shape) |
| **Original material** | `strategy.rs`, `LEARNINGS.md`, `upstream-README.md` (from the source repo's own `README.md`), `confusion_log.md` in this directory (unmodified) |
| **Known parameters** | see below |

## Provenance gate (WHI-1236 Step 0 — owner decision, recorded in the issue, not here)

This is another **live competitor's current** competition submission, not a past entry
(`004`) or a licensed third-party library (`007`). `houseofjiao/prop-amm-challenge` carries
no LICENSE file, and neither does upstream `benedictbrady/prop-amm-challenge`
(`license: null` on both, verified via the GitHub API) — default all-rights-reserved.

Copying this material into this directory for internal analysis was explicitly approved
(WHI-1236 Step 0(a), owner decision recorded 2026-08-22). **Verbatim submission of the
artifact to the challenge is a separate question, left undecided in the issue and
reconfirmed in this session as a standing "no" for this issue's lifetime** — nothing in
this repository treats that as settled, and it is not this directory's decision to make.

## Original material location

Repository: <https://github.com/houseofjiao/prop-amm-challenge>, pinned at commit
`152697153d93baf0ad82a5bac6c485bf17697b7c` (2026-02-20, verified via the GitHub API at
fetch time — commit message "Remove hardcoded user path from coarse_sweep_cntema.py").
Mechanism code (`strategy.rs`) last changed at `d7bddcf247` ("Fix BPF stack overflow:
isolate 1024-byte storage alloc into separate frame"); `dfc62a7055` is the source's own
"Edge 501.62, last known-good BPF build" anchor commit, which their history shows was
explicitly reverted to at some point in their ~90-commit tuning campaign. That `501.62` is
identification metadata (which commit this is), not a comparable number — it was measured
on the source author's own pre-2026-02-16 harness vintage and may not be cited as evidence
anywhere in this project (§ Provenance gate above; docs/DESIGN.md §6.2/§8).

| File here | Upstream path (at the pinned commit) | Why |
| --- | --- | --- |
| `strategy.rs` | `strategy.rs` (repo root) | the mechanism, verbatim (776 lines) |
| `LEARNINGS.md` | `LEARNINGS.md` | a measured dead-end corpus from the source author's own tuning campaign — independently corroborates two of this project's own `WHI-1235` findings (docs/DESIGN.md §8, findings 2 and 4) from a second, unrelated codebase (498 lines) |
| `upstream-README.md` | `README.md` | dates the harness vintage the source's own numbers were produced on (299 lines) |
| `confusion_log.md` | `confusion_log.md` | records which harness ambiguities the source author hit (35 lines) |

Not copied here, per the porting issue's own instruction: `python/*.py` (consumes a CSV
their own instrumented sim emits; unrunnable here without violating docs/DESIGN.md §3.2's
upstream boundary), and `starter.rs` / `lvr_sweep_output.txt` /
`confusion_streamlining_proposals.md` (scratch, no mechanism content).

**No number from this repository may be cited as evidence anywhere in this project**
(docs/DESIGN.md §6.2/§8) — their headline result was measured on a harness vintage that
predates upstream's arb-direction fix (`97a1c67` et al., 2026-02-13), one day after their
shared history diverges from upstream (`afa98be0b1`, 2026-02-12). Only this project's own
re-measurement, starting at `strategies/008-lagging-vwap-fee/NOTES.md`'s pre-registered
probe, counts as evidence here. Their `LEARNINGS.md`'s own correlation coefficients
(`cnt_ema -> norm_fee` r=0.662, `arb_prefers_us -> norm_fee` r=0.828) are cited in
docs/DESIGN.md §8 as third-party-reported corroboration only, never as this project's own
measurement.

## Mechanism

Confirmed directly from `strategy.rs` (not from memory) — see
`strategies/008-lagging-vwap-fee/lib.rs` and its own header comment for the full derivation
this port carries forward verbatim. Summary:

- **Reference price.** A slow, Kalman-filtered VWAP `p_hat` blended with a faster EMA
  `p_hat_fast` under high shock (`p_ref = (1-w)*p_hat + w*p_hat_fast`, `w = min(shock_hat,
  0.3)`) stands in for the fair price the interface does not supply. The lag is load-bearing
  by the source's own measurement (`LEARNINGS.md` § "Signed Flow Price Estimation" — a more
  accurate estimate shrinks the `|spot - p_ref|` gap the fee mechanism depends on).
- **Directional classification.** Each quote is classified by whether it would push `spot`
  further from `p_ref` (presumptively informed — `+arb_k*dev^2` surcharge) or toward it
  (presumptively uninformed — a `-counter_k*(...)*dev^2` rebate, risk-gated by
  `tox_hat`/`shock_hat`).
- **Fee ratchet.** `after_swap` accumulates a `fee_floor` from realized
  volatility/toxicity/shock/size targets, decaying between steps; `compute_swap` reads it
  per-quote alongside the directional term above.
- **Profile ensemble.** Three fixed parameter tuples (`P0` balanced, `P1` max-defense, `P2`
  loss-minimizer) are scored on every executed trade via a counterfactual-edge EMA (with a
  route bonus for counter-direction flow), and the active profile switches under hysteresis
  (3-step cooldown, asymmetric gaps).

## Port target

This is a direct-submission-shaped Rust source targeting the exact same `pinocchio`
ABI/instruction layout this repository's own `crates/shared/src/instruction.rs` and
`crates/submission-sdk` already implement — confirmed byte-identical between the two repos
at the pinned commit (docs/DESIGN.md §6.2/WHI-1236's own verification). Unlike `007`
(a Solidity library requiring a from-scratch derivation port), this port target is
**the whole file**, ported near-verbatim: the only intentional departures are (1) the
`NAME`/`MODEL_USED`/`no-entrypoint` cfg gate this repository's own harness convention
requires (docs/DESIGN.md §2.9's explicitly-permitted minimum changes), and (2) hoisting the
four searched constants into this repository's own `// === PARAMS BEGIN/END ===` block —
see `strategies/008-lagging-vwap-fee/NOTES.md` § Fidelity self-assessment for the itemised,
verified-by-diff list of every departure from the source's own text.

## Known parameters

| Name | Value | Meaning |
| --- | --- | --- |
| `ARB_K_BPS` | 5,200 (source's own P0/balanced-profile value) | directional surcharge coefficient on `dev^2` for arb-direction flow |
| `COUNTER_K_BPS` | 1,000 (source's own P0 value) | rebate coefficient on `dev^2` for counter-direction flow |
| `TARGET_BASE_BPS` | 16 (source's own value, shared by all three profiles) | base fee-ratchet target |
| `SIZE_K_BPS` | 2,900 (source's own P0 value) | size-penalty curvature coefficient |

Every other constant in `strategy.rs` (profiles 1/2 in full, tox/shock read weights, decay
rates, Kalman gain bounds, switcher hysteresis thresholds) is frozen at the source's own
value — see `strategies/008-lagging-vwap-fee/NOTES.md` § Parameter space for why these four
and not others, and why every bound here is a *source* anchor (the source author's own P1/P2
values), not an invented one, despite being measured on that author's stale harness vintage.

## Fidelity note

Per docs/DESIGN.md §2.9, this is filed as a **direct source-available port**, the same
category as `004`/`005` (not `007`'s derivation port): the source is already a `pinocchio`
submission targeting this exact ABI, so "faithful" means "near-byte-identical," not
"re-derived." `strategies/008-lagging-vwap-fee/NOTES.md` § Fidelity self-assessment carries
the machine-verified normalized diff against `strategy.rs` in this directory, itemising
every token-level departure.
