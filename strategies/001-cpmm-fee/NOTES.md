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

Constant-product swap with a single free parameter, the fee, expressed as
`FEE_NUMERATOR / FEE_DENOMINATOR`. This is the **0-line** (`docs/DESIGN.md` §2.8): every other
strategy is measured against the best fixed-fee CPMM, and a strategy that doesn't beat it is a
negative result, not a mid-table entry. `001` also doubles as bench's own self-check — its
fee-edge response must be single-peaked, or the bug is in bench, not in a discovery.

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

## Fitted point

**Not fit here.** Committed as-is at the starter's own value (fee = 500bps) per this issue's
explicit scope ("Do not fit it here"). WHI-1194 runs the search (coarse grid → coordinate
descent, ≤300 points, screening seeds) and updates this section with the winning point plus its
train/validation numbers.

## Parity gate (docs/DESIGN.md §2.6)

Reproduced via `cargo run -p prop-amm-bench -- anchor --file strategies/001-cpmm-fee/lib.rs` — identical
mechanism to the starter, so this should reproduce the same **avg edge 210.50** anchor on the
`observation` segment. See `results/` for the committed snapshot.
