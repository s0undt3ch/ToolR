# Release Prek Failures Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A failing prek hook aborts release prep instead of being committed past, and prek fixes can't touch files
the release didn't change (F-24).

**Architecture:** The `Commit changes` step of `_prepare-release.yml` runs prek once to fix, checks that the fixes
stayed inside the release's own files, runs prek again strictly, then commits. Tests pull the step's `run:` body out
of the workflow YAML and run it under `bash` against a scratch git repo with a stub `prek`, as
`tests/action_yml/test_resolve_version.py` does for `action.yml`.

**Tech Stack:** GitHub Actions YAML, bash, pytest, PyYAML.

**Spec:** [`specs/2026-10-31-release-prek-failures-design.md`](./2026-10-31-release-prek-failures-design.md)

## Global Constraints

- Touch only the `Commit changes`, `Show Changes Diff` and `Tag the release` steps of `_prepare-release.yml`. F-10 and
  F-25 stack on this branch and edit other parts of the release workflows.
- No `${{ … }}` expressions inside the commit step's script; read `$RELEASE_VERSION` and `$GH_TOKEN` from the env.
- Tests need `git` and `bash`; skip on Windows (`pytest.mark.skipif(sys.platform == "win32", …)`), as the step only
  runs on `ubuntu-latest`.
