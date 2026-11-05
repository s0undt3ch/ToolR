# Derived Python Versions and Fast Hook Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `ALL_CPYTHONS` comes from `requires-python` (FU-5), and the version-list checks run as a prek hook that
fires only when their inputs change (FU-4).

**Architecture:** `tools/ci.py` owns the `requires-python` parser and derives `ALL_CPYTHONS` at import. A new
`tools/precommit/python_versions.py` owns the checks and backs `toolr pre-commit check-python-versions`.
`tests/test_python_versions.py` tests both.

**Tech Stack:** toolr command groups, `tomllib`, `packaging.specifiers`, pytest, prek.

**Spec:** [`specs/2026-11-04-python-versions-derive-design.md`](./2026-11-04-python-versions-derive-design.md)

## Global Constraints

- Source of truth: `crates/toolr-py/pyproject.toml` `project.requires-python`.
- `tools/` runs in the tools venv (toolr-py from PyPI, `packaging>=24`). Add no dependency there.
- `ABI3_WHEEL_PYTHONS`, `BINARY_WHEEL_PYTHONS`, `mise.toml` and the classifiers stay hand-kept.
- Imports top-of-file; factory fixtures for `tmp_path` trees.
- Conventional Commits; no `Co-Authored-By`; stage by path, never `git add -A`; `Release-Notes: skip` trailer.
- Format touched files only; no whole-crate formatting.

## Review Focus

1. **Import-time read from another cwd.** `toolr ci generate-build-matrix` run from `crates/` must still find the
   pyproject. `REPO_ROOT` is `Path(__file__).resolve().parents[1]`; don't use a relative path.
2. **The static manifest parser** must still list `toolr ci` commands with a module-level function call in
   `tools/ci.py`. Check `toolr ci --help` after Task 1.
3. **Temp-copy tests must use the copy.** `python_version_mismatches(root)` must read every input relative to `root`,
   including deriving `supported` from `root`'s toolr-py pyproject, not from `tools.ci.ALL_CPYTHONS`.
4. **The hook's `files` regex** must list exactly the checker's inputs. The wiring test builds the expected paths
   from `CONFIG_TOML_FILES` and `CP_TAG_WORKFLOWS`, so a new input without a regex entry fails.
5. **Empty reads.** Keep the "admits no CPython 3 minor" and non-empty guards, so a missing key can't pass silently.

---

### Task 1: Derive `ALL_CPYTHONS` (FU-5)

**Files:**

- Modify: `tools/ci.py`
- Modify: `tests/test_python_versions.py`

**Interfaces:**

- Produces in `tools.ci`: `TOOLR_PY_PYPROJECT: Path`, `python3_floor(requires_python: str) -> int`,
  `supported_minors(requires_python: str) -> list[int]`, `_requires_python(pyproject: Path) -> str`.
- [ ] **Step 1: Move the parsers.** Cut `python3_floor` and `supported_minors` from the test module into `tools/ci.py`
      above `ALL_CPYTHONS`, with their imports (`tomllib`, `SpecifierSet`, `Version`). Keep the comment pointing at
      `python_floor_lockstep.rs`.
- [ ] **Step 2: Derive.** Replace the literal with
      `ALL_CPYTHONS = [f"cp3{m}" for m in supported_minors(_requires_python(TOOLR_PY_PYPROJECT))]`. Reword its comment
      to "Derived from `requires-python` in `crates/toolr-py/pyproject.toml`."
- [ ] **Step 3: Update the tests.** Import `python3_floor` and `supported_minors` from `tools.ci`; delete
      `test_all_cpythons_match`. Add a case asserting `_requires_python` reads `project.requires-python` from a
      `tmp_path` TOML.
- [ ] **Step 4: Run.** `uv run pytest tests/test_python_versions.py tests/test_tools_ci_packslip.py -v` → PASS.
      `toolr ci --help` lists the commands. From `crates/`, `toolr ci generate-build-matrix` prints `test-pythons`
      `["3.11", …, "3.15"]`.
- [ ] **Step 5: Commit.** `refactor(ci): derive ALL_CPYTHONS from requires-python`

### Task 2: Move the checks into a pre-commit command (FU-4)

**Files:**

- Create: `tools/precommit/python_versions.py`
- Modify: `tests/test_python_versions.py`

**Interfaces:**

- Consumes: `tools.ci` `REPO_ROOT`, `ABI3_WHEEL_PYTHONS`, `BINARY_WHEEL_PYTHONS`, `python3_floor`, `supported_minors`.
- Produces: `CONFIG_TOML_FILES`, `CP_TAG_WORKFLOWS`, `python_version_mismatches(repo_root: Path) -> list[str]`, and the
  `check_python_versions(ctx)` command on the `pre-commit` group (`from ._common import group`).
