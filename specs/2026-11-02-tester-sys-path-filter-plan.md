# Plan: keep only the interpreter's own site directories in `CommandsTester`'s `sys.path`

- **Design:** [`2026-11-02-tester-sys-path-filter-design.md`](./2026-11-02-tester-sys-path-filter-design.md)
- **Branch:** `fix/tester-sys-path-filter`

All tests live in `tests/registry/test_integration.py`, which already has the `tools_repo` factory fixture and imports
`_isolated_sys_path` at the top.

## Task 1: failing tests for the reported shape

- [ ] Add `test_discover_ignores_tools_under_a_dir_named_like_site_packages`: build a decoy repo at
      `tmp_path / "decoy" / "windows-site-packages"` with a namespace `tools/` (no `__init__.py`) holding a module
      that raises on import, `monkeypatch.syspath_prepend` it, build the fixture with `tools_repo` and a namespace
      `tools/` (`"tools/__init__.py": None`) holding `CI_MODULE`. `discover()` must not raise and
      `collected_command_groups()` must be `{"tools.ci": ...}` only. This is the shape that failed in
      `.worktrees/windows-site-packages`.
- [ ] Add `test_isolated_sys_path_drops_tools_providers_named_like_site_packages`, parametrised over
      `windows-site-packages`, `my-site-packages-tool` and `dist-packages-fork`: each directory holds `tools/` and is
      dropped.
- [ ] Run `uv run pytest tests/registry/test_integration.py -k "site_packages"`; both new tests fail on `main`.

## Task 2: ask the interpreter for its site directories

- [ ] In `_discovery.py`, import `site` and `sysconfig` and add `_normalise(path) -> str`
      (`os.path.normcase(os.path.realpath(path))`) and `_site_dirs() -> frozenset[str]` built from
      `site.getsitepackages()`, `site.getusersitepackages()` and `sysconfig.get_paths()` `purelib`/`platlib`.
- [ ] In `_isolated_sys_path`, compute `site_dirs = _site_dirs()` once and replace the substring test with
      `_normalise(entry) in site_dirs`. Update the docstring.
- [ ] Rewrite `test_isolated_sys_path_keeps_site_packages_with_tools`: it relied on the directory name. Make it
      monkeypatch `_discovery._site_dirs` to return the `tmp_path` directory, and name the directory something that
      doesn't look like site-packages so the test proves the interpreter query is what counts.
- [ ] Add `test_site_dirs_lists_the_interpreter_site_packages`: the real `_site_dirs()` contains the normalised
      `sysconfig.get_paths()["purelib"]`, and `_isolated_sys_path` keeps that entry.
- [ ] Run the module; all tests pass.

## Task 3: docs and release note

- [ ] `docs/writing-commands/testing.md`: replace "except `site-packages` directories" with the interpreter's own
      site directories.
- [ ] `toolr release-notes add --slug tester-sys-path-filter --text "…"`, committed as `docs(release-notes): …`.
- [ ] `toolr release-notes check --base origin/main` exits 0.

## Task 4: verify

- [ ] `uv run pytest` (full).
- [ ] `prek run --all-files`; commit any regenerated skill refs.
- [ ] `uv run coverage run -m pytest && uv run coverage report -m --include='*/toolr/testing/_discovery.py'`; the
      changed lines are covered.
- [ ] Final commit: `git mv` the design and plan to `specs/archive/2026/`, status `implemented`.
