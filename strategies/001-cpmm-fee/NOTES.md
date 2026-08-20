# 001-cpmm-fee

## Provenance

Source form: **inherited, not ported.** This is a copy of `programs/starter/src/lib.rs` — the
challenge's own shipped starter, a constant-product AMM (`x*y=k`) with a flat fee. No external
material, no fidelity risk: the mechanism is already exactly what upstream ships and simulates.

What actually differs from the starter: `NAME` (identifies this strategy in the registry),
the `after_swap` no-op arm's comment, and an added doc comment on `compute_swap`. The
`compute_swap` mechanism itself — and `MODEL_USED`, describing who produced that mechanism —
is untouched.

## Mechanism

Constant-product swap with a single free parameter, the fee, expressed in basis points out of
10,000 as `FEE_BPS` (WHI-1194 rewrote the original `FEE_NUMERATOR / FEE_DENOMINATOR` = 950/1000
into this convention — bit-for-bit equivalent for every input, since scaling a truncating
integer division's numerator and denominator by the same factor never changes its floor;
verified via `prop-amm run` reproducing the unchanged 210.50 starter anchor at `FEE_BPS = 500`
before any search ran). `FEE_BPS` lives in the `// === PARAMS BEGIN/END ===` block
(`tools/bench/src/params.rs`) so `bench fit` can rewrite it during a search. This is the
**0-line** (`docs/DESIGN.md` §2.8): every other strategy is measured against the best
fixed-fee CPMM, and a strategy that doesn't beat it is a negative result, not a mid-table
entry. `001` also doubles as bench's own self-check — its fee-edge response must be
single-peaked, or the bug is in bench, not in a discovery.

## Frozen parameter space (declared before any search runs — docs/DESIGN.md §2.4)

**fee_bps ∈ [1, 500], integer.**

Rationale, so WHI-1194 can search inside it or challenge it before spending the budget:

- **Upper bound 500bps** — the starter's own shipped value (`FEE_NUMERATOR/FEE_DENOMINATOR` =
  950/1000 = 5% = 500bps). It's the parity anchor (§2.1) and DESIGN.md §2.8 explicitly flags it
  as "almost certainly not the family optimum," so the space must include it rather than start
  below it.
- **Lower bound 1bps** — the practical non-zero floor. 0bps removes the fee mechanism's whole
  differentiation (identical to `normalizer-as-submission`, §2.8's other baseline), so it isn't a
  meaningful point in *this* family's space.
- The competitor (normalizer) samples `norm_fee_bps ∈ U[30, 80]` (`crates/shared/src/config.rs`);
  500bps brackets that range with headroom on both sides for a coarse-grid-then-descent search
  (§2.5) to actually locate an interior optimum rather than hit a boundary.

This is a WHI-1193 (M0) decision, not a §6.2 owner-input item — §6.2 is the *frozen strategy
list* for M1's ported strategies, and `001` is the M0 baseline that measures all of them.

## Fitted point (WHI-1194)

Run via `cargo run -p prop-amm-bench -- fit --strategy strategies/001-cpmm-fee`
(`config/bench.toml`'s `[search]` budget, 300 points; screening segment
`1_000_000..=1_000_199`). Coarse grid (150 evenly-spaced points across `1..=500`) then coordinate descent converged
after **167 of 300** evaluation points total — the search never approached its budget cap. Full curve
(param → screening avg edge) is committed at `results/2026-08-20-fit-001-cpmm-fee.md`; it is
**strictly increasing from `FEE_BPS = 1` to 66, then strictly decreasing to 500** — a clean
single peak, confirming the protocol self-check (§2.8) before this or any other strategy's
numbers are trusted.

**Winning point: `FEE_BPS = 66`** — well inside the competitor's own `norm_fee_bps ∈ U[30, 80]`
sampling range (`crates/shared/src/config.rs`), and far from the starter's 500bps, matching
this issue's own prediction.

| Segment | n | Avg edge |
| --- | --- | --- |
| screening (search inner loop) | 200 | 384.82 |
| train (final evaluation) | 1,000 | 406.14 |
| validation (final evaluation) | 1,000 | 401.80 |
| observation `0..=999` (reporting only, never a decision input) | 1,000 | 399.97 |

The observation-segment number is what's leaderboard-comparable against the **210.50**
starter anchor (§2.1) — `FEE_BPS = 66` very nearly doubles it.

## Parity gate (docs/DESIGN.md §2.6)

Reproduced via `cargo run -p prop-amm-bench -- parity --strategy strategies/001-cpmm-fee`
against the committed `FEE_BPS = 66` point: `prop-amm validate` passes; the fast path and the
reference path agree on **all 1,000 `observation`-segment seeds to `0` relative difference**
(well inside the `1e-9` gate); the fast-path aggregate (avg edge 399.97) matches
`prop-amm run`'s own 2-decimal output exactly. See
`results/2026-08-20-parity-001-cpmm-fee.md` for the committed snapshot.
