---
name: monitor-pr
description: Monitor an open toolr PR after it is created — watch CI checks (prek, cargo test, pytest,
  Codecov project/patch coverage), triage real failures, fix them meaningfully, push, and re-loop until
  checks settle green. Use right after opening a PR, or when asked to babysit / watch / follow / loop on
  a toolr PR through CI.
allowed-tools: Bash, Read, Grep, Glob, Edit, Write
---

# Monitor an open PR (toolr)

**Goal:** carry a freshly opened PR through toolr's CI without a human babysitting it. Watch the checks,
fix what's genuinely broken, push, and repeat until they settle green. Hand back to the author for
anything that isn't a mechanical fix.

Run this **after** the PR exists (i.e. after `git-spice branch submit` or `gh pr create`).

## Guardrails (do not cross)

- **Never** `gh pr merge` or mark a draft ready — that's the author's call, always confirm first.
- **Never** force-push, push to `main`, or edit `.github/workflows/*`/`.codecov.yml` to make a red check
  pass.
- **No mocking your way to green coverage.** A patch-coverage gap gets a real test that exercises real
  behavior through the actual public API, or — if it's a tooling artifact, not a real gap (see Step 3) —
  a code change that removes the artifact. Never pad a number with a test that mocks internals that don't
  need mocking, and never delete or gate code just to dodge the metric.
- `git-spice repo sync` (or `git fetch` + check branch state) before trusting local state — a PR can
  merge or a branch can move under you between rounds.
- Push only fixes for what CI actually flagged on this PR's own diff. Pre-existing gaps or failures
  elsewhere aren't this PR's problem to fix.

## Step 1 — Resolve the PR

```bash
gh pr view --json number,url,headRefName,isDraft,state
```

If you were given a PR number, pass it to every `gh pr` call below instead of relying on the current
branch.

## Step 2 — Watch the checks settle

```bash
gh pr checks <number> --watch --interval 30
```

This blocks until every check completes, then exits non-zero if any failed. In this harness it typically
exceeds the foreground command timeout and moves to a background task automatically — that's expected;
wait for the completion notification rather than polling manually in a sleep loop.

When checks settle, classify:

- **All pass** → Step 5.
- **`Pre-commit` failed** → run `prek run --all-files` locally, apply what it reports, Step 4.
- **A `Test` / `Test Distribution` job failed** → read the failing log (`gh run view <run-id>
  --log-failed`), use `superpowers:systematic-debugging`, fix the root cause, Step 4.
- **`codecov/patch` or `codecov/project*` failed** → Step 3.
- **A shared-CI flake your change didn't cause** → don't work around it; surface it and stop.

## Step 3 — Triage a coverage failure

Toolr has no CI review bot — this is the only triage step, and it's about numbers, not comments.

Get the exact missing lines from the Codecov PR comment:

```bash
gh pr view <number> --json comments --jq '.comments[] | select(.body | test("Codecov")) | .body'
```

Or, for line-level detail on one file, the public API directly:

```bash
curl -s "https://api.codecov.io/api/v2/github/s0undt3ch/repos/ToolR/report/?path=<file>&sha=<head_sha>" \
  | python3 -c 'import json,sys; d=json.load(sys.stdin); \
    print([ln for ln,v in d["files"][0]["line_coverage"] if v==1])'
```

(`v==1` is a miss; `v==0` is a hit — Codecov's tuple is a state code, not a hit count.)

Cross-reference against this PR's own diff (`git diff -U0 main...HEAD -- <file>`) — only lines the diff
actually introduces are this PR's problem; a pre-existing miss elsewhere in the file isn't.

For each newly-missing line, **before writing anything**:

1. **Reproduce locally first.** Rust: `cargo llvm-cov -p <crate> --tests --text --output-path <path>` and
   grep the line. Python: `uv run coverage run -m pytest && uv run coverage report -m` (or read
   `.coveragerc`). Confirm the same line shows 0 hits before deciding what to do about it.
2. **Decide real gap vs. tooling artifact:**
   - **Real gap** — the logic genuinely never executes under any test (an error branch, an edge case, a
     new code path). Fix: write a real test that drives the actual public function/command with real
     inputs and asserts on real outputs. Extract a small testable helper first if the gap is buried
     inside a function that's awkward to exercise directly (e.g. it touches the filesystem or CLI
     dispatch) — don't reach for a mock just to dodge that refactor.
   - **Tooling artifact** — `cargo llvm-cov`'s line-based coverage can misattribute a line that holds only
     closing punctuation (a lone `)?;` or `)?);` closing a multi-line function call) as 0 hits, even
     though the statement executes every time. Confirmed in [PR
     #451](https://github.com/s0undt3ch/ToolR/pull/451): the surrounding lines of the same wrapped call
     showed real hit counts (32), only the closing-paren-only line showed 0. Fix: reformat so the
     `?`/closing token shares a line with real content (may need `cargo fmt`-fighting via a slightly
     different arg layout, or extracting a smaller helper so the call fits on one line) — never write a
     test whose only purpose is coverage of a formatting artifact.
   - If genuinely unsure which, spend the few minutes to verify empirically (steps above) rather than
     guessing.

## Step 4 — Fix, push, re-loop

- Apply the fix.
- Scale local verification to what changed (see `CLAUDE.md`): Rust or Python touched → run the relevant
  `cargo test -p <crate>` / `uv run pytest` subset, or the full `mise run test` umbrella if unsure. Never
  push on faith.
- `prek run --all-files`.
- If the fix touched anything `cargo xtask build-skill-refs` covers (a `toolr.__all__`-exporting module,
  `action.yml`, a `tests/skill_examples/*` file), run it and commit the regenerated `references/*.md`
  too.
- Re-verify branch state (`git-spice repo sync` or `git fetch origin main`), then commit — Conventional
  Commits, no `Co-Authored-By` trailer (see `.claude.local.md`).
- `git-spice branch submit` (or `git push` if not on a git-spice-tracked branch) to update the existing
  PR.
- Return to Step 2. Cap at **3 fix rounds** — if checks still aren't green after 3, stop and hand the
  remaining failures to the author with what you tried.

## Step 5 — Report and stop

Once checks are green:

- Post a short status: which checks were red, what actually fixed each (real test vs. artifact
  reformat — call out which), how many rounds it took.
- Leave the PR exactly as it was (draft stays draft). Do not merge, do not mark ready, unless explicitly
  asked to in this conversation.

## Notes

- Toolr's CI (`.github/workflows/_test.yml`, `.codecov.yml`) has no AI/bot code-review step — Step 3's
  coverage triage is the whole of the "triage" phase here, unlike repos with a CI review bot leaving PR
  comments.
- `.codecov.yml`: patch coverage target is 99% on touched lines; project coverage ratchets via `target:
  auto` (any drop from the base commit fails, any gain raises the floor for the next PR). Both are real
  gates, not advisory.
- `gh pr checks --watch` reliably exceeds this harness's ~2-minute foreground command limit and moves
  itself to a background task — that's normal, not a failure; the notification on completion is the
  signal to act on, not a manual poll loop.
