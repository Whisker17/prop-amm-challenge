# ceilings/

The **ceiling lane** (WHI-1247): out-of-competition measurements of how much edge is on the
table when a candidate is granted something no submittable strategy can actually have — here,
a price re-anchor the arbitrageur cannot front-run (a host-side oracle quoting directly off the
replayed GBM fair-price path, `tools/bench/src/oracle.rs`). This is a *ceiling* probe, not a
strategy: it exists to tell M1/M2 how much of the remaining gap to the 0-line is even reachable
by better re-anchoring, before anyone spends search budget chasing it with a real, front-runnable
mechanism.

**Nothing under `ceilings/` is submittable or ranked.** Unlike `strategies/**`:

- there is no `lib.rs` here, ever — nothing here compiles to BPF or links against
  `crates/submission-sdk`;
- every report `bench ceiling` writes carries `out_of_competition: true` as its own literal
  first line, and every one of its tables is shaped differently from `compare.rs`'s ranked
  `| regime | n | mean diff | 95% CI |` table, so a ceiling report is never mistaken for a
  ranked comparison at a glance (`tools/bench/tests/ceiling_guards.rs` enforces the no-`lib.rs`
  half of this as a repo invariant, not just a review note);
- `bench ceiling` refuses to measure against anything but the allowlisted `000-normalizer`
  (for `--self-check`) or `001-cpmm-fee` (the 0-line) — it can never silently compare against a
  stronger, more recent strategy and be read as if it beat the actual 0-line;
- `bench ceiling` refuses the single-use `test` segment unconditionally, even with
  `--i-am-spending-the-test-segment` — nothing out-of-competition here has a ranking claim for
  that flag to protect.

This is explicitly **not an orphan** under `docs/DESIGN.md` §2.10: it is a deliberately
out-of-competition instrument, not an abandoned or forgotten strategy directory.

## Layout

One directory per ceiling probe: `<id>-<slug>/NOTES.md` — provenance, adaptations from the
ported source, what was deliberately not ported, the frozen self-chosen parameter space and
its rationale, and the measured numbers with full provenance (mirrors `strategies/README.md`'s
own per-entry contract, minus `lib.rs`).

| Id | Name | What it measures |
| --- | --- | --- |
| C-orbic-oracle | Orbic Oracle Re-anchor | A concentrated, spread-bearing Orbic-family curve quoting off a trade-triggered replay of the GBM fair-price path, vs. `001-cpmm-fee` (the 0-line) — `C-orbic-oracle/NOTES.md` |

## Running

```bash
# Loop-parity validation gate: proves the lane's own native batch loop (tools/bench/src/oracle.rs)
# reproduces `strategies/000-normalizer` bit-close through the trusted compile+run path.
# Writes no report.
cargo run -p prop-amm-bench -- ceiling --self-check --segment validation

# The headline measurement: fits (concentration, spread_bps) jointly on `screening`, then
# measures the anchored variant vs. the 0-line on `segment`.
cargo run -p prop-amm-bench -- ceiling --variant anchored --fit --segment validation

# The degenerate/diagnostic contrast: spread_bps re-fit alone, concentration held at the
# anchored variant's own fitted value.
cargo run -p prop-amm-bench -- ceiling --variant floating --fit --concentration <value> --segment validation

# A fixed point, no search, no report (fast iteration):
cargo run -p prop-amm-bench -- ceiling --variant anchored --concentration 10 --spread-bps 20 --no-report
```

See `C-orbic-oracle/NOTES.md` for the frozen parameter ranges, what was and wasn't ported, and
the committed numbers.