- Factory fixtures for `tmp_path` setup; imports at the top of test modules.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`.

## Review Focus

1. **Untracked files the release already has.** `release-notes.md` and `.release-changes.md` are untracked before prek
   runs. The before/after comparison must not flag them. Task 1, `test_commit_step_ignores_preexisting_untracked`.
2. **Fixes to release files still go in.** The first prek pass exists to fix `CHANGELOG.md`; that must keep working.
   Task 1, `test_commit_step_commits_fixes_to_release_files`.
3. **A fix to an untouched file fails, and names the file.** Task 1,
   `test_commit_step_rejects_fixes_outside_release_files`.
4. **Token reaches prek.** `GH_TOKEN` is set on the step, not only on the checkout. Task 2, by reading the YAML.

---

### Task 1: Make the commit step fail on real hook failures

**Files:**

- Modify: `.github/workflows/_prepare-release.yml` (`Commit changes` step, ~261-267)
- Create: `tests/workflows/__init__.py`, `tests/workflows/conftest.py`,
  `tests/workflows/test_prepare_release_commit.py`

**Interfaces:**

- `tests/workflows/conftest.py`:
    - fixture `workflow_step_script() -> Callable[[Path, str, str], str]`: given a workflow or composite-action YAML
      path, a job id (ignored for a composite action; pass `""`) and a step `id`, return the step's `run:` body.
      Fails the test with a clear message if the step is missing. F-10 reuses it.
    - fixture `git_repo(tmp_path) -> Callable[..., Path]`: factory that `git init -b main`s a directory, sets a local
      `user.name`/`user.email`, writes the given `{relative_path: content}` files, commits them, and returns the path.

- `tests/workflows/test_prepare_release_commit.py`:
    - fixture `stub_prek(tmp_path) -> Callable[[list[str]], Path]`: writes an executable `prek` into a `bin/` dir. Each
      call `N` runs the bash snippet at index `N` of the given list (a counter file tracks calls), so a test can say
      "first call edits `CHANGELOG.md` and exits 1, second exits 0". Returns the `bin/` dir to prepend to `PATH`.
    - helper fixture `run_commit_step` that runs the extracted script with `bash -e` (Actions' default), `cwd` the
      repo, env `PATH=<stub>:$PATH`, `RELEASE_VERSION=9.9.9`, and returns the `CompletedProcess`.

- [ ] **Step 1: Write the tests.** Each starts from a repo with `Cargo.toml`, `CHANGELOG.md` and `docs/other.md`
      committed, then edits `Cargo.toml` and `CHANGELOG.md` (the release's changes) and writes untracked
      `release-notes.md`, as the real job does before this step.
    - `test_commit_step_fails_when_prek_keeps_failing`: prek always exits 1 → step exit code != 0, `git rev-list
      --count HEAD` unchanged.
    - `test_commit_step_commits_fixes_to_release_files`: call 1 appends a newline to `CHANGELOG.md` and exits 1, call
      2 exits 0 → exit 0; one new commit, subject `chore(release): Prepare for 9.9.9 release`; `git show HEAD --
      CHANGELOG.md` includes the fix; `release-notes.md` is still untracked.
    - `test_commit_step_rejects_fixes_outside_release_files`: call 1 edits `docs/other.md` and exits 1 → exit != 0,
      stderr contains `docs/other.md`, no new commit, and prek was called once (counter file).
    - `test_commit_step_rejects_new_untracked_files`: call 1 creates `skills/new.md` → exit != 0, stderr names it.
    - `test_commit_step_ignores_preexisting_untracked`: prek exits 0 both times → exit 0; `release-notes.md` is not
      in the commit.

- [ ] **Step 2: Run** `uv run pytest tests/workflows/test_prepare_release_commit.py -v` → the first, third and fourth
      tests fail (the current step ignores prek's exit code and fixes outside the release), and the script extraction
      fails until the step has `id: commit-release`. Add the `id` first so the failures are the real ones.

- [ ] **Step 3: Implement** the step as in the spec's Decision:

    ```sh
    set -euo pipefail
    before=$(git status --porcelain --untracked-files=all | cut -c4- | sort)
    prek run --all-files || true
    after=$(git status --porcelain --untracked-files=all | cut -c4- | sort)
    outside=$(comm -13 <(echo "$before") <(echo "$after"))
    # fail with ::error:: naming $outside if non-empty
    prek run --all-files --show-diff-on-failure
    git commit -am "chore(release): Prepare for ${RELEASE_VERSION} release"
    ```

  Add `env: GH_TOKEN: ${{ steps.app-token.outputs.token || github.token }}` to the step.

- [ ] **Step 4: Run** the Step 2 command → PASS. `uv run pytest tests/action_yml -q` → still PASS.

- [ ] **Step 5: Commit.** `fix(release): fail release prep when a prek hook fails`

### Task 2: Show the committed diff, drop the unused local tag

**Files:**

- Modify: `.github/workflows/_prepare-release.yml` (`Show Changes Diff`, `Tag the release`)

**Interfaces:** none.

- [ ] **Step 1:** Move `Show Changes Diff` below `Commit changes` and change its body to
      `git show --color --stat --patch HEAD`.
- [ ] **Step 2:** Delete the `Tag the release` step. `git grep -n 'RELEASE_VERSION' .github/` → no remaining use
      depends on a local tag (`Create Release Patch` uses `HEAD^`).
- [ ] **Step 3:** `prek run --files .github/workflows/_prepare-release.yml` → actionlint and shellcheck clean.
- [ ] **Step 4: Commit.** `ci(release): show the committed release diff and drop the unused local tag`

### Task 3: Verification, archive

- [ ] **Step 1:** No `UNRELEASED.md` entry (CI only).
- [ ] **Step 2:** `uv run pytest tests/workflows tests/action_yml` → green. `prek run --all-files` → green. No Rust or
      runtime Python changed, so `mise run test` isn't needed.
- [ ] **Step 3:** The PR's own `Prepare Release` job (`ci.yml` calls `_prepare-release.yml` on every PR) runs the new
      step on a dev-version bump. Check its log: the second prek run passes, and `Show Changes Diff` shows the
      commit. To see the failure path, push a throwaway commit with an unused import to the PR and confirm the job
      goes red, then drop the commit.
- [ ] **Step 4:**
      `git mv specs/2026-10-31-release-prek-failures-design.md specs/2026-10-31-release-prek-failures-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive release prek failures design and plan`.
