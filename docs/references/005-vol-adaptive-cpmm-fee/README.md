# 005 — Vol-Adaptive CPMM Fee (EdgeMax CumVar)

| Field | Value |
| --- | --- |
| **Name** | Vol-Adaptive CPMM Fee (`EdgeMax CumVar`, per its own `NAME` constant) |
| **Source form** | source (Rust, direct Solana submission — same shape as this challenge's own `lib.rs`) |
| **Original material** | `EdgeMax_CumVar.rs`, `upstream-README.md`, `eval.sh` in this directory (unmodified) |
| **Known parameters** | see below |

## Original material location

Repository: <https://github.com/dcccrypto/percolator-perp-liquidity>. The user-supplied
link (`tree/main/amm`) points at a path that **no longer exists on `main`** — a later
commit (`42f243ca36ef500712076a236b3505a3814e02f2`, message: "Consolidate into a clean
design+proofs+graphs repo (no engine internals) ... Remove ... the unrelated prop-amm
submission") deliberately deleted the `amm/` directory as unrelated to that repo's current
focus (a perpetuals funding/margin/insurance model under `model/` — not this strategy, and
not fetched here).

The material below is pinned at the **parent** commit, before that removal:
`0c7170244d695a681f3a4393e20079dfb7dc137b`. The repo is Apache-2.0 licensed at that
commit. Because the file no longer exists on any branch, this pinned copy is the only
durable record of it — treat this directory as the sole surviving source.

| File here | Upstream path (at the pinned commit) |
| --- | --- |
| `EdgeMax_CumVar.rs` | `amm/EdgeMax_CumVar.rs` |
| `upstream-README.md` | `amm/README.md` |
| `eval.sh` | `amm/eval.sh` |

## Mechanism

Confirmed directly from the source's own extensive header comment and constants — this is
unusually well self-documented:

- **Estimator**: cumulative mean (MLE), not EWMA — because the per-simulation GBM
  volatility `sigma` is *stationary* for the whole simulation (this challenge's own
  `crates/shared/src/config.rs` samples one `gbm_sigma` per simulation, held fixed for all
  10,000 steps), the maximum-likelihood variance estimate is the plain average of per-step
  squared relative price moves — and its error shrinks to 0 as steps accumulate, unlike a
  fixed-alpha EWMA's irreducible noise floor. The file's own comment argues this
  explicitly and states it is "strictly worse" for this specific setting.
- **Sampling point**: one relative-move sample per step, taken on the first executed trade
  of a new step (right after the arbitrageur has already corrected the reserve-implied
  price toward fair) — deliberately timed to approximate a clean per-step fair-price
  sample without needing L2-level price-path observability.
- **Fee formula** (integer, bps):
  `fee_bps = clamp(FEE_LO + A_NUM*sigma_hat/A_DEN + sigma_hat^2/B_DEN, FEE_LO, FEE_HI)`,
  i.e. `fee_bps = clamp(20 + 0.7*sigma_hat + sigma_hat^2/160, 20, 130)` — matches the
  user-supplied description exactly.
- **Shape safety**: the fee function reads only reserves + storage, never the swap's own
  input amount, so each side is a pure fee-discounted constant-product curve — provably
  monotone and concave by construction, per the file's own comment. Relevant to this
  repo's concavity panic (`crates/sim/src/curve_checks.rs`, `docs/DESIGN.md` §2.9).
- **Native/BPF parity**: the file's own comment states its BPF tag-2 path decodes storage
  and re-runs the same `after_swap` so native and BPF fee trajectories are identical — the
  same parity property this repo's own `docs/DESIGN.md` §2.6 gate checks for.

## Known parameters

| Name | Value | Meaning |
| --- | --- | --- |
| `FEE_LO` | `20` bps | fee floor (LVR floor — lower leaks value to the arbitrageur) |
| `FEE_HI` | `130` bps | fee ceiling (rarely binding, per the source's own comment) |
| `A_NUM` / `A_DEN` | `7` / `10` | linear term coefficient, `0.7 * sigma_hat` |
| `B_DEN` | `160` | quadratic term divisor, `sigma_hat^2 / 160` |
| `COLD_FEE` | `55` bps | default fee before the estimator warms up |
| `WARMUP_STEPS` | `16` | per-step samples required before trusting `sigma_hat` |
| `MOVE_BPS_CAP` | `250` bps | cap on a single per-step relative move before squaring |
| `R2_CAP` | `62_500` (`= 250^2`) | corresponding cap on the squared move |

Per `docs/DESIGN.md` §2.4, these are this source's own tuned defaults (for a market this
challenge's own reserve/fee ranges may or may not resemble) — they seed, but do not fix,
the porting issue's frozen parameter space.

## Fidelity note

This is the highest-fidelity source in this freeze: it is already written against
`pinocchio`/`prop_amm_submission_sdk` in the exact single-file shape this challenge
grades, and its header comment already reasons in this challenge's own terms (LVR vs.
retail-spread capture, per-sim stationary `gbm_sigma`, native/BPF parity). Porting risk is
low; the main open question for the porting issue is whether its own reserve/storage byte
layout lines up with `crates/submission-sdk`'s current ABI or needs adjusting.
