# Make `CommandsTester.discover()` fail on import errors and keep the stdlib importable

- **Status:** implemented
- **Origin:** whole-project review finding F-09 (medium).
- **Plan:** [`2026-10-31-tester-discover-errors-plan.md`](./2026-10-31-tester-discover-errors-plan.md)

## Problem

`toolr.testing.CommandsTester` (`crates/toolr-py/python/toolr/testing/_discovery.py`) has two defects that turn
broken `tools/*.py` modules into green discovery tests.

1. **Import errors are swallowed.** `_import_tools_modules(warnings)` catches every `Exception` per module and appends
   a string to `warnings`. `CommandsTester.discover()` creates that list and never reads it. A module that raises on
   import passes discovery. The decorator rule in `_decorators.py::_require_docstring` exists precisely so "a
   `CommandsTester` discovery test fails the same way `toolr` would", and `docs/writing-commands/docstrings.md` and the
   changelog entry for undocumented commands say the same. Neither holds. Worse, `command_group(...)` runs before the
   failing `@command`, so `collected_command_groups()` can contain the group, with no commands, while `toolr` itself
   refuses to build the manifest.
2. **The standard library is dropped from `sys.path`.** `CommandsTester.__enter__` sets
   `sys.path[:] = [str(self.search_path), *site_pkg_entries]`, keeping only entries that contain `site-packages` or
   `dist-packages`. The stdlib directory, `lib-dynload` and the stdlib zip are gone. Any stdlib module that is not
   already in `sys.modules` raises `ModuleNotFoundError`, which defect 1 then swallows, so the whole group vanishes. The
   same filter drops editable-install paths and `PYTHONPATH` entries.

