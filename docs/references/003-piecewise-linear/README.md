# 003 — Piecewise Linear

| Field | Value |
| --- | --- |
| **Name** | Piecewise Linear |
| **Source form** | source (Rust, Solana/pinocchio-shaped) + prose (blog) |
| **Original material** | this directory's `.rs` files (unmodified) + `upstream-README.md` + the blog excerpts below |
| **Known parameters** | see below |

## Original material location

Repository: <https://github.com/benedictbrady/prop-amm>, pinned at commit
`47dd714c50c57e9da8f433f71ecc5b7c8a1c7c9a`. No repo-level license is declared (GitHub's
repository API reports none at this commit). Blog writeup:
<https://www.benedict.dev/prop-amm>, published 2026-01-08 — fetched and committed here as
`blog-benedict-dev-prop-amm.html` (byte-exact page snapshot) and
`blog-benedict-dev-prop-amm.md` (verbatim excerpts transcribed from that snapshot's
server-rendered content, quoted rather than paraphrased).

Files copied byte-for-byte from `programs/prop-amm/src/` at that commit:

| File here | Upstream path |
| --- | --- |
| `lib.rs` | `programs/prop-amm/src/lib.rs` |
| `state.rs` | `programs/prop-amm/src/state.rs` |
| `math/mod.rs` | `programs/prop-amm/src/math/mod.rs` |
| `math/piecewise.rs` | `programs/prop-amm/src/math/piecewise.rs` |
| `math/scaled.rs` | `programs/prop-amm/src/math/scaled.rs` |
| `math/sqrt.rs` | `programs/prop-amm/src/math/sqrt.rs` |
| `instructions/swap.rs` | `programs/prop-amm/src/instructions/swap.rs` |
| `instructions/update_oracle.rs` | `programs/prop-amm/src/instructions/update_oracle.rs` |
| `upstream-README.md` | `README.md` (repo root) |

Note: this is `benedictbrady/prop-amm` — the challenge author's own reference
implementation repo, **not** `benedictbrady/prop-amm-challenge` (this repo's read-only
`upstream` remote, the simulator/grader itself). The two are separate repos by the same
author; only the simulator one is off-limits per `AGENTS.md`'s upstream-sync lane. This
strategy's material is fair game to port, same as any other collected strategy.

## Mechanism

Confirmed from source (`state.rs`, `math/piecewise.rs`):

- **`NUM_PRICE_POINTS = 7`, `NUM_SEGMENTS = 6`** per side of the book (`PiecewiseBookSide`)
  — a 6-segment piecewise-linear price curve per side, not a single linear range.
- **Liquidity per segment is uniform**: `liquidity_per_price_unit = quantity / (upper -
  lower)` within each segment (matches the user-supplied description).
- **Exact-in / exact-out** swap amounts are computed via a closed form per segment
  (`math/piecewise.rs`, `math/sqrt.rs` — integer sqrt helpers back a square-root term in
  the per-segment fill calculation).
- **Self-replenishing**: `instructions/swap.rs`'s "heal consumed" step explicitly moves
  liquidity consumed on one side back toward replenishing the spread — matches the
  user-supplied description and the blog's "self-replenishing" framing.
- **Oracle updates only move price points** (`instructions/update_oracle.rs`); segment
  *quantities* are pool state, not oracle-supplied — the oracle is a price reference, not a
  liquidity source.

## Known parameters

| Name | Constraint | Source |
| --- | --- | --- |
| `NUM_PRICE_POINTS` / `NUM_SEGMENTS` | `7` / `6`, compile-time constants | `state.rs:18,21` |
| `SCALE` | `1_000_000_000` (fixed-point scale) | `state.rs:15` |
| per-segment liquidity | derived (`quantity / (upper - lower)`), not a free parameter | `math/piecewise.rs` |

The 7 price points themselves (per side) are the actual state the oracle writes and the
curve is shaped by — not a fixed parameter set. The porting issue's frozen search space
(`docs/DESIGN.md` §2.4) will need to define what varies (e.g. initial point spacing/shape,
segment count if made tunable) rather than searching literal price levels, since those are
oracle/state-driven here, not authored constants.

## Fidelity note — one detail is prose-only and unconfirmed in source

The user-supplied description also mentions "oracle staleness backoff (时间越久 spread
指数扩大)" — spreads widening (exponentially, as staleness grows) the longer it's been
since the last oracle update. **This mechanism was not found in the fetched source** at
`programs/prop-amm/src/` (`instructions/swap.rs` and `instructions/update_oracle.rs` were
both grepped for staleness/backoff/decay logic — none present at this commit).

The blog post confirms the *idea* is real but was an **exploratory mock-up**, not the
deployed mechanism — quoting `blog-benedict-dev-prop-amm.md` verbatim: it is one of three
"axes of improvement" (the other two: inventory skew, depth placement) the author asked
Claude to "mock up," and the author's own verdict is that "they are not terrible but they
are not above the level of a competent market maker." The post gives only a qualitative
chart (spread % vs. seconds since the last oracle update; widening begins at 30s; quotes
are pulled entirely at 45s) and **no formula** — per `docs/DESIGN.md` §2.9, "Provenance is
mandatory. Prose-only strategies may be second-hand or simply wrong": this detail is
exactly that. The porting issue should treat the 7-point/6-segment self-replenishing curve
(confirmed both in source and in the blog's own account of what was actually shipped) as
the strategy to port faithfully, and treat staleness backoff as, at most, an explicit
`003b`-style variant (§2.9) if pursued at all — not part of the faithful port.
