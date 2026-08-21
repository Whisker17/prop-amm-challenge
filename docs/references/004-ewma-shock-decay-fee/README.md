# 004 — EWMA Dynamic Fee + Shock-Decay

| Field | Value |
| --- | --- |
| **Name** | EWMA Dynamic Fee + Shock-Decay |
| **Source form** | source (Rust, direct Solana submission) + Solidity (richer EVM port) + source (v3 Rust extension) |
| **Original material** | this directory's files (unmodified) |
| **Known parameters** | see below |

## Original material location

Repository: <https://github.com/lilaclilac09/pamm-a>, pinned at commit
`b2899305f8d91cf0df03858ca6515682493bece5`. No repo-level license is declared (GitHub's
repository API reports none at this commit). This is a research/experimentation monorepo
("Research and implementation sandbox for prop AMM, market making, and simulation on
Solana") with several unrelated sub-projects; only the files below pertain to this
strategy.

| File here | Upstream path | What it is |
| --- | --- | --- |
| `v2-solana-lib.rs` | `src/lib.rs` | **The actual competition submission** — repo's own README calls it out directly: "Competition entry — EWMA Dynamic Fee v2 (prop-amm-challenge submission)". Already in this challenge's exact single-file `compute_swap`/`after_swap` shape. |
| `v2-ethereum-Strategy.sol` | `references/ethereum/Strategy.sol` | Repo's own "Solidity port of the strategy, for EVM comparison" — a **richer** variant (adds momentum + inventory-skew terms on top of the base EWMA-vol/shock-decay mechanism). |
| `v3-flow-aware-strategy.rs` | `flow-aware-ewma/strategy.rs` | A v3 extension of `v2-solana-lib.rs` adding an off-chain "flow pressure" signal (competitor-activity based). Repo's own README: "not the competition submission — `src/` stays locked." |
| `v3-flow-aware-README.md` | `flow-aware-ewma/README.md` | Describes the v3 signal and its wire-up. |
| `upstream-README.md` | `README.md` (repo root) | Top-level map of the monorepo, for provenance context. |

**Not copied**, and not part of this strategy: `pinocchio-prop-amm/` (a separate, unrelated
"PMM" — proactive market maker, DODO-style — AMM in the same monorepo; its `swap`
instruction doc explicitly says "AMM swap using PMM pricing formula", a different
mechanism entirely) and the various bot/visualization directories.

## Mechanism

Confirmed from `v2-solana-lib.rs` (the actual submission, most direct source):

- **Storage layout** (1024-byte submission storage, first 32 bytes used):
  `[0..8] ewma_vol`, `[8..16] last_rx`, `[16..24] last_ry`, `[24..32] shock_steps`.
- **Fee** = `BASE + vol_fee + shock_fee`, capped at 100 bps.
  - `vol_fee = ewma_vol * VOL_MULT`.
  - `shock_fee = shock_steps * SHOCK_FEE_PER_STEP` (max when a shock just fired, decays
    each subsequent trade — `shock_steps` counts down toward 0 in `after_swap`).
- **`after_swap`** updates the vol EWMA (`α = 0.20`, ~15-step convergence to steady state)
  and re-arms `shock_steps` to its max whenever the price move since the last trade exceeds
  `SHOCK_THRESHOLD_1E9` (0.5%), otherwise decrements it by 1.

`v2-ethereum-Strategy.sol` (also labelled "v2", independently, on the EVM side) adds on top
of the same EWMA-vol core: a **momentum EWMA** (buy/sell pressure, faster-decaying α=0.20)
that adds a symmetric surcharge when sustained directional flow is detected, and an
**inventory-skew** term that shifts bid/ask fees apart based on reserve ratio. Whether the
porting issue treats this as within-scope for "004" or as a `004b` variant (§2.9) is a call
for that issue — the two "v2"s are not identical despite the shared label.

`v3-flow-aware-strategy.rs` adds a `flow_pressure_1e9` term (from an **off-chain** signal
with no analogue inside this simulator — it's derived from watching competing Solana AMMs'
own on-chain pool activity) capped at 12 bps. It has no in-simulator inputs to derive from,
so it is very unlikely to be portable at all; noted for completeness, not as an expected
M1 candidate.

## Known parameters (from `v2-solana-lib.rs`, the base version)

| Name | Value | Meaning |
| --- | --- | --- |
| `SHOCK_THRESHOLD_1E9` | `5_000_000` (0.5%) | price-move threshold that (re-)arms the shock fee |
| vol EWMA α | `0.20` | convergence ~15 steps |
| fee cap | `100 bps` | hard ceiling on `BASE + vol_fee + shock_fee` |
| `VOL_MULT`, `SHOCK_FEE_PER_STEP`, `SHOCK_DECAY_STEPS`, `BASE` | see `v2-solana-lib.rs`'s own doc comment (lines 22–31) for the worked examples (calm ≈28bps, post-shock ≈46bps for 8 steps, volatile ≈14bps) | exact numeric constants, copy verbatim |

These are this strategy's own defaults, tuned for a different market/venue — per
`docs/DESIGN.md` §2.4 they seed the porting issue's frozen search space, they are not
searched-over as-is.

## Fidelity note

`v2-solana-lib.rs` is already shaped almost exactly like this challenge's submission
interface (pinocchio, `NAME` constant, byte-storage `after_swap`), so it is the
**highest-fidelity, lowest-porting-risk** source in this freeze — closer to a straight
adaptation than a port. The Solidity v2 and Rust v3 variants are provided for context and
as candidate follow-on variants, not as the primary thing to port.
