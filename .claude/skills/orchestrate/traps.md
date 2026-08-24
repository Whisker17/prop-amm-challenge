# Trap registry — append-only, repo-specific

Carry every entry into each launch prompt. Cost is why they are here. Each entry below is
cited against the repo so it stays checkable; an entry that stops being true belongs in
`docs/DEFERRED_ISSUES.md`'s own resolved section, not silently deleted here.

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
5. **Generate committed artifacts only from a clean, committed sha.** `tools/bench/src/
   report.rs` appends `+dirty` to the commit-sha field whenever the working tree has
   uncommitted changes at generation time — regenerating a report before committing the
   change it measures silently stamps the wrong provenance onto a committed number.
6. **Commit before dispatching a reviewer, then hand it a three-dot diff, and verify it is
   non-empty before dispatching.** `/code-review`'s own canonical command is `git diff
   <fixed-point>...HEAD` (three-dot, against the merge-base) plus an explicit non-empty
   check before the parallel reviewers ever run — use the same form here. Two-dot
   (`git diff <base>`, base-tip vs. working tree) looks like the fix for an empty three-dot
   diff on an uncommitted branch, but it is not: once `<base>` advances past your merge-base,
   two-dot shows the reviewer those foreign commits, reversed, presented as part of the
   change under review — a non-empty, plausible, and much harder to catch failure than the
   empty diff it was meant to fix. Commit first (so three-dot is never comparing against an
   empty commit range) and use three-dot.
7. **Report slots are per-stage-per-day and stage names may omit the segment.** Same-day runs
   of one target on two segments can collide on one filename (`docs/DEFERRED_ISSUES.md`,
   WHI-1195). WHI-1215 resolved this for `grid`/`l1`/`compare` only (per-target stage naming) —
   `anchor.rs::STAGE` is still a constant and still collides (`docs/DEFERRED_ISSUES.md`'s
   WHI-1215 follow-on note); check the current stage-naming code before assuming a given
   command is fixed.
