# Release Tree Binding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A release whose `main` moved during the run publishes nothing, and the release patch can only be applied on
the commit the run was dispatched on (F-10).

**Architecture:** `publish-release` pushes the bump commit and the tag in one `git push --atomic`.
`apply-release-patch` checks `HEAD^ == $GITHUB_SHA` after `git am`. Tests extract each step's `run:` body and run it
against scratch git repos, reusing F-24's `tests/workflows/conftest.py` fixtures.

**Tech Stack:** GitHub Actions YAML, bash, git, pytest.

**Spec:** [`specs/archive/2026/2026-10-31-release-tree-binding-design.md`](./2026-10-31-release-tree-binding-design.md)

## Global Constraints

- Stacked on `fix/release-prek-failures`; `tests/workflows/conftest.py` exists. Don't edit `_prepare-release.yml`.
- Tests use local bare repos as `origin` (`file://` transport supports `--atomic`). Skip on Windows.
- Factory fixtures for `tmp_path` setup; imports at the top of test modules.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`.

## Review Focus

1. **Nothing left behind on rejection.** After a rejected push the remote has no `v…` tag and `main` is unchanged.
   Task 1, `test_push_publishes_nothing_when_branch_moved`.
2. **Happy path still tags the pushed commit.** Task 1, `test_push_publishes_branch_and_tag_together`.
3. **The `ci.yml` `docs` job still applies the patch.** It is the second caller of `apply-release-patch`; on PRs
   `github.sha` is the merge commit, which is also what it checks out. The PR's own CI run proves it.
4. **Re-run semantics.** "Re-run failed jobs" after an atomic rejection fails again (same `github.sha`); the spec says
   to dispatch again. Check the error message says so.

---

### Task 1: Reproduce, then push atomically

This finding is PLAUSIBLE. Step 2 decides whether it goes on.

**Files:**

- Modify: `.github/workflows/release.yml` (`publish-release`, `Push tag` step ~422-427)
- Create: `tests/workflows/test_release_publish_push.py`

**Interfaces:**

- fixture `release_remote(tmp_path, git_repo) -> Callable[..., tuple[Path, Path]]`: builds a bare `origin` with `main`
  at commit A, clones it to a work dir at A, commits `chore(release): Prepare for 9.9.9 release` there and creates the
  annotated tag `v9.9.9` (as the `Tag the …` step does). With `advance_main=True` it pushes commit B to `origin/main`
  from a second clone. Returns `(origin, workdir)`.

- fixture `run_push_step` runs the `push-release` step body with `bash -e`, `cwd=workdir`, env
  `RELEASE_VERSION=9.9.9`, `GITHUB_REF=refs/heads/main`, `GITHUB_REF_NAME=main` (overridable).

- [ ] **Step 1: Write the tests** (add `id: push-release` to the step first, body unchanged):
    - `test_push_publishes_nothing_when_branch_moved`: `advance_main=True` → exit != 0;
      `git ls-remote --tags origin` is empty; `origin` `main` still equals B.
    - `test_push_publishes_branch_and_tag_together`: `advance_main=False` → exit 0; `origin` `main` equals the work
      dir's `HEAD`; `v9.9.9^{commit}` on `origin` equals the same SHA.
    - `test_push_refuses_non_branch_ref`: `GITHUB_REF=refs/tags/v9.9.8`, `GITHUB_REF_NAME=v9.9.8` → exit != 0,
      stderr mentions `refs/heads/`, `git ls-remote origin` unchanged.

- [ ] **Step 2: Run** `uv run pytest tests/workflows/test_release_publish_push.py -v` against the unchanged body.
      Expected: `test_push_publishes_nothing_when_branch_moved` FAILS (the tag is on `origin`), as the scratch repro
      in the spec showed. **Close criteria:** if it passes against the unchanged two-push body, the finding is closed:
      delete the test file, revert the `id`, skip Tasks 2 and 3, and record in the PR that F-10 did not reproduce.

- [ ] **Step 3: Implement.** Rename the step `Push the release commit and tag`. Body: `set -euo pipefail`; refuse
      non-`refs/heads/*` refs with `::error::`; then
      `git push --atomic origin "HEAD:refs/heads/${GITHUB_REF_NAME}" "refs/tags/v${RELEASE_VERSION}"`. On failure,
      print `::error::` saying the branch moved since dispatch and the release must be dispatched again. Pass
      `GITHUB_REF`/`GITHUB_REF_NAME` through the environment (Actions sets them); no `${{ }}` in the script.

- [ ] **Step 4: Run** the Step 2 command → all PASS.

- [ ] **Step 5: Commit.** `fix(release): push the release commit and tag atomically`

### Task 2: Pin the patch to the dispatched commit

**Files:**

- Modify: `.github/actions/apply-release-patch/action.yml` (`Apply The Release Patch` step)
- Create: `tests/workflows/test_apply_release_patch.py`

**Interfaces:** none produced.

- [ ] **Step 1: Write the tests** (add `id: apply` to the step):
    - fixture: a repo with commit A (touches `a.txt`) and child B (touches `b.txt`); a patch made with
      `git format-patch --keep-subject --binary --stdout` from a commit on A touching `Cargo.toml`, saved as
      `toolr-9.9.9.patch`.
    - `test_apply_on_dispatched_commit`: `HEAD` at A, `GITHUB_SHA=A` → exit 0, `HEAD^ == A`.
    - `test_apply_refuses_other_base`: `HEAD` at B, `GITHUB_SHA=A` → exit != 0, stderr contains both SHAs. (`git am`
      itself succeeds here, so only the new check catches it.)
- [ ] **Step 2: Run** `uv run pytest tests/workflows/test_apply_release_patch.py -v` → `test_apply_refuses_other_base`
      FAILS.
- [ ] **Step 3: Implement.** After `git am`, compare `git rev-parse HEAD^` with `$GITHUB_SHA`; on mismatch print
      `::error::` with both and exit 1.
- [ ] **Step 4: Run** Step 2 → PASS.
- [ ] **Step 5: Commit.** `fix(release): refuse a release patch applied off the dispatched commit`

### Task 3: Verification, archive

- [ ] **Step 1:** No `UNRELEASED.md` entry (CI only).
- [ ] **Step 2:** `uv run pytest tests/workflows tests/action_yml` → green. `prek run --all-files` → green.
- [ ] **Step 3:** The PR's CI runs `apply-release-patch` in the `docs` job; confirm it passes. The atomic push only
      runs in a real release: check the first release after merge pushes both refs (`git ls-remote origin
      refs/tags/vX.Y.Z` points at the bump commit on `main`).
- [ ] **Step 4:**
      `git mv specs/2026-10-31-release-tree-binding-design.md specs/2026-10-31-release-tree-binding-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive release tree binding design and plan`.
