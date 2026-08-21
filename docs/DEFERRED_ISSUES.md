# Deferred issues registry

A living log of issues that were **surfaced during review but consciously not fixed**
in the PR that found them. This is not a bug tracker for open work — it is the record of
*known, accepted debt*: things we decided to defer, so a future change touching the same
area starts from knowledge instead of rediscovery.

## How to use this file

- **Add an entry** whenever a review turns up a real issue that a PR deliberately leaves
  unfixed (scope, risk, or priority). Record it here in the same PR that defers it.
- **Reference it** before working on the affected area — check whether the thing you are
  about to "discover" is already logged, and whether a listed fix is now in scope.
- **Close an entry** by moving it to *Resolved* (bottom) with the PR/commit that fixed
  it, rather than deleting — the history is useful.
- Keep entries short. Link the originating tracker issue / PR and the code symbol so the
  entry stays findable as the code moves.

Severity is the reviewer's judgement at defer time: **High** (correctness/safety, fix
soon — anything touching a declared high-risk path defaults to at least High), **Medium**
(operational/perf, fix when convenient), **Low** (nit/consistency).

## Entry format

```markdown
- **<one-line description of the defect>** (<Severity>, <issue-id>).
  `<file>::<symbol>` — what's wrong, why it was deferred, and what the fix would be.
```

---

## Open

- **`bench`'s parity checks are bounded to 2-decimal-place agreement with `prop-amm run`, not
  the "1e-9 relative" / "exactly" the WHI-1193 acceptance criteria state** (Low, WHI-1193).
  `crates/cli/src/output.rs:31-32` only ever prints edge at 2dp — there is no higher-precision
  output to cross-check bench's own full-precision numbers against, and editing that
  upstream-owned file is out of bounds. `tools/bench/src/commands/anchor.rs::edges_agree`
  rounds both sides to 2dp before comparing, and says so in its own doc comment and in the
  committed `results/*.md` report text. Accepted because both bench's aggregate-mode and
  per-seed paths call the exact same `HyperparameterVariance::apply`/`run_batch_native`
  functions `prop-amm run` calls internally with identical seed derivation (docs/DESIGN.md
  §2.2, §4.3), so full-precision agreement is structurally guaranteed by shared code, not
  merely hoped for — but that guarantee is by code inspection, not by a runtime assertion at
  1e-9. Fix: none available without either editing `crates/cli/src/output.rs` (upstream) or
  bench importing `prop_amm_sim`/`prop_amm_shared` types to reconstruct the CLI's own
  in-process value directly instead of parsing its stdout — worth reconsidering if a future
  strategy's ranking ever turns on a margin finer than a cent.
- **Root `Cargo.toml` now carries a one-line diff against upstream** (Low, WHI-1193).
  `Cargo.toml::[workspace].members` gained `"tools/bench"`. Accepted per the issue's own
  instruction: `tools/bench`'s own dependencies (`serde`, `toml`) are pinned inside
  `tools/bench/Cargo.toml` rather than added to `[workspace.dependencies]`, so this member-list
  line is the only upstream-owned-file cost of the whole measurement layer. Fix: none needed
  unless a future upstream sync itself touches the `members` list, in which case re-add this
  line during that merge.
- **`cargo clippy -- -D warnings` fails on inherited upstream code** (Low, template
  bootstrap). `crates/shared/src/instruction.rs:8` (`empty line after doc comment`) and
  `crates/shared/src/normalizer.rs:36,41` (`manually reimplementing div_ceil`, twice) —
  3 lints, all in upstream-owned files. Deferred because fixing them inside any of our PRs
  guarantees a conflict at the next `upstream-sync`, and the lints are cosmetic. Consequence:
  the merge gate is `cargo test --workspace` plus *no new* clippy warnings in touched files,
  **not** a clean `-D warnings` run (`AGENTS.md` § Build, test, run). Fix: send them upstream,
  or clean them in a dedicated `upstream-sync`-adjacent chore once upstream stops moving.
