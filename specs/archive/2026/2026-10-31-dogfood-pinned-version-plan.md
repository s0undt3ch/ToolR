# Dogfood Pinned Version Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** CI dogfoods the same toolr binary version as `mise.toml` and the `toolr-py` in `tools/uv.lock`, so a release
that bumps the runner schema can't turn CI red before Renovate's bump lands (F-25).

**Architecture:** One `TOOLR_VERSION` pin in `.github/env`, read by every `uses: ./` call (through a `toolr-version`
input for the reusable `_prepare-release.yml`), kept in Renovate's "toolr release" group and checked by a pytest
consistency test.

**Tech Stack:** GitHub Actions YAML, Renovate config (JSON5), pytest, PyYAML, `tomllib`.

**Spec:** [`specs/archive/2026/2026-10-31-dogfood-pinned-version-design.md`](./2026-10-31-dogfood-pinned-version-design.md)

## Global Constraints

- Stacked on `fix/release-tree-binding`. Don't touch `_prepare-release.yml`'s commit steps (F-24) or `release.yml`'s
  `publish-release` job (F-10).
- Don't change `action.yml` or its `inputs.version.default`.
- Imports at the top of test modules.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`.

## Review Focus

1. **No call left on `latest`.** The test walks every workflow, not a hard-coded list. Task 2,
   `test_in_repo_action_calls_use_the_pin`.
2. **Empty pin can't fall back to `latest`.** The action treats an empty `version` as `latest`. The test asserts
   `.github/env` has a non-empty `TOOLR_VERSION`; the `_prepare-release.yml` input is `required: true`.
3. **Renovate groups the new dep with the other two.** Check the edited rule's `matchManagers`, `matchFileNames` and
   `matchPackageNames` all admit the custom manager's dep.

---

### Task 1: Reproduce the mismatch

This finding is PLAUSIBLE. This task decides whether it goes on.

**Files:** none committed.

- [ ] **Step 1:** Write a valid runner spec with `"schema_version": 4` to a scratch `spec.json` outside the repo
      (dump one from a `tests/runner/` fixture and edit the number), and run, from the repo root,
      `tools/.venv/bin/python -c 'from toolr._runner import load_spec; load_spec("<scratch>/spec.json")'` against the
      tools venv (`toolr-py` 0.35.1 from `tools/uv.lock`; `toolr project venv sync` first if it doesn't exist).
- [ ] **Step 2:** Expected: `SpecError`. toolr-py 0.35.1 predates the schema-first check, so the text may be
      "failed schema validation" rather than "speaks schema 3, but the toolr binary emitted schema 4"; either aborts
      dispatch. **Close
      criteria:** if `load_spec` returns without error, a newer binary can't break the frozen venv this way. Close
      the finding, skip the remaining tasks and note the result in the PR.

### Task 2: Pin test

**Files:**

- Create: `tests/test_dogfood_toolr_pin.py`

**Interfaces:**

- `REPO_ROOT = Path(__file__).resolve().parents[1]`
- `_github_env_pin() -> str`: the `TOOLR_VERSION=` value in `.github/env`; fails the test if missing or empty.
- `_in_repo_action_calls() -> list[tuple[str, str, dict]]`: `(workflow file, job id, step)` for every step in
  `.github/workflows/*.yml` whose `uses` is `./`.
- [ ] **Step 1: Write the tests:**
    - `test_toolr_pin_matches_mise_and_lock`: `_github_env_pin()` == `tomllib` `mise.toml`
      `["tools"]["aqua:s0undt3ch/ToolR"]` == the `version` of the `[[package]]` named `toolr-py` in `tools/uv.lock`.
    - `test_in_repo_action_calls_use_the_pin`: `_in_repo_action_calls()` is non-empty, and every step's
      `with.version` is `${{ env.TOOLR_VERSION }}`, except in `_prepare-release.yml`, where it is
      `${{ inputs.toolr-version }}`. The failure message lists offending `file:job`.
    - `test_prepare_release_requires_toolr_version`: `_prepare-release.yml`'s `on.workflow_call.inputs.toolr-version`
      has `required: true`, and each caller (`ci.yml`, `release.yml`) passes
      `toolr-version: ${{ needs.prepare-ci.outputs.toolr-version }}`. (PyYAML loads the `on` key as `True`; read
      `data[True]`.)
- [ ] **Step 2: Run** `uv run pytest tests/test_dogfood_toolr_pin.py -v` → all three FAIL (no pin, six `latest`, no
      input).
- [ ] **Step 3: Commit** nothing yet; Task 3 makes them pass in the same commit.

### Task 3: Wire the pin

**Files:**

- Modify: `.github/env`, `.github/workflows/ci.yml`, `.github/workflows/release.yml`,
  `.github/workflows/_prepare-release.yml`

**Interfaces:**

- `_prepare-release.yml` `on.workflow_call.inputs.toolr-version`: `required: true`, `type: string`, description
  "toolr binary version to dogfood. Pass the caller workflow's TOOLR_VERSION env var."
- `ci.yml` and `release.yml` `prepare-ci.outputs.toolr-version`, set by a step `id: toolr-version` that runs
  `echo "toolr-version=$TOOLR_VERSION" >> "$GITHUB_OUTPUT"`, next to the existing `mise-version` step.
- [ ] **Step 1:** `.github/env`: add `TOOLR_VERSION=0.35.1` (the current `mise.toml` value) with a comment that
      Renovate's "toolr release" group bumps it with `mise.toml` and `tools/uv.lock`.
- [ ] **Step 2:** Replace `version: latest` with `version: ${{ env.TOOLR_VERSION }}` in the five top-level calls, and
      `version: ${{ inputs.toolr-version }}` in `_prepare-release.yml`. Add the input, the outputs and the callers'
      `toolr-version:` lines. Update the `docs` job comment to say the pin keeps binary and venv aligned.
- [ ] **Step 3: Run** `uv run pytest tests/test_dogfood_toolr_pin.py -v` → PASS. `prek run --all-files` → actionlint,
      `check-pipeline-gate-needs` and `pin-github-actions` green.
- [ ] **Step 4: Commit** the test and the workflow changes together.
      `ci: dogfood the pinned toolr version instead of latest`

### Task 4: Renovate

**Files:**

- Modify: `renovate.json5`

**Interfaces:** none.

- [ ] **Step 1:** Add the `customManagers` entry from the spec, next to the `MISE_VERSION` one.
- [ ] **Step 2:** In the "Track our own toolr release in mise / GH Actions ASAP" rule, add `"custom.regex"` to
      `matchManagers` and `".github/env"` to `matchFileNames`. Update the rule's comment.
- [ ] **Step 3:** `npx --yes --package renovate -- renovate-config-validator renovate.json5` → valid (if `npx` isn't
      available, rely on Renovate's own config-validation check on the PR).
- [ ] **Step 4: Commit.** `chore(renovate): bump the CI toolr pin with the toolr release group`

### Task 5: Verification, archive

- [ ] **Step 1:** No `UNRELEASED.md` entry (CI only).
- [ ] **Step 2:** `uv run pytest tests/test_dogfood_toolr_pin.py tests/workflows tests/action_yml` → green.
      `prek run --all-files` → green.
- [ ] **Step 3:** The PR's own CI runs all five `ci.yml`-reachable calls (`prepare-ci`, `pre-commit`,
      `packslip-check`, `docs`, and `prepare-release` through the input). Check each `Setup ToolR` log says it
      installed `0.35.1`, not "resolved 'latest'". After merge, the Dependency Dashboard lists `s0undt3ch/ToolR` under
      `.github/env`, and the next "toolr release" PR changes all three files.
- [ ] **Step 4:**
      `git mv specs/2026-10-31-dogfood-pinned-version-design.md specs/2026-10-31-dogfood-pinned-version-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive dogfood pinned version design and plan`.
