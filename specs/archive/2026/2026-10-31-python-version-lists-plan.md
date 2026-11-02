# Python Version Lists Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A test fails when any hand-kept Python version list disagrees with `toolr-py`'s `requires-python` (F-38).

**Architecture:** One pytest module derives the supported minors from `requires-python` with `packaging` and checks the
full-range lists and the floor-only settings against them.

**Tech Stack:** pytest, `tomllib`, `packaging.specifiers`.

**Spec:** [`specs/archive/2026/2026-10-31-python-version-lists-design.md`](./2026-10-31-python-version-lists-design.md)

## Global Constraints

- Source of truth: `crates/toolr-py/pyproject.toml` `project.requires-python`.
- `MIN_PYTHON_MINOR` stays covered by `crates/toolr-core/tests/python_floor_lockstep.rs` only.
- Imports top-of-file; factory or module-scoped fixtures, no bare helpers for file setup.
- No schema change, no release note.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`.

## Review Focus

1. **A patch-level `mise.toml` entry** (`3.15.2`) must compare as `3.15`. `test_mise_pythons_match` strips to
   major.minor.
2. **An upper bound that isn't `<3.N`** (for example `<=3.15` or none). `supported_minors` must still give a finite,
   correct list. `test_supported_minors_reads_bounds` covers `>=3.11,<3.16`, `>=3.11,<=3.15` and `>=3.12`.
3. **Duplicates or order** in `ALL_CPYTHONS`. `TEST_PYTHONS_NARROW` takes the last entry as "latest", so assert the
   list is sorted ascending too, in `test_all_cpythons_match`.
4. **A pre-release in mise** (`3.16.0rc1`). Parse the minor from the first two dot-separated fields; don't feed it to
   `Version` blindly.
5. **The test passes trivially if a reader returns empty.** Each full-range test also asserts its list is non-empty.

---

### Task 1: The lockstep test

**Files:**

- Create: `tests/test_python_versions.py`
- Modify: `crates/toolr-py/pyproject.toml` (comment above `requires-python`)
- Modify: `tools/ci.py` (comment above `ALL_CPYTHONS`)
- Modify: `mise.toml` (comment above `python`)

**Interfaces:**

- Produces: `supported_minors(requires_python: str) -> list[int]` (module-level in the test file) and a module-scoped
  fixture `pyprojects() -> dict[str, dict[str, Any]]` keyed `"toolr-py"`, `"toolr"`, `"root"`, `"mise"`, `"cargo"`.

- [ ] **Step 1: Write the tests.** Imports: `tomllib`, `pytest`, `packaging.specifiers.SpecifierSet`,
      `from tools.ci import ABI3_WHEEL_PYTHONS, ALL_CPYTHONS, REPO_ROOT`.
    - `test_supported_minors_reads_bounds`: the three specifiers in Review Focus 2 → `[11..15]`, `[11..15]`,
      `[12..39]`.
    - `test_all_cpythons_match`: `ALL_CPYTHONS == sorted(ALL_CPYTHONS)`, non-empty, and
      `[int(t[3:]) for t in ALL_CPYTHONS] == supported`.
    - `test_classifiers_match`: the `Programming Language :: Python :: 3.N` classifiers of toolr-py, as ints, equal
      `supported`.
    - `test_mise_pythons_match`: `sorted({int(v.split(".")[1]) for v in mise["tools"]["python"]}) == supported`.
    - `test_floor_settings_match`, parametrised over `(label, actual)` pairs, each compared with `min(supported)`:
      pyo3's features in root `Cargo.toml` `workspace.dependencies.pyo3.features` contain `abi3-py3{floor}`;
      `ABI3_WHEEL_PYTHONS == [f"cp3{floor}"]`; toolr-py and toolr `tool.cibuildwheel.build == f"cp3{floor}-*"`;
      toolr's `requires-python` floor via `supported_minors(...)[0]`; ruff `tool.ruff.target-version ==
      f"py3{floor}"`; mypy `tool.mypy.python_version == f"3.{floor}"`.
    - Every assertion message: `"<file> <key> is X; requires-python in crates/toolr-py/pyproject.toml implies Y"`.

- [ ] **Step 2: Run** `uv run pytest tests/test_python_versions.py -v` → PASS (the lists agree today).

- [ ] **Step 3: Prove it bites.** Temporarily drop `"3.11.16"` from `mise.toml`, rerun → `test_mise_pythons_match`
      fails with a message naming `mise.toml`. Temporarily change `abi3-py311` to `abi3-py312` in `Cargo.toml` →
      `test_floor_settings_match[pyo3-abi3]` fails. Revert both (`git diff` clean except the new file).

- [ ] **Step 4: Point the comments at the test.** Replace the "Raising the floor? Also bump ..." comment in toolr-py's
      `pyproject.toml` with: "Adding or dropping a CPython? `tests/test_python_versions.py` lists every place to
      change; `crates/toolr-core/tests/python_floor_lockstep.rs` covers `MIN_PYTHON_MINOR`." In `tools/ci.py`, replace
      "Keep aligned with `requires-python` ... bump when a CPython is added/removed." with "Checked against
      `requires-python` by `tests/test_python_versions.py`." Add the same sentence to the `mise.toml` python comment.

- [ ] **Step 5: Commit.** `test(ci): check every Python version list against requires-python`

### Task 2: Verification and archive

- [ ] **Step 1:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green.
- [ ] **Step 2:** `git mv specs/2026-10-31-python-version-lists-design.md
      specs/2026-10-31-python-version-lists-plan.md specs/archive/2026/`; fix the plan's `Spec:` link and set the
      design's status to implemented. Commit `docs(specs): archive python version lists design and plan`.
