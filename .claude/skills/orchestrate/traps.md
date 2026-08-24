# Trap registry — append-only, repo-specific

This registry is the **sole channel** by which traps reach an implementer —
`implementer-prompt.md`'s launch prompt pastes it verbatim, and an entry missing here reaches
nobody. Carry every entry into each launch prompt. Cost is why they are here. Each entry below
is cited against the repo so it stays checkable; an entry that stops being true belongs in
`docs/DEFERRED_ISSUES.md`'s own resolved section, not silently deleted here.

**Trimming is self-declaring.** Removing or merging an entry is allowed, but the PR that does
it must say which entry number was dropped or merged and why. WHI-1251 trimmed a reviewed
9-entry draft to the 7 below it without disclosing the 3 that didn't survive, and the
implementer's own final report ("all 7 survived scrutiny") was accurate about the seven
present and silent about the ones absent — exactly the failure this rule exists to make
impossible to repeat quietly (WHI-1252, which restored entries 5 and 9 below and the ordering
half of entry 6, verified against the actual reviewed draft preserved at
`scratch/orchestrate-skill-draft/SKILL.md:146-164`). A trim with no such statement in its PR
body is presumed accidental, not reviewed.

1. **`gh pr create` needs `--repo <owner/repo>`.** This clone (`git remote -v`) is a fork of
   `benedictbrady/prop-amm-challenge`, with `upstream` set to `no_push` — `gh` otherwise
   resolves the PR base against `upstream` and fails.
2. **Never `cargo fmt --all`, and `cargo fmt -- <files>` does not scope either.** Verified
   directly: in an isolated crate, `cargo fmt -- src/other.rs` reformatted an untouched
   `src/main.rs` in the same crate too — passing files after `--` does not stop cargo fmt
   from formatting every target in the crate. The scoping tool is plain `rustfmt <files>`
   (verified the same way: it left the crate's other file untouched). A repo-wide run
   touches the 6 upstream files with known fmt drift and makes every future upstream-sync
   merge conflict on formatting (`docs/DEFERRED_ISSUES.md`'s fmt-drift entry).
3. **`cargo clippy -- -D warnings` and `cargo fmt --check` already fail on inherited upstream
   code** (`AGENTS.md`'s lint/format caveat; exact list in `docs/DEFERRED_ISSUES.md`). That is
   the baseline, not the implementer's bug.
4. **Measurements cannot run from inside `.claude/worktrees/`.** The reference compile path
   (`tools/bench/src/compile.rs::build_and_load`) writes a nested `Cargo.toml` that cargo
   refuses there (`docs/DEFERRED_ISSUES.md`, WHI-1247). Run from a detached checkout outside
   the repo tree, synced to the reported commit, and copy artifacts back.
5. **Release profile only — liveness and CPU do not prove it.** Echo the binary path
   immediately before every measurement and confirm it reads `target/release/`, not
   `target/debug/`; a bare `cargo run`/`cargo build` without `--release` is one to two orders
   of magnitude slower, and `ps` cannot tell the difference — a healthy-looking, high-CPU
   process is exactly what a debug build in the middle of a multi-minute run also looks like.
   WHI-1247's own ceiling-lane measurement is the recorded cost of trusting liveness alone:
   the run that was actually reading `target/debug/` took roughly 25 minutes of wall clock
   for work `target/release/` does in about 7, and `ps` showed it pegged at 746% CPU the
   entire time — indistinguishable from correct-and-busy until the binary path itself was
   checked. The single mechanical fix (echo the path, assert `release`) is what closes this,
   not a CPU or liveness threshold of any kind.
6. **Generate committed artifacts only from a clean, committed sha — and only once, after the
   review loop closes.** Two distinct failure modes on the same object, both required:
   - *Provenance:* `tools/bench/src/report.rs` appends `+dirty` to the commit-sha field
     whenever the working tree has uncommitted changes at generation time — regenerating a
     report before committing the change it measures silently stamps the wrong provenance
     onto a committed number.
   - *Ordering:* `/code-review` dispatches reviewers against `git diff <fixed-point>...HEAD`
     (`code-review/SKILL.md:21`) — code and the spec, never a generated report file — so a
     measurement report has no reason to exist before the review loop is done with it.
     Regenerating it after every round anyway turns each purely prose-level review finding
     into a full re-measurement, which is exactly what happened rerunning WHI-1247's
     multi-hour `ceiling --fit` after each round instead of once at the end. Generate the
     committed artifact exactly once, from the final clean commit, after round 3 (or the
     escalation pass) closes — never per round.
7. **Commit before dispatching a reviewer, then hand it a three-dot diff, and verify it is
   non-empty before dispatching.** `/code-review`'s own canonical command is `git diff
   <fixed-point>...HEAD` (three-dot, against the merge-base) plus an explicit non-empty
   check before the parallel reviewers ever run — use the same form here. Two-dot
   (`git diff <base>`, base-tip vs. working tree) looks like the fix for an empty three-dot
   diff on an uncommitted branch, but it is not: once `<base>` advances past your merge-base,
   two-dot shows the reviewer those foreign commits, reversed, presented as part of the
   change under review — a non-empty, plausible, and much harder to catch failure than the
   empty diff it was meant to fix. Commit first (so three-dot is never comparing against an
   empty commit range) and use three-dot.
8. **Report slots are per-stage-per-day and stage names may omit the segment.** Same-day runs
   of one target on two segments can collide on one filename (`docs/DEFERRED_ISSUES.md`,
   WHI-1195). WHI-1215 resolved this for `grid`/`l1`/`compare` only (per-target stage naming) —
   `anchor.rs::STAGE` is still a constant and still collides (`docs/DEFERRED_ISSUES.md`'s
   WHI-1215 follow-on note); check the current stage-naming code before assuming a given
   command is fixed.
9. **`pgrep -fl` dumps this machine's entire shell-snapshot environment** (hundreds of lines
   of `export` noise) instead of the one process you meant to find — it wastes output budget
   every time it's used, and the project's own long-running-work convention
   (`implementer-prompt.md`'s "Long-running work" section) already uses the working
   alternative: `pgrep -f <pattern> | head -1` to get the pid, then
   `ps -o pid,etime,command -p <pid>` to check it's alive and is the right binary.
