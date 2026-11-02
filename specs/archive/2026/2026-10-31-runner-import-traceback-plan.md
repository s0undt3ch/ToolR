# Runner Import Traceback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A module-level import failure in a command module prints its traceback, and the venv-sync hint no longer
appears for the repo's own `tools` package (F-28).

**Architecture:** `run()`'s `except SpecError` arm prints `exc.__cause__`'s traceback when it is an `ImportError`.
`_print_missing_dep_hint` skips `tools` and `tools.*` names, which covers both `ImportError` arms.

**Tech Stack:** Python 3.11+, pytest.

**Spec:** [`specs/archive/2026/2026-10-31-runner-import-traceback-design.md`](./2026-10-31-runner-import-traceback-design.md)

**Stacks on:** `fix/runner-type-hints` (F-03). Its changes to `_coerce_args` and `toolr/utils/_annotations.py` are
already in this branch; nothing here touches them.

## Global Constraints

- No runner spec change, so no `RUNNER_SCHEMA_VERSION`/`SCHEMA_VERSION` bump.
- Exit codes don't change: 2 for the `SpecError` arm, 1 for the in-body `ImportError` arm.
- stderr order for a wrapped import failure: traceback, then `toolr runner: failed to import <module>: <message>`, then
  the hint when it applies.
- First-party rule: `name == "tools" or name.startswith("tools.")` → no hint. `name is None` → generic hint, as now.
- Python tests: factory fixtures for `tmp_path` packages, imports at the top of the file. Each test that imports a
  `tools` package restores `sys.path` and pops every `tools`/`tools.*` key from `sys.modules`.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`.

## Review Focus

1. **The exact reported shape:** `tools/demo.py` does `from tools._helpers import load`, and `tools/_helpers.py` does
   `import <missing third-party>` at line 1. stderr names `_helpers.py", line 1` and still has the hint. Task 1,
   `test_run_prints_traceback_for_transitive_import_failure`.
2. **A typo'd local import** (`from tools._helprs import load`) prints the traceback and no `toolr project venv sync`.
   Task 1, `test_run_omits_sync_hint_for_missing_tools_submodule`.
3. **A circular import** between two `tools/*.py` modules gets no hint. Task 1,
   `test_run_omits_sync_hint_for_circular_tools_import`.
4. **The in-body arm follows the same rule.** Task 1, `test_run_omits_sync_hint_for_in_body_tools_import`.
5. **Existing tests stay green unchanged**, including the hand-built `ImportError` ones whose exception has no
   traceback (`print_exception` prints one line for those).

---

### Task 1: Print the import traceback and gate the hint

**Files:**

