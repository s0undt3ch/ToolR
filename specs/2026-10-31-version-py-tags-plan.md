# Version Py Tags Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pre-release versions can be released, a pre-release tag no longer breaks dev-version computation, and
`--new-version` is normalised and validated before `toolr version bump` writes anything (F-26).

**Architecture:** `tools/version.py` parses every version (explicit input and latest tag) with
`packaging.version.Version` and renders the SemVer form Cargo needs from the parsed parts. The dev version comes from
three git commands instead of parsed `git describe` output. A `release-prerelease` output drives `--prerelease` on
the GitHub release.

**Tech Stack:** Python 3.11+, `packaging`, pytest, real git repositories in `tmp_path`, bash for the workflow step.

**Spec:** [`specs/2026-10-31-version-py-tags-design.md`](./2026-10-31-version-py-tags-design.md)

## Global Constraints

- Tests use real git repositories built by factory fixtures in `tmp_path`, and a real `Context` from
  `toolr.testing.make_context`; git is not mocked. `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_NOSYSTEM` are set with
  `monkeypatch.setenv` so the developer's git config doesn't leak in.
- `GITHUB_OUTPUT`, `GITHUB_ENV` and `GITHUB_STEP_SUMMARY` go to `tmp_path` files via `monkeypatch.setenv`;
  `monkeypatch.delenv` them where a test doesn't need them.
- Imports at the top of the test module. Conventional Commits; no `Co-Authored-By`. Stage by path.
- Don't touch `action.yml` (another open PR edits it).

## Review Focus

1. **The reported tag shape.** `test_compute_dev_version_after_prerelease_tag` tags a real repository `v1.0.0-rc1`
   and expects `1.0.0-rc.2.dev3`.
2. **Nothing written before validation.** `test_bump_rejects_malformed_version_before_writing`: the `GITHUB_OUTPUT`,
   `GITHUB_ENV` and `GITHUB_STEP_SUMMARY` files stay absent.
3. **The `v` slip checks the right tag.** `test_bump_strips_leading_v` and
   `test_bump_leading_v_finds_existing_tag`: with `v0.36.0` tagged, `--new-version v0.36.0 --check-existing-tag`
   exits 1.
4. **Wheel versions.** `test_cargo_version_maps_back_to_pep440`: each rendered Cargo version normalises back to the
   PEP 440 version it came from.

---

### Task 1: Parse and render versions with `packaging`

**Files:** `tools/version.py`, `tools/pyproject.toml`, `tools/uv.lock`, `tests/test_tools_version.py`

- [ ] **Step 1: Tests.**
    - `_parse_release_version` accepts `0.36.0`, `v0.36.0`, `1.0.0-rc1`, `1.0.0rc1`, `1.0.0-rc.1`, `1.0.0-alpha.2`;
      rejects `0.36`, `0.36.0.1`, `vv0.36.0`, `latest`, `""`, `1.0.0-1` (post), `1.0.0.dev1`, `1!1.0.0`,
      `1.0.0+local`.
    - `_cargo_version` renders the spec's table, and `str(Version(_cargo_version(v))) == str(v)` for each row.
- [ ] **Step 2:** Run them → FAIL (missing helpers).
- [ ] **Step 3:** Add `packaging` to `tools/pyproject.toml`, `uv lock --project tools`. Implement `_parse_version`,
      `_parse_release_version`, `_cargo_version`. Remove `_bump_patch`.
- [ ] **Step 4:** Run → PASS.

### Task 2: Dev versions from three git commands

**Files:** `tools/version.py`, `tests/test_tools_version.py`

- [ ] **Step 1: Tests** (real repositories, `monkeypatch.chdir`):
    - final tag `v0.20.0` + 2 commits → `0.20.1-dev2`; with `GITHUB_EVENT_NAME=pull_request` →
      `0.20.1-dev2+g<sha>`.
    - tag `v1.0.0-rc1` + 3 commits → `1.0.0-rc.2.dev3`; same with `v1.0.0-rc.1`, and with `v1.0.0-beta.1` →
      `1.0.0-beta.2.dev3`.
    - no tag, 1 commit → `<TODAY_VERSION patch+1>-dev1`.
    - tag `v1.2.3-foo` (matches the glob, not PEP 440) → `SystemExit`, error names the tag.
