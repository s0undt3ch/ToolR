# Action Latest Repo Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Setup ToolR action's `version: latest` resolves against `s0undt3ch/ToolR` whatever repository runs the
workflow (F-22).

**Architecture:** The `resolve-version` step in `action.yml` makes one `gh release view --repo s0undt3ch/ToolR`
call. The offline bash harness in `tests/action_yml/` learns to set `GITHUB_REPOSITORY` and to answer per `--repo`.

**Tech Stack:** composite GitHub Action (bash), pytest + PyYAML, `cargo xtask build-skill-refs`.

**Spec:** [`specs/archive/2026/2026-10-31-action-latest-repo-design.md`](./2026-10-31-action-latest-repo-design.md)

## Global Constraints

- Touch only the `resolve-version` step and the `version` input description in `action.yml`. Later branches in this
  stack (`fix/action-cache-key`, `fix/action-attestation-signer`) edit other steps.
- No schema change.
- Factory fixtures for `tmp_path` setup, imports top-of-file.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`.

## Review Focus

1. **The regression test reproduces the reported shape.** The stub must answer differently per `--repo`, and
   `GITHUB_REPOSITORY` must be a consumer repo. A stub that ignores `--repo` passes before and after the fix. Task 1,
   `test_latest_ignores_caller_repository`.
2. **The consumer tag must clear the `0.20.0` floor** (`v9.9.9`), so the old code fails on the version assertion, not
   on the minimum-version error.
3. **A `gh` failure still fails the step,** now with `gh`'s own stderr. Task 1, `test_latest_gh_failure_fails`.
4. **Not provable in toolr's own CI.** There `GITHUB_REPOSITORY` is `s0undt3ch/ToolR`, so the bug never fires. The
   harness test is the proof; the PR's CI only shows that the steps using `version: latest` still log
   `setup-toolr: resolved 'latest' to <x>`.

---

### Task 1: Pin the `latest` lookup to `s0undt3ch/ToolR`

**Files:**

- Modify: `action.yml` (`inputs.version.description`; step `resolve-version`, the `latest` branch)
- Modify: `tests/action_yml/test_resolve_version.py` (fixture `resolve_runner`, new tests)
- Regenerate: `skills/toolr-ci-setup/references/action.md`

**Interfaces:** `resolve_runner(*, input_version: str = "", latest_tag: str = "v0.20.1", caller_repo: str =
"consumer/repo", caller_tag: str = "v9.9.9", gh_fails: bool = False) -> str`. The new keyword arguments default so the
existing tests keep calling it unchanged.

- [ ] **Step 1: Extend the harness.** In `resolve_runner`:
    - export `GITHUB_REPOSITORY="<caller_repo>"`;
    - the `gh` stub walks its arguments for `--repo <value>`, prints `latest_tag` for `s0undt3ch/ToolR`, `caller_tag`
      for anything else, and returns 1 with `gh: release not found` on stderr when `gh_fails` is set;
    - append every stub call's argument list to `tmp_path / "gh_calls"`, one line per call.

  Add a `gh_calls` fixture returning a reader for that file, so tests don't rebuild the path.

- [ ] **Step 2: Write the tests.**
    - `test_latest_ignores_caller_repository`: `resolve_runner(input_version="latest")` → returns `"0.20.1"`; every
      `gh_calls` line contains `--repo s0undt3ch/ToolR`; no line contains `consumer/repo`.
    - `test_empty_version_ignores_caller_repository`: same with `input_version=""`.
    - `test_latest_gh_failure_fails`: `gh_fails=True` → `AssertionError` whose message matches `release not found`
      (the harness raises with the step's stderr).
    - The existing `test_resolve_returns_expected_version` cases stay as they are.

- [ ] **Step 3: Run and confirm the failure.** `uv run pytest tests/action_yml/test_resolve_version.py -v` →
      `test_latest_ignores_caller_repository` and `test_empty_version_ignores_caller_repository` fail with `'9.9.9' !=
      '0.20.1'`; `test_latest_gh_failure_fails` fails because the old `2>/dev/null` hides the stub's stderr; the rest
      pass.

- [ ] **Step 4: Implement.** Replace the two lookups and the fallback comment with the single
      `gh release view --repo s0undt3ch/ToolR --json tagName --jq '.tagName' | sed 's/^v//'` call (no `2>/dev/null`),
      keep the empty-result error. Update the comment above it to say the caller's repo is deliberately not consulted.
      Change the `version` input description's `latest` sentence to "The special value `latest` resolves to the most
      recent `s0undt3ch/ToolR` release, whichever repository runs the workflow."

- [ ] **Step 5: Regenerate the skill reference.** `cargo xtask build-skill-refs`, then
      `cargo xtask build-skill-refs --check` → clean. Only `skills/toolr-ci-setup/references/action.md` changes.

- [ ] **Step 6: Run.** `uv run pytest tests/action_yml -v` → all pass.

- [ ] **Step 7: Commit.** `fix(action): resolve version latest against s0undt3ch/ToolR only`

### Task 2: Release notes, verification, archive

- [ ] **Step 1:** Add the spec's `UNRELEASED.md` text verbatim. Commit
      `docs(unreleased): note the action's latest-version lookup fix`.
- [ ] **Step 2:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green (covers
      `actionlint`/`shellcheck` on `action.yml` if configured).
- [ ] **Step 3: Actions check.** On the PR, open any job that runs `uses: ./` with `version: latest` (for example
      `ci.yml`'s pre-commit job). The "Resolve toolr version" step logs `setup-toolr: resolved 'latest' to <x>` where
      `<x>` is the current ToolR release. This shows nothing regressed; the bug itself is covered by Task 1's tests.
- [ ] **Step 4:**
      `git mv specs/2026-10-31-action-latest-repo-design.md specs/2026-10-31-action-latest-repo-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive action latest repo design and plan`.