- Modify: `crates/toolr-py/python/toolr/_runner.py` (`run()`'s `except SpecError` arm, `_print_missing_dep_hint`)
- Test: `tests/runner/test_runner_coverage.py`

**Interfaces:**

- `_print_missing_dep_hint(exc: ImportError, stream: Any) -> None`: unchanged signature. Returns early when
  `_is_repo_tools_module(exc.name)`.

- New `_is_repo_tools_module(name: str | None) -> bool`: `name is not None and (name == "tools" or
  name.startswith("tools."))`.

- [ ] **Step 1: Write the tests** in `tests/runner/test_runner_coverage.py`, next to the existing missing-dep hint tests.
  Add a factory fixture `make_tools_package(tmp_path)` → `(files: dict[str, str]) -> Path`. It writes
  `tmp_path/tools/__init__.py` plus each `files` entry (dedented) under `tmp_path/tools/`, and returns `tmp_path` for
  use as `repo_root`. On teardown it restores `sys.path[:]` and pops `tools` and every `tools.*` key from `sys.modules`,
  following `tests/runner/test_current_context.py::test_run_sets_the_contextvar_to_the_context_it_builds`. Each test
  calls `_run_isolated(_runner_spec(module="tools.demo", function="hello", repo_root=repo))` and reads
  `capsys.readouterr().err`. Use `toolr_f28_missing_dep` as the missing third-party name.
    - `test_run_prints_traceback_for_transitive_import_failure` (Review Focus 1): exit code 2;
      `"Traceback (most recent call last)" in err`; `'_helpers.py", line 1' in err`;
      `"ModuleNotFoundError: No module named 'toolr_f28_missing_dep'" in err`;
      `err.index("Traceback") < err.index("toolr runner: failed to import tools.demo")`; `"`toolr_f28_missing_dep`"`
      and `"toolr project venv sync"` in `err`.
    - `test_run_omits_sync_hint_for_missing_tools_submodule` (Review Focus 2): `demo.py` does
      `from tools._helprs import load` → exit code 2; `'demo.py", line' in err`; `"toolr project venv sync" not in err`.
    - `test_run_omits_sync_hint_for_circular_tools_import` (Review Focus 3): `demo.py` does `from tools.a import x`,
      `a.py` does `from tools.b import y` then `x = 1`, `b.py` does `from tools.a import x` then `y = 2` → exit code 2;
      `"partially initialized module" in err`; `"toolr project venv sync" not in err`.
    - `test_run_omits_sync_hint_for_in_body_tools_import` (Review Focus 4): monkeypatch `_import_target` to return a
      target that raises `ImportError("No module named 'tools.nope'", name="tools.nope")` → exit code 1;
      `"toolr project venv sync" not in err`.
    - `test_is_repo_tools_module` parametrised: `"tools"` → True, `"tools.x.y"` → True, `"toolsmith"` → False,
      `"yaml"` → False, `None` → False.

- [ ] **Step 2: Run them and confirm they fail.**
      `uv run pytest tests/runner/test_runner_coverage.py -k "traceback or repo_tools or omits_sync_hint"` → the
      traceback and hint assertions fail; `test_is_repo_tools_module` fails on the import.

- [ ] **Step 3: Implement.** Add `_is_repo_tools_module` and the early return in `_print_missing_dep_hint`. In the
      `except SpecError` arm, when `isinstance(exc.__cause__, ImportError)`, call
      `traceback.print_exception(exc.__cause__, file=sys.stderr)` before the `toolr runner:` print, then the hint as
      now. Update the arm's comment to say why the traceback is printed. Keep the `# noqa: T201` on the print.

- [ ] **Step 4: Run** the Step 2 command → PASS, then `uv run pytest tests/runner` → PASS. The existing
      `test_run_emits_missing_dep_hint_for_transitive_importerror_via_spec_error`,
      `test_run_emits_missing_dep_hint_for_function_body_importerror` and
      `test_run_falls_back_to_generic_hint_when_importerror_has_no_name` must pass unchanged.

- [ ] **Step 5: Commit.** `fix(runner): show the traceback when a command module fails to import`

**As implemented.** Review added three changes to Task 1:

- `_import_target` calls `__import__(spec.module)` instead of `importlib.import_module`, so the printed traceback has
  no `<frozen importlib._bootstrap>` frames. It then reads `sys.modules.get(spec.module)` and falls back to
  `importlib.import_module` when a replaced `builtins.__import__` skips registration.
  `test_run_prints_traceback_for_transitive_import_failure` asserts the frozen frames are absent, and
  `test_import_target_falls_back_when_import_hook_skips_registration` covers the fallback.
- The helper is named `_is_repo_tools_module`, since it deliberately excludes plugin modules.
- `make_tools_package` uses `monkeypatch` for `sys.path` and `sys.modules`, and evicts only the modules it wrote.

### Task 2: Release notes, verification, archive

- [ ] **Step 1:** Add the spec's release note with
      `toolr release-notes add --slug runner-import-traceback --text "…"`. Commit
      `docs(release-notes): note import tracebacks for command modules`.
- [ ] **Step 2:** `mise run test`, polling the output every 30-60 s → green. `prek run --all-files` → green. No docs
      change, so no `mkdocs build`.
- [ ] **Step 3:**
      `git mv specs/2026-10-31-runner-import-traceback-design.md specs/2026-10-31-runner-import-traceback-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and the design's `Stacks on:` link, and set the design's status to implemented. Commit
      `docs(specs): archive runner import traceback design and plan`.
