# Plan: resolve `tools` from the fixture only in `CommandsTester`

- **Design:** [`2026-11-02-tester-sys-path-filter-design.md`](./2026-11-02-tester-sys-path-filter-design.md)
- **Branch:** `fix/tester-sys-path-filter`

All tests live in `tests/registry/test_integration.py`, which already has the `tools_repo` factory fixture and imports
`_isolated_sys_path` at the top.

## Task 1: failing tests for the reported shapes

- [x] `test_discover_ignores_tools_under_a_dir_named_like_site_packages`, parametrised over `windows-site-packages`,
      `my-site-packages-tool`, `dist-packages-fork` and `site-packages`: a decoy at `.../.worktrees/<name>` with a
      namespace `tools/` holding a module that raises on import, prepended to `sys.path`; the fixture's namespace
      `tools/` holds `CI_MODULE`. `collected_command_groups()` is `["tools.ci"]` only. Fails on `main`.
- [x] `test_discover_keeps_an_overlay_site_dir_holding_a_stray_tools`, parametrised over a regular and a namespace
      fixture: an overlay `.../archive-v0/<hash>/lib/site-packages` with a stray regular `tools/` that raises on
      import and an `overlaydep` package; the fixture's `tools/ci.py` imports `overlaydep`. Fails on the
      interpreter-site-directory fix with `ModuleNotFoundError: overlaydep`.

## Task 2: a `sys.meta_path` finder for `tools`

- [x] Add `_FixtureToolsFinder(search_path)` to `_discovery.py`: `find_spec` handles only `tools`, via
      `PathFinder.find_spec("tools", [search_path])`, raising `ModuleNotFoundError(name="tools")` when the fixture has
      none; a namespace spec gets a `_NamespacePath` of importlib's own type whose path finder looks only in
      `search_path` and returns plain lists.
- [x] `CommandsTester` gets a `tools_finder` field; `__enter__` inserts it at `sys.meta_path[0]`, `__exit__` removes it.
- [x] `_isolated_sys_path` keeps every entry except `""` and `search_path`; remove `_provides_tools`, `_site_dirs` and
      `_normalise`.
- [x] Replace the `_isolated_sys_path` filtering tests with
      `test_isolated_sys_path_drops_only_cwd_and_search_path`.
- [x] `test_namespace_tools_path_stays_in_the_fixture_when_sys_path_changes`: after `discover()`-style import,
      prepending a directory with another namespace `tools/` leaves `tools.__path__` on the fixture, and so does
      removing the fixture's `tools/` (the recompute finds nothing).
- [x] `test_namespace_tools_supports_importlib_resources`, `test_commands_tester_removes_its_finder_on_exit` (nested
      testers leave `sys.meta_path` unchanged).
- [x] Mutation check: disabling the namespace rebuild, the plain-list conversion or the `ModuleNotFoundError` each
      makes a test fail.

## Task 2b: review round 2

- [x] `test_discover_ignores_host_dirs_added_by_extend_path`: a fixture `tools/__init__.py` calling
      `pkgutil.extend_path`, with the host repo on `sys.path`; fails until the walk skips directories outside
      `search_path / "tools"`.
- [x] `test_commands_tester_exit_restores_state_when_cwd_is_gone`: delete the original cwd inside the tester; `__exit__`
      raises `FileNotFoundError` but the finder is gone and `sys.path` restored. Restore the cwd last.
- [x] `test_find_spec_for_tools_raises_without_a_fixture_tools` pins the documented `ModuleNotFoundError`.

## Task 2c: review round 3 (maintainability, no behaviour change)

- [x] Rename `_isolated_sys_path` to `_tester_sys_path`, and move the `_NamespacePath` rebuild and its path finder into
      `_fixture_only_namespace_path`.
- [x] Add a `restore_import_state` fixture for tests that enter a tester by hand or prepend to `sys.path` inside one.
- [x] Drop `test_discover_keeps_sys_path_entries_without_tools` (the overlay test covers it) and keep only the
      reported `windows-site-packages` name.
- [x] Docs: move the walk-scope sentence to the `.discover()` paragraph and note that assertion rewriting skips the
      top-level `tools/__init__.py`.

## Task 3: docs and release note

- [x] `docs/writing-commands/testing.md`: describe the finder instead of the `sys.path` filtering.
- [x] Rewrite `release-notes/…tester-sys-path-filter.md` to match, committed as `docs(release-notes): …`.
- [x] `toolr release-notes check --base origin/main` exits 0.

## Task 4: verify

- [x] `uv run pytest` (full).
- [x] `prek run --all-files`; commit any regenerated skill refs.
- [x] Coverage of `_discovery.py` is 100% for lines and branches.
- [x] Final commit: `git mv` the design and plan to `specs/archive/2026/`, status `implemented`.