- [ ] **Step 2:** Run → the rc and bad-tag tests FAIL (`ValueError: too many values to unpack`).
- [ ] **Step 3:** Rewrite `_compute_dev_version` per the spec.
- [ ] **Step 4:** Run → PASS.

### Task 3: Validate `--new-version` in `bump`, emit `release-prerelease`

**Files:** `tools/version.py`, `tests/test_tools_version.py`

- [ ] **Step 1: Tests:**
    - `bump(new_version="v0.36.0", check_existing_tag=True)` in a repo without that tag → `release-version=0.36.0`,
      `release-patch-name=toolr-0.36.0.patch`, `release-prerelease=false`, `RELEASE_VERSION=0.36.0`.
    - the same with `v0.36.0` tagged → `SystemExit`, nothing written.
    - `new_version="1.0.0rc1"` → `release-version=1.0.0-rc.1`, `release-prerelease=true`.
    - `new_version` in `0.36`, `latest`, `1.0.0-1` with `write=True` → `SystemExit` with code 1, no output, env or
      summary file created.
    - no `new_version` in a repo tagged `v1.0.0-rc1` → prints the dev version, `release-prerelease=true`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement; update the `new_version` docstring.
- [ ] **Step 4:** Run → PASS.

### Task 4: Pre-release GitHub releases

**Files:** `.github/workflows/_prepare-release.yml`, `.github/workflows/release.yml`,
`tests/workflows/test_release_create.py`

- [ ] **Step 1: Test:** extract the `create-release` step from `release.yml`, run it with a stub `gh` on `PATH` that
      records its arguments. `RELEASE_PRERELEASE=true` → `--prerelease` present, `--latest` absent; `false` → the
      reverse.
- [ ] **Step 2:** Run → FAIL (no step with id `create-release`).
- [ ] **Step 3:** Expose `release-prerelease` from `_prepare-release.yml` (job and `workflow_call` outputs); give the
      step `id: create-release`, pass `RELEASE_PRERELEASE`, choose the flag in bash.
- [ ] **Step 4:** Run → PASS. Commit Tasks 1–4 as `fix(release): support pre-release versions in version bump`.

### Task 5: Round-1 review: tag spellings and pre-release rollup

**Files:** `tools/version.py`, `cliff.toml`, `.github/workflows/_prepare-release.yml`, `tests/test_tools_version.py`,
`tests/workflows/test_prepare_release_prerelease.py`

- [ ] **Step 1: Tests:** `--new-version 1.0.0-rc1` with an existing `v1.0.0-rc1`, `v1.0.0rc1` or `v1.0.0-rc.1` tag →
      `SystemExit`; a dev build after a final tag → `release-prerelease=false`; the `update-changelog` and
      `clear-release-notes` steps are gated on `release-prerelease`; `cliff.toml`'s `tag_pattern` matches final tags
      only; with git-cliff available, `--unreleased --tag 1.0.0` after `v1.0.0-rc.1` lists the commits from both sides
      of the rc. Dev-version tests use the canonical `v1.0.0-rc.1`, keeping one hand-made `v1.0.0-rc1` case.
- [ ] **Step 2:** Implement per the spec. Commit `fix(release): roll pre-releases up into the final release`.

### Task 6: Release note, verification, archive

- [ ] **Step 1:** `toolr release-notes add --slug version-py-tags --text "…"`; commit
      `docs(release-notes): …`. `toolr release-notes check --base origin/main` → exit 0.
- [ ] **Step 2:** `uv run pytest`, `prek run --all-files` → green. Coverage of the changed lines in
      `tools/version.py` ≥ 99%.
- [ ] **Step 3:**
      `git mv specs/2026-10-31-version-py-tags-design.md specs/2026-10-31-version-py-tags-plan.md specs/archive/2026/`;
      set the design's status to implemented. Commit `docs(specs): archive version py tags design and plan`.
