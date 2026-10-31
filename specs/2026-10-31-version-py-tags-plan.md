# Version Py Tags Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A pre-release tag no longer breaks dev-version computation, and `--new-version` is normalised and validated
before `toolr version bump` writes anything (F-26).

**Architecture:** `_compute_dev_version` parses `git describe` output with one regex and bumps the numeric core.
`bump` passes an explicit version through `_normalise_release_version` before any side effect.

**Tech Stack:** Python 3.11+, pytest, `unittest.mock`.

**Spec:** [`specs/2026-10-31-version-py-tags-design.md`](./2026-10-31-version-py-tags-design.md)

## Global Constraints

- Only `tools/version.py` and `tests/test_tools_version.py` change.
- Reuse the module's `_FakeRun` / `_ctx_with_run` stubs. For paths that call `ctx.exit`, set
  `ctx.exit.side_effect = SystemExit` so execution stops there, as the real `Context.exit` does.
- `GITHUB_OUTPUT`, `GITHUB_ENV` and `GITHUB_STEP_SUMMARY` go to `tmp_path` files via `monkeypatch.setenv`;
  `monkeypatch.delenv` them where a test doesn't need them.
- Imports at the top of the test module. Conventional Commits; no `Co-Authored-By`. Stage by path.

## Review Focus

1. **The rc-shaped describe output.** Task 1, `test_compute_dev_version_after_prerelease_tag`, uses the exact
   `v1.0.0-rc1-3-gabc1234` shape from the finding.
2. **Nothing written before validation.** Task 2, `test_bump_rejects_malformed_version_before_writing`: the
   `GITHUB_OUTPUT` file stays empty and `ctx.run` is never called.
3. **The `v` slip checks the right tag.** Task 2, `test_bump_strips_leading_v`: the `git tag -l` call uses `v0.36.0`,
   not `vv0.36.0`.

---

### Task 1: Parse pre-release describe output

**Files:**

- Modify: `tools/version.py` (`_compute_dev_version`, new `_DESCRIBE_RE`)
- Test: `tests/test_tools_version.py`

**Interfaces:**

- `_DESCRIBE_RE: Final[re.Pattern[str]]` as in the spec.
- [ ] **Step 1: Write the tests:**
    - `test_compute_dev_version_after_prerelease_tag`: `_FakeRun(["v1.0.0-rc1-3-gabc1234\n"])`, no
      `GITHUB_EVENT_NAME` → `"1.0.1-dev3"`.
    - `test_compute_dev_version_after_hyphenated_prerelease_tag`: `"v1.0.0-rc-1-3-gabc1234\n"` → `"1.0.1-dev3"`.
    - `test_compute_dev_version_prerelease_tag_pull_request`: `GITHUB_EVENT_NAME=pull_request`,
      `"v1.0.0-rc.2-5-gdeadbee\n"` → `"1.0.1-dev5+gdeadbee"`.
    - `test_compute_dev_version_unparsable_describe`: `"garbage\n"`, `ctx.exit.side_effect = SystemExit` →
      `pytest.raises(SystemExit)`; `ctx.error` called once with a message containing `garbage`.
- [ ] **Step 2: Run** `uv run pytest tests/test_tools_version.py -v -k compute_dev_version` → the four new tests FAIL
      (`ValueError: too many values to unpack`, or `not enough values` for `garbage`); the existing ones PASS.
- [ ] **Step 3: Implement.** Replace the `split("-")` with `_DESCRIBE_RE.fullmatch(describe)`; on no match,
      `ctx.error(f"cannot parse git describe output {describe!r}")` and `ctx.exit(1)`. Bump from `core`. Update the
      format comment above it.
- [ ] **Step 4: Run** Step 2 → PASS.
- [ ] **Step 5: Commit.** `fix(release): derive dev versions after pre-release tags`

### Task 2: Normalise and validate `--new-version`

**Files:**

- Modify: `tools/version.py` (`bump`, new `_SEMVER_RE`, `_normalise_release_version`)
- Test: `tests/test_tools_version.py`

**Interfaces:**

- `_SEMVER_RE: Final[re.Pattern[str]]` as in the spec.
- `_normalise_release_version(raw: str) -> str`: `raw.removeprefix("v")`, then `fullmatch` or
  `ValueError(f"{raw!r} is not a semantic version (expected X.Y.Z)")`.
- [ ] **Step 1: Write the tests:**
    - `test_normalise_release_version` (parametrised): `"0.36.0"` → `"0.36.0"`; `"v0.36.0"` → `"0.36.0"`;
      `"1.0.0-rc1"` → `"1.0.0-rc1"`; `"1.0.0-rc.1+build.5"` unchanged.
    - `test_normalise_release_version_rejects` (parametrised): `"0.36"`, `"vv0.36.0"`, `"0.36.0.1"`, `"latest"`, `""`,
      `"0.36.0-"` → `pytest.raises(ValueError)`.
    - `test_bump_strips_leading_v`: `_FakeRun([""])` (no existing tag), `GITHUB_OUTPUT`/`GITHUB_ENV` in `tmp_path`;
      `bump(ctx, new_version="v0.36.0", check_existing_tag=True)` → `run.calls[0] == ("git", "tag", "-l",
      "v0.36.0")`; the output file contains `release-version=0.36.0` and `release-patch-name=toolr-0.36.0.patch`;
      the env file contains `RELEASE_VERSION=0.36.0`.
    - `test_bump_rejects_malformed_version_before_writing`: `bump(ctx, new_version="0.36", check_existing_tag=True,
      write=True)` with `ctx.exit.side_effect = SystemExit` → raises `SystemExit`; `ctx.exit` called with `1`;
      `run.calls == []`; the `GITHUB_OUTPUT` and `GITHUB_ENV` files are empty or absent.
- [ ] **Step 2: Run** `uv run pytest tests/test_tools_version.py -v -k "normalise or bump_"` → all new tests FAIL
      (`ImportError` for the helper; `vv0.36.0` in the tag call; the output file written).
- [ ] **Step 3: Implement.** At the top of `bump`, if `new_version` is set, normalise it inside `try`/`except
      ValueError` → `ctx.error(str(exc))`, `ctx.exit(1)`. Update the `new_version` docstring line: "A leading `v` is
      accepted and stripped."
- [ ] **Step 4: Run** Step 2 → PASS. `uv run pytest tests/test_tools_version.py` → all PASS. Update the module
      docstring of the test file, which lists the covered surface, to include `bump` and
      `_normalise_release_version`.
- [ ] **Step 5: Commit.** `fix(release): validate and normalise --new-version before writing outputs`

### Task 3: Verification, archive

- [ ] **Step 1:** No `UNRELEASED.md` entry (release tooling only).
- [ ] **Step 2:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green (ruff, mypy).
- [ ] **Step 3:** The PR's own `Prepare Release` job runs `toolr version bump` with a dev version: check it still
      writes `X.Y.Z-devN+gSHA`.
- [ ] **Step 4:**
      `git mv specs/2026-10-31-version-py-tags-design.md specs/2026-10-31-version-py-tags-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive version py tags design and plan`.
