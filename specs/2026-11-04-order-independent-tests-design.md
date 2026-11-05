# Order-independent Python tests and pytest-randomly: design

Follow-up FU-21. Stacked on the registry isolation work (FU-12), whose root autouse `_clean_registry` fixture
this builds on.

## Problem

The Python suite only passes in its default order. Validated on this branch (FU-12's fixture in place):

- **Reverse order** (`uv run pytest --co -q -p no:randomly | grep :: | tail -r`, node ids passed via
  `@file`, with `-p no:cacheprovider`): 4 failed, 1 error. `test_chdir.py` (2), `test_include_timestamps.py`,
  `test_path_hygiene.py` (`No module named 'tools.probe'`), and the pytester error "no `tools/pyproject.toml`
  found" (`test_pytest_plugin.py`, `test_it.py::test_uses_repo_root`).
- **`pytest-randomly` 5.0.0, seeds 1 to 10** (not committed, `uv run --with pytest-randomly pytest
  -p randomly --randomly-seed=N`): every seed but one fails, between 1 and 75 tests. Seeds 8 and 9 fail dozens
  at once.

No test relies on a fixed `random`, `faker` or numpy sequence (`grep` finds none; `hypothesis` is the only
randomised library and `pytest-randomly` reseeds it correctly).

## Root causes

1. **Working directory leak (most failures).** `tests/context/test_core.py::test_chdir_nonexistent_original`
   calls `os.chdir(new_cwd)` and then deletes the directory while `ctx.chdir` is inside it, so the process is
   left in a deleted cwd. `test_chdir_str_path` calls `os.chdir(tmp_path)` and never restores it. Every later
   test that calls `Path.cwd()` or spawns a process fails with `FileNotFoundError` or "no `tools/pyproject.toml`
   found": `test_chdir.py`, `test_core.py::test_chdir` (setup), `tests/utils/command` (21), `commands_tester`
   users (`registry/test_registry_basic.py`, 13 errors), `test_tools_release_notes.py` (24),
   `test_tools_version.py` (22), `test_types_static.py`, `test_make_context.py`, e2e, `sources/test_e2e.py` and
   the pytester tests.
2. **Leaked `logging.StreamHandler` patch.** Five tests in `tests/utils/logs/test_setup.py` call
   `patch("logging.StreamHandler").start()` and never stop it. The process then has a `MagicMock` for
   `logging.StreamHandler`, so `test_include_timestamps_with_timestamp_formatter` builds a mock handler and
   gets `False`. The mocks were pointless: the tests only need handler objects with a `setFormatter` spy.
3. **Cached repo `tools` package.** Collection imports the repo's own `tools` namespace package
   (`tools.ci`, `tools.release_notes`). `test_path_hygiene.py` and `test_current_context.py` write a regular
   `tools` package under `tmp_path` and expect `import tools.probe` to find it, but a cached namespace package
   keeps its old search path. They pass only when something popped `tools` first. Both also clean up by
   hand, and `test_runner_coverage.py::make_tools_package` already has the right pattern (hide the repo's
   `tools*` entries from `sys.modules` with `monkeypatch.delitem`).

The signal e2e tests (`tests/e2e/test_signals.py`) also failed during validation. That is a harness artefact:
a backgrounded shell (`&`, `nohup`) starts children with SIGINT ignored, which `toolr` inherits. They pass in a
foreground run, including under random order. Not a finding.

## Design

### Fixes at the source

- **cwd:** a root autouse fixture `_restore_cwd` in `tests/conftest.py`. It records `os.getcwd()` before the
  test and `os.chdir`s back afterwards, so no test can leak a cwd, including a deleted one, whatever it calls.
  No test edits. It sits beside `_clean_registry`; no new directory, so the `__init__.py` and `files/` rules hold.
- **StreamHandler:** in `test_setup.py` replace each `patch("logging.StreamHandler").start()` with
  `MagicMock()`. Nothing is left to stop.
- **`tools`:** a `tests/runner/conftest.py` autouse fixture `_isolate_repo_tools` that copies `sys.path` with
  `monkeypatch.setattr` and removes the repo's `tools*` modules with `monkeypatch.delitem` (using
  `toolr._runner._is_repo_tools_module`). Drop the manual save/restore and `sys.modules.pop` from the two
  tests, and the duplicate setup lines in `make_tools_package`. Scoped to `tests/runner` because other
  directories legitimately use the cached `tools.ci`; monkeypatch restores them afterwards.

### Adopt pytest-randomly

- Add `pytest-randomly>=5.0.0` to the `dev` dependency group. 5.0.0 was released on 2026-09-01, far outside the
  7-day `exclude-newer` cooldown, so no `exclude-newer-package` entry. It needs Python 3.10 or later and
  pytest 8 or later; the repo supports 3.11 or later. `uv lock` adds it.
- **CI:** no flag. The plugin shuffles by default and its header prints `Using --randomly-seed=N`; the CI
  invocation already uses `-v`, so the seed is in the log. Failing runs are reproducible from the log alone.
- **`mise run test` / `pytest-test`:** no flags. Same default, same seed header.
- **xdist:** not used, so no interaction. If added, `pytest-randomly` is xdist-aware and shares one seed.
- **`distribution` suite and `tests/e2e`:** `_test_distribution.yml` and `install-smoke.yml` also run pytest
  with the dev group, so randomising applies there too. Verified in the plan (e2e in the full runs; the
  `distribution` suite once with a seed). If a distribution test proves order-dependent, fix the test; it is not exempted
  with `-p no:randomly` (decided).
- **Docs:** `CONTRIBUTING.md` Tests section gains "Reproducing a failure": copy the seed from the header and
  run `uv run pytest -p randomly --randomly-seed=N`; `-p no:randomly` restores the default order. It also
  documents the reverse-order recipe as a debugging aid.

### Reverse-order CI step

Rejected. A random shuffle covers every order a reverse run covers, and does so with a varying seed each
run; a fixed reverse step only repeats one permutation. The recipe stays as a local debugging tool.

## Non-goals

- No meta-tests. The proof is the suite passing across many seeds and in reverse.
- No change to `toolr` source, schemas or the manifest. Test-only.

## Testing and release note

Verification (plan Task 5): 20 seeds plus reverse order, all green. `Release-Notes: skip` trailer on every
commit; no `release-notes/` entry.
