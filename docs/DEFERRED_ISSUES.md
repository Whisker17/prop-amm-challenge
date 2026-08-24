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

- **`resolve_ceiling_segment` re-implements part of `SegmentSelector::resolve`'s single-use
  check, and `VariantArg` carries its `OracleVariant` mapping and its report-slug string as
  two separate hand-written `match`es** (Low, WHI-1247). Both flagged in round-1 review of
  this issue and accepted as judgement calls at the time, but never actually logged here per
  `AGENTS.md`'s own Git-workflow step 3 until round-2 review caught the gap.
  `tools/bench/src/commands/ceiling.rs::resolve_ceiling_segment`'s own doc comment already
  explains why it can't just call `SegmentSelector::resolve` unmodified (that method only
  blocks `single_use` in the *absence* of the spend flag, which would let `--i-am-spending-
  the-test-segment` defeat this lane's stronger, unconditional `test`-segment refusal) —
  the duplication is the single-use-check tail after that guard, not the guard itself.
  `VariantArg`'s `impl From<VariantArg> for OracleVariant` and `impl VariantArg { fn
  slug() }` are two small, separately-necessary switches over the same two-variant enum
  (one for the oracle's own type, one for a report filename fragment) rather than one
  combined mapping, because they serve genuinely different consumers (`oracle.rs` vs. the
  report writer) and a single fused function would couple the two for no shared benefit.
  Fix, if ever revisited: none planned unless `SegmentSelector::resolve` itself grows a
  variant that takes the spend flag into account for *all* single-use segments (which would
  let `resolve_ceiling_segment` shrink to a thin wrapper), or `VariantArg` grows a third
  consumer that would make a combined mapping pay for itself.
- **`run_fit`'s `Anchored`/`Floating` arms share the same shape** (Low, WHI-1247).
  `tools/bench/src/commands/ceiling.rs::run_fit` — both arms call
  `search::coarse_grid_then_coordinate_descent` with a closure that runs
  `run_catching_panics` and maps `values` into an `OracleParams`, differing only in the
  `specs` array (`[concentration_spec(), spread_bps_spec()]` vs `[spread_bps_spec()]` alone)
  and how `values` maps to `(concentration, spread_bps)` (both fit jointly vs.
  `concentration` held at `fixed_concentration.unwrap()`). Flagged in round-2 review of this
  issue. Deferred rather than fixed in this PR: the two arms differ in exactly the two
  places you'd need a generic callback for (the spec array's arity and the
  values-to-params mapping), so collapsing them would trade a readable two-branch match for
  a closure-of-a-closure that is not obviously clearer — worth a second look once the
  follow-up issue (the fixed-lag cursor rung) adds a third rung and a third arm, if that
  third arm turns out to fit the same shape.
- **`CeilingArgs::cursor` is a hand-validated `String`, not a `ValueEnum`** (Low, WHI-1247).
  `tools/bench/src/commands/ceiling.rs::CeilingArgs::cursor` is validated by comparing
  against `CURSOR_MODE` in `run()`, in a file where `variant` is a real `ValueEnum` and
  gets `--help` enumeration and rejection for free. Flagged in round-2 review of this
  issue. Its own doc comment already gives the reason: this issue delivers exactly one
  rung, so a `ValueEnum` today would be a one-variant enum purely for a rung that doesn't
  exist yet — the follow-up issue this one Blocks adds the second rung, and that is the
  natural point to convert `cursor` to a real `ValueEnum` with both values, rather than
  guessing the second variant's name now.
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
- **`tools/bench/src/estimator_probe.rs` mirrors `strategies/005-vol-adaptive-cpmm-fee/lib.rs`'s
  and `strategies/004-ewma-shock-decay-fee/lib.rs`'s `after_swap` math with no automated
  drift-detection control** (Medium, WHI-1225). Same category as the `curve_checks.rs` entry
  above: `docs/DESIGN.md` §4.3 permits duplicating upstream/strategy logic only alongside a
  control, and this module (the shadow accumulator `bench estimator-probe` runs alongside a
  real `005`/`004` batch to replicate their variance/EWMA estimators without touching real
  storage) has none — its own tests only assert self-consistency properties (`elapsed_sum >=
  count`, a monotone floor ladder), not agreement with the actual strategy files' committed
  math. If either `lib.rs` changes its estimator formula, nothing here would fail, and Probe
  A's numbers would silently stop describing the strategy they claim to. Mitigated, not
  closed: this issue's own containment check (a scratch copy with the new storage layout but
  the *old* divisor reproduced `strategies/005-vol-adaptive-cpmm-fee`'s committed screening/
  train/validation numbers bit-exactly, `strategies/005b-elapsed-steps-divisor-fix/NOTES.md`
  § Probe B) is one-time evidence the mirror was faithful *at measurement time*, not an
  ongoing control. Fix: re-diff `vol005_recorder`/`ewma004_recorder` against the two
  `strategies/*/lib.rs` files by hand whenever either changes; revisit if this probe is ever
  reused for a future issue, at which point a shared regression fixture (mirroring
  `curve_checks.rs`'s ported-test approach) would be worth the added coupling.
- **`strategies/005-vol-adaptive-cpmm-fee/lib.rs`'s inherited "MLE of stationary variance"
  header comment is not softened, despite WHI-1225 measuring exactly why it's imprecise**
  (Low, WHI-1225). WHI-1225's own acceptance criteria asked for this comment (`fee_from_state`'s
  `variance` line, `lib.rs:257`) to be softened to "consistent moment estimator" — `var_sum /
  count` is not the MLE of a per-step variance whenever a sample spans a multi-step gap, which
  is precisely what that issue measured (a 35.6% average `sigma_hat` inflation, `strategies/
  005b-elapsed-steps-divisor-fix/NOTES.md` § Probe A). Left unfixed: WHI-1225 committed no
  `005b` `lib.rs` to carry an edited comment, and `005` itself is a shipped, ranked strategy —
  editing its source as a side effect of a probe-gated ablation issue is out of §2.9's
  minimum-change scope for a faithful port. `strategies/005-vol-adaptive-cpmm-fee/NOTES.md`
  § Estimator bias has an addendum explaining the imprecision, which defends the comment's
  accuracy as a statement of intent rather than softening it — so the criterion's literal text
  is not satisfied. Fix: the next issue that touches `005`'s own `lib.rs` (a genuine `005b`
  variant that ships a corrected estimator, or a governance-scoped comment sweep) should
  soften this line then, not as an unrelated side effect of a different issue.
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
- **§2.10's "open one variant for each of the top three" has no explicit spend-tracking
  location** (Low, WHI-1223). `docs/DESIGN.md` §2.10 authorizes exactly three §2.9 variant
  slots (one per top-3 finalist) but names no single place that records how many have been
  opened or which finalists they cover — each variant issue (WHI-1223 for `004b`, the
  sibling `WHI-1224` for `003b`) is individually traceable via Linear, but there is no
  `docs/DESIGN.md` §6.2 row or other repo-local ledger totting up the three-slot budget the
  way the 300-point search budget has a declared cap in `config/bench.toml`'s `[search]
  max_points`. Not fixed here: adding that ledger is a `docs/DESIGN.md` process change
  spanning all three variant issues, not a `004b`-specific concern this issue's own
  acceptance criteria asked for. Fix: once all three top-3 variants have opened issues, add
  a short §2.10 addendum (or a §6.2-adjacent table) listing the three slots and the issue
  that spent each.
- **`floating`'s two required ACs — a paired-by-seed comparison against the 0-line, and a
  staleness distribution — are unmet, with no manufactured substitute** (Low, WHI-1247).
  WHI-1247's acceptance criteria ask for both numbers "for both `target_x` variants," but
  `floating`'s fitted point panics on final re-evaluation (`ceilings/C-orbic-oracle/NOTES.md`
  § `floating`, `results/2026-08-23-ceiling-floating-...md`), so no paired comparison or
  staleness distribution exists for this variant. Flagged in round-3 spec review of this
  issue. Deferred rather than fixed: `NOTES.md`'s own "Considered and rejected: recovering a
  number for `floating` anyway" section already explains why manufacturing one (e.g.
  reporting a runner-up point's number as if it were `floating`'s) would misrepresent a
  variant WHI-1247 step 3 itself scopes as "never a result on its own" — the empty result
  *is* the diagnostic, not a gap to fill. Logged here so the unmet AC has a tracked home
  rather than only living in report/NOTES.md prose. Fix: none planned for this variant under
  its current frozen parameter space; would only become moot if a future issue changes
  `floating`'s own scope (e.g. bounding the oracle's quote by the reserve, which WHI-1247's
  orchestrator direction — see the panic characterization in `NOTES.md` § `floating` — treats
  as a structural finding to report, not a defect to patch around).
- **Step 3(b)'s "expect `concentration` to run to its upper bound and the axis to be
  degenerate" prediction was never tested** (Low, WHI-1247). WHI-1247 step 10 freezes
  `concentration` for variant (b) ("re-fit spread only"), so `floating`'s search never
  varies `concentration` at all — it stays pinned at `anchored`'s own fitted 2.33 throughout
  variant (b)'s run, and the prediction about where an unconstrained `concentration` axis
  would land is untestable under that budget. Flagged in round-3 spec review of this issue;
  neither `NOTES.md` nor the committed reports previously noted this tension between step
  3(b)'s prediction and step 10's freeze. `ceilings/C-orbic-oracle/NOTES.md` § `floating` now
  states this explicitly. Fix: none planned — re-opening `concentration` as a free variable
  for variant (b) would mean re-fitting *two* parameters jointly for a variant already scoped
  as diagnostic-only, spending search budget WHI-1247's own step 10 chose not to spend here;
  worth revisiting only if a future issue specifically wants the degenerate-axis claim tested
  rather than just noted as untested.
- **`ceiling --fit`'s reference-path compile fails when run from inside
  `.claude/worktrees/<name>`** (Low, WHI-1247). `tools/bench/src/compile.rs::build_and_load`
  shells out to `cargo run -p prop-amm -- build`, which drives `crates/cli/src/commands/
  compile.rs`'s `ensure_build_dir` — an isolated build package with no `[workspace]` table
  of its own. When the working tree it runs from is itself nested inside another git
  worktree of the same repo (exactly this repo's own mandated `.claude/worktrees/<name>`
  layout for issue work), cargo's ancestor search resolves the *primary clone's* workspace
  instead of the isolated package's manifest and hard-errors with `current package believes
  it's in a workspace when it's not` — documented in `tools/bench/src/compile.rs`'s own test
  doc comment. `tools/bench/src/fast_compile.rs` hit the identical error for its own fast
  build path and was fixed under WHI-1205 by giving its generated `Cargo.toml` an empty
  `[workspace]` table; that fix was never extended to `ensure_build_dir`, because that
  function lives in `crates/cli`, upstream-owned code this repo edits only through an
  upstream sync (`AGENTS.md`). Both `ceiling --fit` runs behind this issue's committed
  numbers were run from a detached scratch worktree instead (`ceilings/C-orbic-oracle/
  NOTES.md` § Fitted point now records this as a reproducibility caveat). Fix: none planned
  in this PR — `crates/cli` is out of scope for WHI-1247 and any fix belongs to the upstream
  sync lane, not a feature issue; would need its own ticket proposing the same empty-
  `[workspace]`-table fix for the reference compile path.
- **The `floating` panic's committed evidence is a placeholder; the string-payload version
  of the same site is only attested at an earlier, different commit** (Low, WHI-1247).
  `831284c`'s committed floating report and `ceilings/C-orbic-oracle/NOTES.md` § `floating`
  record `panicked with a non-string payload (at crates/sim/src/curve_checks.rs:23:9)` for
  the fitted point's final-re-evaluation panic — this lane's own `panic_message` downcast
  (`tools/bench/src/commands/ceiling.rs`) only recognizes `&str`/`String` payloads and
  failed on whatever this one actually is. A separate, uncommitted run of the identical
  command at this branch's first commit (`1992c09`, before `49509a3` added the
  catch-and-report path) hit the same panic site with no catching harness in front of it,
  so Rust's default panic hook printed the real message verbatim: `submission shape
  violation during arbitrage sell search: monotonicity violated: input 0.686681 -> output
  51.183366, input 0.740636 -> output 0.000000`. `crates/sim/src/curve_checks.rs` is
  byte-identical between `1992c09` and `831284c`, and `tools/bench/src/oracle.rs`'s
  `oracle_swap` diff between them (round-3 review, standards finding #4) only reorders the
  `side`-match arms without changing the `price`/`k`/`out` formulas, so this string is
  good-faith evidence of the same mechanism, cited in `NOTES.md` with that caveat — but it
  was never re-derived at `831284c` itself, and no run at `831284c` has confirmed this is
  the bit-identical violation rather than a different invalid point from the same search
  space. Fix: none planned in this PR — the panic-catching harness's non-string-payload
  case would need to actually be identified (why is this payload not a plain `&str`/
  `String`?) and either widened or bypassed to recover the real message at the current
  commit, which is a harness change orthogonal to the ceiling lane's own scope; would need
  its own ticket if a future issue wants the exact current-commit message rather than the
  attributed-earlier-commit citation this PR settles for.
- **`evaluate_fingerprint_point` scores candidate parameter points by averaging only over
  seeds that survive that point's own fingerprint hardening checks, contaminating the
  search itself rather than just the final reported number** (Medium, WHI-1249).
  `tools/bench/src/commands/ceiling.rs::evaluate_fingerprint_point` drops seeds that trip a
  fingerprint hardening check from that point's own score, the same survivors-only
  averaging that produced the selection-biased `L=1` headline documented in
  `ceilings/C-orbic-oracle/NOTES.md` — except here it runs on every point the search
  evaluates, not just the final one. A point that trips more (disproportionately adverse)
  seeds into floor collisions has those seeds silently excluded from its own score, so it
  can look better than a point that survives the same seeds honestly; the search therefore
  has a standing incentive to drift toward fragile points instead of the true optimum.
  Symptom observed in this investigation: the fingerprint-mode fit converged at
  `concentration = 94.33` (within 6% of its declared upper bound of 100.00) against the
  kept trade-triggered fit's `concentration = 2.33` (deep interior of the same space).
  Fix: not made in this issue (WHI-1249 is a labels-and-documentation correction only, no
  `crates/`/`strategies/`/`results/` changes). WHI-1250 (buy-probe-only cursor-advance) is
  the tracked follow-up that most directly attacks the false-match rate feeding this
  mechanism, but restricting cursor-advance does not by itself restructure
  `evaluate_fingerprint_point`'s survivors-only scoring — a real fix needs the search's own
  scoring path to stop silently dropping tripped seeds (e.g. penalize instead of exclude,
  or score over the full seed set with a fixed penalty for tripped ones) rather than only
  reducing how often seeds trip in the first place.

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