- **`cargo fmt --all -- --check` fails on inherited upstream code** (Low, template
  bootstrap). `crates/shared/src/{config,normalizer}.rs`,
  `crates/sim/src/{arbitrageur,curve_checks,engine}.rs`, `crates/sim/tests/integration.rs` —
  6 files drift from rustfmt. Same reasoning as above: a repo-wide `cargo fmt --all` would
  touch upstream files in every diff and make every future upstream merge conflict on
  formatting. Consequence: run `cargo fmt` on **the files you touched**, never `--all`.
- **A temporary `release/v*` cut is only distinguishable from a live integration branch by
  its open PR into `main`** (Medium, inherited from the template). `docs/GIT_WORKFLOW.md`
  § Fan-out (the live-branch query) — between pushing a fresh cut and opening its PR, the
  query classifies the cut as "live", so a concurrent fan-out merges unreleased `dev` into a
  branch queued for production. Deferred because the only real fix changes the branch-naming
  contract (a distinct prefix for temporary cuts, or pushing only after the PR exists); the
  workflow mitigates it with prose only ("open the PR immediately after the push",
  § Releasing to `main`). Low likelihood in a single-operator repo.
- **`telemetry.rs`'s recorder-dispatch mechanism duplicates `compile.rs`'s `Slot`/
  `LOADED_AFTER_SWAP` shape** (Low, WHI-1195). `tools/bench/src/telemetry.rs::AmmSlot` /
  `REAL_AFTER_SWAP` / `record_and_delegate` reproduce `compile.rs`'s `Slot` /
  `LOADED_AFTER_SWAP` / `call_after_swap` mechanism (static `AtomicPtr` array + a
  `#[repr(usize)]` enum + transmute-and-call) nearly line for line. Deferred because the two
  serve genuinely different jobs despite the shared shape: `compile.rs`'s dispatches between
  two *candidate identities* (`compare`'s two build slots), while `telemetry.rs`'s dispatches
  submission-vs-normalizer and additionally has to fall back to a no-op when no real
  `after_swap` was installed — a case `compile.rs` doesn't have. A shared generic wrapper
  around "install a `fn`-pointer behind an `AtomicPtr`, dispatch through a transmuting
  trampoline" would need its own abstraction over that difference, for a net gain of maybe a
  dozen lines. Fix: revisit if a third call site needs the same shape — two duplicates is a
  pattern worth naming, three is worth extracting.
- **`AGENTS.md`'s Status section still describes §6.2 as owner-input-blocked, present
  tense** (Low, WHI-1197). `AGENTS.md:36-38` (**Not written yet**) reads "the *frozen
  strategy list* M1 iterates over is an owner input that has not been supplied yet,
  tracked as WHI-1197. Do not pick up an M1 issue until that list is frozen" — this PR
  (WHI-1197) supplied that list, filed the material under `docs/references/`, froze
  `docs/DESIGN.md` §6.2, and opened the five M1 issues (`WHI-1206`–`WHI-1210`) it names,
  so the section is now stale the moment this PR lands. `AGENTS.md:34` ("§1–§5, §7, and
  §8 are written") is stale the same way — §6 is now written too. Left unfixed here
  because `AGENTS.md` is a repo-wide governance carve-out path (`docs/GIT_WORKFLOW.md` §
  Repo-wide governance carve-out) and this PR's diff is version-scoped, not
  carve-out-only — touching it here would mix scopes, exactly the case `AGENTS.md` § Git
  workflow itself calls out ("A mixed PR ... must be split"). Fix: a follow-up
  governance-scoped PR (carve-out paths only, base `dev`) updates `AGENTS.md:30-38` to
  record §6 as fully written and the freeze + M0 as landed, past tense — same shape as
  WHI-1200's fix for the analogous `docs/agents/issue-tracker.md` staleness (see the
  WHI-1199 entry under *Resolved*).
- **`commands/l1.rs` bakes `strategies/000-normalizer/lib.rs` into generic measurement
  infrastructure's `--file` default** (Low, WHI-1195). `tools/bench/src/commands/l1.rs::
  DEFAULT_NORMALIZER_AS_SUBMISSION` — a specific strategy id is now a default in a command
  that should otherwise work on any submission file. Deferred (not really disputed, just not
  changed): `commands/anchor.rs::DEFAULT_FILE` already hardcodes `programs/starter/src/lib.rs`
  the same way, and the report-collision constraint (`report.rs` allows one report per
  `(day, stage)`; the ticket wants "a `results/` snapshot covering the starter and the
  normalizer reference" as one artifact) is what forces `l1` to default to a *list* rather
  than a single required path. Fix: if a third baseline ever needs the same treatment,
  consider moving the default file list into `config/bench.toml` instead of a second hardcoded
  constant.
- **`tools/bench/src/curve_checks.rs`'s mirror of `crates/sim/src/curve_checks.rs::
  submission_shape_violation` has no automated drift-detection control** (Medium, WHI-1212).
  `docs/DESIGN.md` §4.3 permits duplicating upstream logic only alongside a control — for
  `fast_compile.rs`'s own duplicated `compile.rs` logic, that control is `bench parity`'s
  per-seed output-agreement check, which would fail if the two paths' compiled *behavior*
  ever diverged. `curve_checks.rs`'s mirror has no equivalent: it is a pure math function
  used only by `bench fuzz`'s own gate, so nothing else in the system would fail if upstream
  changed `submission_shape_violation`'s constants or logic and this copy silently didn't
  follow. Mitigated, not closed, by porting one of upstream's regression tests
  (`exposes_false_positive_from_cancellation_prone_concave_curve`) verbatim into this
  mirror's own test module — upstream's `accepts_extensive_{analytic,piecewise}_*_matrix`
  and normalizer-curve/runtime-path tests were not ported, since they need `rand_pcg`/
  `rand_distr`/`BpfAmm`/`engine::run_simulation_native` machinery `tools/bench` doesn't
  otherwise depend on. Fix: re-diff this file against `crates/sim/src/curve_checks.rs` by
  hand at every upstream sync that touches it (the header comment says so); revisit if a
  cheap way to assert the two copies are byte-identical (e.g. a build script diffing both
  files) is ever worth the coupling.
- **`bench fuzz`'s golden-section-shaped sampling is a faithful-shape mirror, not a literal
  port of `crates/sim/src/arbitrageur.rs`'s bracket-then-golden-section search or
  `crates/sim/src/router.rs`'s alpha-split objective** (Low, WHI-1212). `tools/bench/src/
  fuzz.rs::golden_section_sample` runs one generic golden-section maximization of a
  profit-like objective directly over `[MIN_INPUT, MAX_INPUT_AMOUNT]` against several
  synthetic fair-price multipliers; it reproduces neither `arbitrageur.rs`'s doubling-growth
  `bracket_maximum` preamble nor `router.rs`'s search over an alpha *split fraction* of a
  fixed total order size. This was a disclosed, approved design trade-off (see the plan this
  issue's implementation was reviewed against) made to avoid coupling `tools/bench` to two
  more private search algorithms for a fuzz gate whose job is triggering the same
  *check function*, not reproducing the exact economics that would lead a real arbitrageur
  or router there. Fix: if a future violation is found at runtime that this gate's sampling
  shape would not have reached, port the missing algorithm shape (bracket preamble, or an
  alpha-split objective) as its own additional sample-set kind alongside the dense sweep and
  the current golden-section one.
- **A second, same-day, *distinct* `bench fuzz` violation on the same strategy produces no
  committed evidence for that second violation** (Low, WHI-1212). `tools/bench/src/
  commands/fuzz.rs::run` — `report.rs`'s one-report-per-`(day, stage)` rule (the same rule
  that motivated dropping the PASS-path report, see this PR's own commit history) means a
  violation found after an earlier same-day violation on the same strategy already claimed
  today's `results/<date>-fuzz-<slug>.md` slot just prints "(violation evidence not
  written: ... already exists)" instead of committing anything for the second one. Accepted
  because the gate's own point is to run repeatedly and cheaply before every search — a
  strategy author debugging a violation is expected to fix it and re-run, not accumulate
  several same-day violations that all need separate evidence — and this exact
  one-report-per-day tradeoff is already an accepted, precedented limitation of `report.rs`
  (see the "A committed `compare` report with a regime-slice table needed a non-standard
  filename" entry in *Resolved* below — WHI-1215 resolved its filename symptom, not this
  underlying one-report-per-day tradeoff, which both entries accept). Fix: none needed
  unless a workflow emerges that genuinely needs multiple same-day violation reports for one
  strategy; if so, a `--report-suffix` flag (or a timestamp in the filename) would resolve
  it the same way that entry's manual rename did.
- **`bench anchor` still has a constant `STAGE` despite taking a per-target `--file`
  argument** (Low, WHI-1215). `tools/bench/src/commands/anchor.rs::STAGE`/`DEFAULT_FILE` —
  the same shape WHI-1215 fixed in `grid`/`l1`/`compare`, but WHI-1215's own "Verified
  state" section only named `grid.rs:98`, `l1.rs:183`, `compare.rs:99` as colliding, not
  `anchor.rs`; out of that issue's stated scope, so left unfixed here rather than expanding
  it. Lower risk in practice than the three fixed here: `anchor`'s whole point (docs/DESIGN.md
  §2.6) is cross-checking bench's own numbers against `prop-amm run` for one file at a time,
  and `DEFAULT_FILE` (the starter) is rarely overridden the way `grid`/`compare`'s
  `--candidate`/`l1`'s primary `--file` are expected to vary per strategy — but two
  same-day `anchor` runs against two different `--file` values would still collide exactly
  the way `grid`/`l1`/`compare` did before this issue. Fix: the same treatment —
  `slug_from_source_path(&args.file)` to derive the stage, then the same
  `report::ensure_report_slot_free` call `grid`/`l1`/`compare` each make directly — if this
  is ever hit in practice.
- **`bench l1`'s stage still collides across two runs that share only their primary
  (first) `--file`** (Low, WHI-1215). `tools/bench/src/commands/l1.rs::run` — the issue's
  own implementation note offered two choices for `l1` ("derive from the primary target or
  accept an explicit suffix"); this PR took the first, so `--file a --file b` and
  `--file a --file c` on the same day still collide on `l1-<a-slug>`. Accepted: the primary
  file is what varies day to day across strategies in the scenario the issue describes,
  and this mirrors the same short-slug tradeoff `resolve_strategy_lib_path` already accepts
  for `fit`/`parity`/`fuzz` (two differently-located strategy directories sharing a final
  path component would collide there too — not a new limitation this issue introduces).
  Fix: an explicit `--stage-suffix` (the issue's second, declined option) if this is ever
  hit in practice.
- **`slug_from_source_path` doesn't sanitize its output, so an unusual path component
  lands verbatim in a committed `results/` filename** (Low, WHI-1215).
  `tools/bench/src/commands/mod.rs::slug_from_source_path` — a `--candidate` like
  `strategies/a b/lib.rs` (a space) or one containing another filename-hostile character
  produces a report path with that character in it. Not fixed here: every strategy
  directory that exists in this repo today (`strategies/000-normalizer`,
  `strategies/001-cpmm-fee`) already follows a fixed `NNN-kebab-case` naming convention
  with no such characters, so this is a theoretical gap against today's actual inputs, not
  an observed failure — adding sanitization for a shape no real strategy directory uses
  would be speculative. Fix: sanitize (e.g. replace non-`[A-Za-z0-9_-]` bytes) if a
  strategy or ad-hoc `.rs` file with such a name is ever actually used.

---

## Resolved

- **`docs/DESIGN.md` cites `WHI-` issue ids while `AGENTS.md`/`docs/agents/**` still name
  `.scratch/` as the tracker of record** (Low, WHI-1192). `docs/DESIGN.md` §6.1, §8 —
  inconsistent tracker naming across the repo, owned by the separate governance issue
  WHI-1196. Resolved by WHI-1196 (`dc973d1`), which rebound the live governance path
  (`AGENTS.md`, `docs/GIT_WORKFLOW.md`, `docs/agents/issue-tracker.md`, and friends) to
  Linear naming. Entry moved here by WHI-1199.
- **The `Done` state flip cannot ride its own PR** (Low, template bootstrap).
  `docs/agents/issue-tracker.md` § Issue lifecycle ↔ Git — this was deferred on the
  premise that the tracker is in-repo, so `State: Done` has to be committed onto the
  resolved base *after* the merge, a direct commit to a protected branch by construction.
  Resolved by WHI-1196 (`dc973d1`): the tracker is now Linear, so `State: Done` is a
  `linear.save_issue` call, not a git commit — the protected-branch-commit defect this
  entry named is gone. This closes only that narrow defect, not the broader trade-off it
  sat next to: `docs/agents/issue-tracker.md` § Decisions #2 records, as its own
  already-accepted debt, that Linear and git are now separate systems with no shared
  commit boundary and can drift — real and ongoing, mitigated by operational discipline
  only, not eliminated by this entry's closure. Entry moved here by WHI-1199.
- **`tools/bench`'s fast-path timing (`0.11–0.57 s`/point) has no measurement to cite**
  (Low, WHI-1192). `docs/DESIGN.md` §2.6 — the number described `tools/bench`'s search
  fast path, which didn't exist in the repo yet. Resolved by WHI-1194, which added the
  fast path and re-measured it (`strategies/001-cpmm-fee/NOTES.md`,
  `results/2026-08-20-fit-001-cpmm-fee.md`) — the original estimate was **not**
  reproduced: 160 warm compiles measured min=0.472s, mean=1.000s, max=1.311s in the
  measuring session's environment, re-confirmed under lower system load with no
  improvement, and attributed to that environment rather than the fast path's design
  (which never rebuilds `pinocchio`/`wincode`/`prop-amm-submission-sdk` after the
  directory's first use). §2.6 now cites the real figures instead of the estimate. This
  closes the "no measurement to cite" defect; it did not claim the `< 1s` figure a
  future WHI-1194 acceptance check named — that was a live, disclosed gap in
  `strategies/001-cpmm-fee/NOTES.md`, not a re-opening of this entry. WHI-1205 settled
  that gap: none of the four candidate structural causes it checked explained the 9x
  discrepancy, and a fresh bounded re-measurement on the same machine meets the `< 1s`
  target (see `strategies/001-cpmm-fee/NOTES.md` and `docs/DESIGN.md` §2.6 for the full
  accounting — WHI-1194's own session-specific numbers remain unexplained, not
  reproduced).
- **A committed `compare` report with a regime-slice table needed a non-standard filename**
  (Low, WHI-1195). `results/2026-08-20-compare-with-regime-slices.md` — `report.rs`'s
  one-report-per-`(day, stage)` rule meant `2026-08-20-compare.md`'s slot, already spent by
  WHI-1193's own compare run, forced a hand-named file for the second same-day `compare`
  report. Resolved by WHI-1215 (PR #14): `compare`'s stage is now
  `compare-<candidate-slug>-vs-<reference-slug>` (`tools/bench/src/commands/compare.rs::run`),
  so two different `compare` pairs on the same day get distinct filenames automatically —
  no manual rename needed the way this entry's case required. `grid` and `l1` got the same
  per-target treatment (`grid-<candidate-slug>`, `l1-<primary-file-slug>`), closing the
  `grid.rs`/`l1.rs`/`compare.rs` constant-`STAGE` collision this whole entry, and WHI-1215's
  own issue, were about. `results/2026-08-20-compare-with-regime-slices.md` and every other
  pre-existing `results/*.md` file are left untouched — `report.rs`'s own doc comments
  call `results/` snapshots "committed evidence" that is "never overwritten silently", and
  this PR only ever adds new stage names, never renames an existing file — only new
  reports use the new naming.
- **`docs/agents/issue-tracker.md` described `.scratch/` and the WHI-1192/"`Done` state
  flip" `docs/DEFERRED_ISSUES.md` entries as present-tense open problems** (Low, WHI-1199).
  `docs/agents/issue-tracker.md:8-13` (top-of-file forward-reference), `:62-72`
  (§ Decisions, Decision 1 — "This PR left it in place, stale text and all"), and
  `:102-141` (§ "What happened to `.scratch/`") — WHI-1199 deleted `.scratch/` and moved
  both `docs/DEFERRED_ISSUES.md` entries those sections cite to *Resolved*, but all three
  sites still read as if none of that had happened. This was deferred because
  `docs/agents/issue-tracker.md` is a governance carve-out path
  (`docs/GIT_WORKFLOW.md` § Repo-wide governance carve-out) and WHI-1199's diff was
  non-carve-out only. Resolved by WHI-1200 (`b767dbc`), which updated all three sites to
  past tense. Entry moved here by WHI-1214.