Repro (the audit's shape): a fixture repo with

- `tools/__init__.py` (empty),
- `tools/bad.py`: `g = command_group("bad", "Bad", "Bad")` and `@g.command def nodoc(ctx): pass`,
- `tools/stdlib_user.py`: `import sqlite3, tomllib, zipfile, csv`, `g = command_group("std", "Std", "Std")` and a
  documented `@g.command def ok(ctx)`.

`CommandsTester(search_path=repo)` then `discover()` returns normally, and `collected_command_groups()` is
`{"tools.bad": <no commands>}`. `tools.std` is missing (when `sqlite3` was not preloaded) and no error is raised. `toolr
project manifest rebuild` on the same repo fails with "commands without a docstring".

The existing test `tests/registry/test_integration.py::test_discover_walks_a_real_tools_package` writes a `broken.py`
and asserts discovery succeeds, so it pins the defect.

## Decision

### Raise one `ExceptionGroup` after the walk

`_import_tools_modules()` stops taking a `warnings` list and returns `list[Exception]`: every exception raised while
importing a `tools.*` module, each with a note added via `exc.add_note(f"raised while importing `{name}`")`. The walk
still visits every module, so one run reports every broken file. `discover()` raises

```python
ExceptionGroup(f"CommandsTester.discover(): {n} module(s) under {search_path / 'tools'} failed to import", errors)
```

when the list is non-empty. This mirrors the manifest build, which batches `MissingDocstrings` and `UnknownGroupRefs`
into one error listing every offender. pytest renders each sub-exception with its own traceback.

The top-level `import tools` keeps its early return for `ModuleNotFoundError`, but only when `exc.name == "tools"`. A
`ModuleNotFoundError` raised from inside `tools/__init__.py` (a missing third-party dependency) is a real failure and
goes into the group, as does any other exception from the top-level import.

Only `Exception` is caught, as today. `SystemExit` and `KeyboardInterrupt` at module level propagate.

### Keep every `sys.path` entry that can't shadow the fixture

The original filter exists so the host repo's own `tools/` can't leak into the fixture. That leak needs a `sys.path`
entry that provides a top-level `tools` package. With the fixture first, a regular fixture package wins anyway, but a
fixture `tools/` without `__init__.py` is a namespace package: a regular `tools` package (or `tools.py`) anywhere on
`sys.path` replaces it, and any other namespace `tools/` directory merges into it. A fixture with no `tools/` at all would
import the host's.

A new module-level helper `_isolated_sys_path(search_path: Path, saved: list[str]) -> list[str]` returns
`[str(search_path), *kept]`, where `kept` is `saved` minus:

- `""` (the "current directory" entry),
- `str(search_path)` itself (no duplicate),
- any entry `p` where `Path(p, "tools")` is a directory or `Path(p, "tools.py")` is a file, unless `p` contains
  `site-packages` or `dist-packages`.

Site-packages directories are always kept, as today. Everything else (stdlib, `lib-dynload`, the stdlib zip,
src-layout editable-install paths, `PYTHONPATH` entries) now stays. An editable-install root that itself contains
`tools/` is dropped like any other `tools` provider. The host repo root, pytest's rootdir insertion and the entry
`toolr._runner._append_repo_root` adds from the pytest plugin all contain `tools/`, so they are still dropped.

`__enter__` calls `sys.path[:] = _isolated_sys_path(self.search_path, self.sys_path)`. `__exit__` is unchanged.

### Rejected alternatives

- **`pytest.fail` in `discover()`.** `toolr.testing` is a library usable without pytest, and `_discovery.py` doesn't
  import it. An exception also lets callers assert on the error with `pytest.raises`.
- **A named `ToolsDiscoveryError` subclass in `toolr.testing.__all__`.** Friendlier in `pytest.raises(...)`, but it
  grows the public API for something `pytest.raises(ExceptionGroup)` plus `excinfo.group_contains(...)` (pytest ≥ 8,
  the repo floor is 9.1.1) already covers.
- **Raise on the first failing module.** Hides every later failure until the first is fixed. The manifest build
  reports them all.
- **`warnings.warn` per failure.** A warning is exactly what gets missed. The point is a red test.
- **A `strict=False` opt-out on `discover()`.** New API for a mode that only exists to keep false-green tests green.
- **Allow-list stdlib directories from `sysconfig`.** Misses the Windows `DLLs` directory, the stdlib zip, editable
  installs and `PYTHONPATH`. The drop-list removes exactly what can shadow and nothing else.
- **Resolve the host repo root and drop everything under it.** `CommandsTester` doesn't know the host repo; deriving
  it from the cwd breaks when tests run from elsewhere, and an in-tree `tools/.venv` site-packages lives under the root
  and would need a carve-out.

## Scope and compatibility

- Touches `_discovery.py`: `_import_tools_modules`, a new `_isolated_sys_path`, `CommandsTester.discover` and the
  `sys.path` block in `CommandsTester.__enter__`. F-19 (`fix/tester-standalone-group`) adds a patcher field, one line in
  `__enter__` before `command_group_patcher.start()`, one in `__exit__` and the body of `collected_command_groups`.
  The hunks don't overlap; whichever lands second rebases cleanly.
- No runner or manifest schema change. No new public name, so `toolr.testing.__all__` is unchanged, but the
  `discover()` and class docstrings change and `skills/toolr-command-authoring/references/testing.md` embeds them:
  regenerate with `cargo xtask build-skill-refs`.
- Behaviour change: a discovery test that passed only because an import error was swallowed now fails. That is the
  fix.
- Docs: `docs/writing-commands/testing.md` "What it does" (the `sys.path` bullet and the `.discover()` paragraph). The
  claims in `_decorators.py::_require_docstring` and `docs/writing-commands/docstrings.md` become true; no edit. The
  historical changelog entry is not edited.
- Not covered: a stray top-level `tools` package inside site-packages still replaces a namespace fixture `tools/`, as
  today.
- `UNRELEASED.md`:

  > `toolr.testing.CommandsTester.discover()` now raises when a `tools/*.py` module fails to import, for example
  > because a command has no docstring. It imports every module first, then raises one `ExceptionGroup` holding each
  > module's error, so a discovery test fails the way `toolr` does. It used to hide those errors and pass, sometimes
  > with a group that had no commands. Discovery also keeps the standard library on `sys.path` now: a `tools/*.py`
  > that imports `sqlite3`, `tomllib` or another stdlib module not loaded yet used to vanish from
  > `collected_command_groups()` without an error. A discovery test that passed only because of either problem now
  > fails.