- [ ] **Step 1: Write the failing tests.** In the test module:
    - `test_repo_python_versions_agree`: `python_version_mismatches(REPO_ROOT) == []`.
    - A factory fixture `make_python_versions_tree(tmp_path)` that copies every `CONFIG_TOML_FILES` value and
      `CP_TAG_WORKFLOWS` entry into `tmp_path`, preserving relative paths, and returns the root.
    - `test_mismatch_reported`, parametrised over `(path, old, new, expected_fragment)`: drop the `"3.11.16"` entry from
      `mise.toml`; drop the `3.15` classifier; `abi3-py311` → `abi3-py312` in `Cargo.toml`; `cptag="cp311"` →
      `cptag="cp312"` in `_test.yml`. Assert exactly one message, and that it contains the fragment (file and key).
    - Run `uv run pytest tests/test_python_versions.py -v` → FAIL (`ImportError`).
- [ ] **Step 2: Move the checks.** Port `test_classifiers_match`, `test_mise_pythons_match`, `_floor_settings`,
      `test_floor_settings_match` and `test_requires_python_floors_match` into `python_version_mismatches`, each
      appending a message instead of asserting. Move `CONFIG_TOML_FILES`, `CP_TAG_WORKFLOWS`, `CP_TAG_PATTERN`,
      `COMMENT_PATTERN` and `FloorSetting` with them. Delete the moved tests and the module-scoped fixtures.
- [ ] **Step 3: Add the command.** `@group.command def check_python_versions(ctx: Context) -> None:` with a docstring;
      calls `python_version_mismatches(REPO_ROOT)`, `ctx.error`s each message, then `ctx.exit(1)` if any.
- [ ] **Step 4: Run.** `uv run pytest tests/test_python_versions.py -v` → PASS. `toolr pre-commit
      check-python-versions` → exit 0.
- [ ] **Step 5: Commit.** `refactor(ci): check Python version lists from a toolr pre-commit command`

### Task 3: Wire the prek hook

**Files:**

- Modify: `.pre-commit-config.yaml`
- Modify: `tests/test_python_versions.py`
- Modify: `crates/toolr-py/pyproject.toml`, `mise.toml` (pointer comments)
- [ ] **Step 1: Write the failing wiring test.** `test_hook_files_cover_inputs`: load `.pre-commit-config.yaml` with
      `yaml.safe_load`, find the local hook with `id == "check-python-versions"`, compile its `files` (verbose
      regex). Assert it matches every `CONFIG_TOML_FILES` value, every `CP_TAG_WORKFLOWS` entry, `tools/ci.py`,
      `tools/precommit/python_versions.py` and `tests/test_python_versions.py`, and doesn't match `README.md`. Run →
      FAIL (hook missing).
- [ ] **Step 2: Add the hook** to the first `repo: local` block, after `check-pipeline-gate-needs`:
      `entry: toolr pre-commit check-python-versions`, `language: system`, `pass_filenames: false`, a `(?x)` `files`
      regex listing the inputs, and a two-line comment saying why it exists (catches drift before the wheel build).
- [ ] **Step 3: Repoint comments.** The `requires-python` comment in toolr-py's `pyproject.toml` and the
      `mise.toml` python comment name `toolr pre-commit check-python-versions` instead of
      `tests/test_python_versions.py`.
- [ ] **Step 4: Prove it.** `prek run check-python-versions --files mise.toml` → Passed. `prek run
      check-python-versions --files README.md` → Skipped. Temporarily drop `"3.11.16"` from `mise.toml`, rerun the
      first → Failed, naming `mise.toml`; revert (`git diff` shows only intended files).
- [ ] **Step 5: Run.** `uv run pytest tests/test_python_versions.py -v` → PASS.
- [ ] **Step 6: Commit.** `ci(pre-commit): run the Python version checks as a prek hook`

### Task 4: Verification and archive

- [ ] **Step 1:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green.
- [ ] **Step 2:** `toolr release-notes check --base main` passes with the `Release-Notes: skip` trailer on a commit.
- [ ] **Step 3:** `git mv specs/2026-11-04-python-versions-derive-design.md
      specs/2026-11-04-python-versions-derive-plan.md specs/archive/2026/`; fix the relative links and set the
      design's status to implemented. Commit `docs(specs): archive derived Python versions design and plan`.
