# Trap registry — append-only, repo-specific

This registry is the **durable channel** by which a trap reaches every implementer after the
first one it's handed to — `implementer-prompt.md`'s launch prompt pastes it verbatim into
every issue's prompt. (`SKILL.md`'s "Feed forward" step already names the other, immediate
channel: mentioning a fresh trap directly in the very next launch prompt, which takes effect
with no commit at all but doesn't outlive that one issue.) An entry that never makes it into
this file does not survive past whichever single issue it may have been mentioned to by hand.
Carry every entry into each launch prompt. Cost is why they are here. Each entry below is
cited against the repo so it stays checkable; an entry that stops being true belongs in
`docs/DEFERRED_ISSUES.md`'s own resolved section, not silently deleted here.

**A trim is self-declaring.** An entry leaves this file in one of exactly two ways: it
graduates to `docs/DEFERRED_ISSUES.md`'s resolved section once it stops being true (the
paragraph above), or it gets merged into another entry that already covers the same failure.
Either way, the PR that does it must say which entry number left or was absorbed, and why.
WHI-1251 trimmed a reviewed 9-entry draft down to the 7 that first landed here without
disclosing what didn't survive — two entries outright (release profile; `pgrep -fl`) and half
of a third (the *ordering* half of the generate-once lesson; only its *provenance* half made
it through as entry 5) — and the implementer's own final report ("all 7 survived scrutiny")
was accurate about the seven present and silent about what was missing. That is exactly the
failure this rule exists to make impossible to repeat quietly. WHI-1252 restored the missing
content as entries 8 and 9 below, plus entry 5's ordering half. A trim with no disclosure of
what left and why, stated in the PR that does it, is presumed accidental, not reviewed.

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
5. **Generate committed artifacts only from a clean, committed sha — and only once, after the
   review loop closes.** Two distinct failure modes on the same object, both required:
   - *Provenance:* `tools/bench/src/report.rs` appends `+dirty` to the commit-sha field
     whenever the working tree has uncommitted changes at generation time — regenerating a
     report before committing the change it measures silently stamps the wrong provenance
     onto a committed number.
   - *Ordering:* `/code-review` dispatches reviewers against `git diff <fixed-point>...HEAD`
     (`.claude/skills/code-review/SKILL.md:21`) — a review round reads that diff, nothing
     else, so it needs no measurement report to exist at all before it runs. Regenerating one
     before every round anyway turns each purely prose-level review finding into a full
     re-measurement — exactly what happened rerunning WHI-1247's multi-hour `ceiling --fit`
     after each of three review rounds instead of once at the end. Generate the committed
     artifact exactly once, from the final clean commit, after round 3 (or the escalation
     pass) closes — never per round.
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
8. **Release profile only — liveness and CPU do not prove it.** Echo the binary path
   immediately before every measurement and confirm it reads `target/release/`, not
   `target/debug/`; `ps` cannot tell the two apart — a healthy-looking, high-CPU process is
   exactly what a debug build in the middle of a multi-minute run also looks like. Every
   strategy's own `NOTES.md` already runs its measurements as `cargo run -p prop-amm-bench
   --release -- ...` (e.g. `strategies/004-ewma-shock-decay-fee/NOTES.md:214`) — this entry is
   that same convention, made mechanically checkable instead of merely followed by habit.
   WHI-1247's own account, as related in WHI-1252, puts one lapse at roughly 25 minutes of
   wall clock against the ~7 a `target/release/` build takes for the same work, with `ps`
   pegged at 746% CPU the entire time — no committed report in this repo independently
   reproduces those three figures, so treat them as the reported cost, not a re-derived one.
   Either way, the mechanical fix (echo the path, assert `release`) is what actually closes
   the trap, not any CPU or liveness threshold.
9. **`pgrep -fl` dumps this machine's entire shell-snapshot environment** instead of the one
   process you meant to find. Verified directly (WHI-1252): backgrounding a process and
   running `pgrep -fl sleep` matched 6 processes and printed 470 lines / ~37 KB total (longest
   line 308 bytes) — one match is the shell-snapshot-sourcing wrapper itself, whose command
   string embeds hundreds of literal `export CODEX_COMPANION_SESSION_ID=...` /
   `export CLAUDE_PLUGIN_DATA=...` lines verbatim. `pgrep -f sleep | head -1` printed only the
   5-digit pid. Use the second form to get the pid, then `ps -o pid,etime,command -p <pid>` to
   check it's alive and is the right binary.
