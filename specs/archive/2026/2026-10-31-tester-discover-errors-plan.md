# Tester Discover Errors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `CommandsTester.discover()` fails when a `tools/*.py` module fails to import, and `tools/*.py` modules can
import stdlib modules under it (F-09).

**Architecture:** `_import_tools_modules()` returns the exceptions it caught and `discover()` raises them as one
`ExceptionGroup`. A new `_isolated_sys_path()` drops only the `sys.path` entries that provide a `tools` package.

**Tech Stack:** Python 3.11+, pytest ≥ 9 (`ExceptionInfo.group_contains`), attrs.

**Spec:** [`specs/archive/2026/2026-10-31-tester-discover-errors-design.md`](./2026-10-31-tester-discover-errors-design.md)

## Global Constraints

- No new public name. `toolr.testing.__all__` is unchanged.
- Catch `Exception` only. `SystemExit` and `KeyboardInterrupt` still propagate.
- Stay inside `_import_tools_modules`, `_isolated_sys_path`, `discover` and the `sys.path` block of `__enter__`.
  F-19 (`fix/tester-standalone-group`) edits the patchers, one line before `command_group_patcher.start()`, `__exit__`
  and `collected_command_groups`.
- Regression fixtures use the audit's exact shapes (`tools/bad.py`, `tools/stdlib_user.py` with
  `import sqlite3, tomllib, zipfile, csv`).
- Factory fixtures for `tmp_path` setup; imports at the top of test modules.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`.

## Review Focus

1. **Stdlib test can't pass vacuously.** Only modules missing from `sys.modules` break. The test removes `sqlite3`,
   `tomllib`, `zipfile` and `csv` with `monkeypatch.delitem` before building the tester and asserts they are absent.
   Task 2, `test_discover_imports_stdlib_modules_not_yet_loaded`.
2. **The fixture still wins over the host's `tools/`.** A namespace-package fixture (no `__init__.py`) merges every
   `tools/` on `sys.path`. Task 2, `test_discover_ignores_tools_dirs_elsewhere_on_sys_path`.
3. **The walk finishes before raising.** Every broken module is reported and the good ones still import. Task 1,
   `test_discover_raises_after_walking_every_module`.
4. **No `tools/` at all is still fine.** The shared `commands_tester` fixture in `tests/conftest.py` discovers an empty
   `tmp_path`; it must keep working unchanged. A broken `tools/__init__.py` must not be mistaken for "no package".
   Task 1, `test_discover_raises_when_tools_package_import_fails`.

---

### Task 1: Raise import errors as an `ExceptionGroup`

**Files:**

- Modify: `crates/toolr-py/python/toolr/testing/_discovery.py` (`_import_tools_modules`, `CommandsTester.discover`,
  module and class docstrings)
- Test: `tests/registry/test_integration.py`

**Interfaces:**

- `_import_tools_modules() -> list[Exception]` (was `(warnings: list[str]) -> None`). Each returned exception carries a
  note ``raised while importing `tools.<name>` ``.
- `CommandsTester.discover() -> None`. Raises `ExceptionGroup` when any import failed.
- [ ] **Step 1: Write the tests** in `tests/registry/test_integration.py`. Add a factory fixture
      `tools_repo(tmp_path) -> Callable[[dict[str, str]], Path]` that writes `tools/__init__.py` (empty unless the dict
      sets it) plus each `{relative_path: source}` under `tmp_path` and returns `tmp_path`.
    - Replace `test_discover_walks_a_real_tools_package` with `test_discover_raises_after_walking_every_module`: the
      same `ci.py` and `broken.py` (`raise RuntimeError('boom')`). Inside the `with CommandsTester(...)` block,
      `pytest.raises(ExceptionGroup) as excinfo` around `discover()`, then `"tools.ci" in sys.modules`. Assert
      `excinfo.group_contains(RuntimeError, match="boom")` and that the sub-exception's `__notes__` contains
      ``raised while importing `tools.broken` ``.
    - `test_discover_raises_on_undocumented_command`: the audit's `tools/bad.py`
      (`g = command_group("bad", "Bad", "Bad")`, `@g.command def nodoc(ctx): pass`) →
      `excinfo.group_contains(ValueError, match="add a docstring")`.
    - `test_discover_reports_every_failing_module`: `bad.py` and `broken.py` → `len(excinfo.value.exceptions) == 2`,
      and the group message contains `2 module(s)`.
    - `test_discover_raises_when_tools_package_import_fails`: `tools/__init__.py` is
      `import toolr_missing_dependency_xyz` → `excinfo.group_contains(ModuleNotFoundError)`.
    - `test_discover_without_tools_package_is_a_no_op`: empty `tmp_path` → `discover()` returns and
      `collected_command_groups() == {}`.
- [ ] **Step 2: Run** `uv run pytest tests/registry/test_integration.py -k discover` → the four raising tests fail
      (`DID NOT RAISE`); the no-op test passes.
- [ ] **Step 3: Implement.** `_import_tools_modules()` builds and returns `errors: list[Exception]`. The top-level
      `import tools` returns `[]` on `ModuleNotFoundError` only when `exc.name == "tools"`; any other exception there
      gets a note and is returned as the single error. Per-module failures get
      ``exc.add_note(f"raised while importing `{module_info.name}`")`` and the walk continues. `discover()` raises
      `ExceptionGroup(f"CommandsTester.discover(): {len(errors)} module(s) under {self.search_path / 'tools'} failed to
      import", errors)` when `errors` is non-empty. Update the `_import_tools_modules` docstring ("one bad file must not
      poison discovery" becomes "one bad file doesn't stop the walk; every failure is returned"), the `discover()`
      docstring (add a `Raises:` section) and the module docstring.
- [ ] **Step 4: Run** the Step 2 command → PASS. Then `uv run pytest tests/registry tests/decorators` → PASS.
- [ ] **Step 5: Commit.** `fix(testing): make CommandsTester.discover() raise on tools import errors`

### Task 2: Keep stdlib and other non-shadowing entries on `sys.path`

**Files:**

- Modify: `crates/toolr-py/python/toolr/testing/_discovery.py` (new `_isolated_sys_path`, `CommandsTester.__enter__`)
- Test: `tests/registry/test_integration.py`

**Interfaces:**

- `_isolated_sys_path(search_path: Path, saved: list[str]) -> list[str]`: `[str(search_path), *kept]`, dropping `""`,
  `str(search_path)`, and entries `p` where `Path(p, "tools").is_dir()` or `Path(p, "tools.py").is_file()` unless `p`
  contains `site-packages` or `dist-packages`.
- [ ] **Step 1: Write the tests:**
    - `test_discover_imports_stdlib_modules_not_yet_loaded(tools_repo, monkeypatch)`: for each of `sqlite3`,
      `tomllib`, `zipfile`, `csv`, `monkeypatch.delitem(sys.modules, name, raising=False)` and assert it is gone; then
      the audit's `tools/stdlib_user.py` (`import sqlite3, tomllib, zipfile, csv`, `g = command_group("std", "Std",
      "Std")`, documented `@g.command def ok(ctx)`) → `discover()` returns and
      `"ok" in groups["tools.std"].get_commands()`.
    - `test_discover_ignores_tools_dirs_elsewhere_on_sys_path(tmp_path, monkeypatch)`: `tmp_path/host/tools/__init__.py`
      and `host/tools/host_only.py` registering `command_group("host", ...)`; fixture at `tmp_path/fixture/tools/ci.py`
      with **no** `__init__.py`; `monkeypatch.syspath_prepend(tmp_path / "host")` before building the tester →
      `"tools.ci" in groups` and `"tools.host" not in groups`.
    - `test_discover_without_tools_ignores_host_tools(tmp_path, monkeypatch)`: same `host`, fixture with no `tools/` →
      `collected_command_groups() == {}` and `"tools.host_only"` not in `sys.modules` inside the block.
    - `test_discover_keeps_sys_path_entries_without_tools(tmp_path, monkeypatch)`: `tmp_path/libdir/fixture_helper.py`
      (`VALUE = 1`), `monkeypatch.syspath_prepend(libdir)`; fixture `tools/ci.py` does `import fixture_helper` and
      registers a group → the group is collected.
    - Unit tests on `_isolated_sys_path` (imported from `toolr.testing._discovery`):
      `test_isolated_sys_path_drops_tools_providers_and_cwd` (entries `""`, a dir with `tools/`, a dir with `tools.py`,
      a plain dir → only the plain dir survives, after `search_path`) and
      `test_isolated_sys_path_keeps_site_packages_with_tools` (a `.../site-packages` dir containing `tools/` is kept).
- [ ] **Step 2: Run** `uv run pytest tests/registry/test_integration.py -k "sys_path or stdlib or host"` → the stdlib,
      `keeps_sys_path_entries` and `_isolated_sys_path` tests fail; the two host tests pass already.
- [ ] **Step 3: Implement** `_isolated_sys_path` and make `__enter__` call
      `sys.path[:] = _isolated_sys_path(self.search_path, self.sys_path)`. Rewrite the comment above it to say what is
      dropped and why (a `tools` provider would shadow or merge into the fixture's package).
- [ ] **Step 4: Run** the Step 2 command → PASS. Then `uv run pytest tests/registry tests/testing` → PASS.
- [ ] **Step 5: Commit.** `fix(testing): keep the stdlib on sys.path during CommandsTester discovery`

### Task 3: Docs, skill refs, release notes, verification, archive

**Files:**

- Modify: `docs/writing-commands/testing.md`, `skills/toolr-command-authoring/references/testing.md` (generated),
  `UNRELEASED.md`
- [ ] **Step 1:** `docs/writing-commands/testing.md`, "What it does": replace bullet 1 with "Saves and replaces
      `sys.path` so `search_path` comes first and no other entry that provides a `tools` package is visible. The
      standard library and `site-packages` stay." After the `.discover()` paragraph, add: if any module fails to import,
      `.discover()` still imports the rest, then raises one `ExceptionGroup` with each module's error, and show
      `pytest.raises(ExceptionGroup)` with `excinfo.group_contains(ValueError, match="add a docstring")`.
- [ ] **Step 2:** `cargo xtask build-skill-refs` (regenerate, don't hand-merge). Check that `SKILL.md`'s
      `CommandsTester` prose still reads true.
- [ ] **Step 3:** Add the spec's `UNRELEASED.md` text verbatim. Commit
      `docs(testing): document CommandsTester discovery errors and sys.path`.
- [ ] **Step 4:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green.
      `uv run mkdocs build --strict` → green.
- [ ] **Step 5:**
      `git mv specs/2026-10-31-tester-discover-errors-design.md specs/2026-10-31-tester-discover-errors-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive tester discover errors design and plan`.
