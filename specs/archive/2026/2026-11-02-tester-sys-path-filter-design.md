# Resolve `tools` from the fixture only in `CommandsTester`

- **Status:** implemented
- **Origin:** audit follow-up FU-10.
- **Plan:** [`2026-11-02-tester-sys-path-filter-plan.md`](./2026-11-02-tester-sys-path-filter-plan.md)

## Problem

`CommandsTester.__enter__` replaces `sys.path` with `_isolated_sys_path(search_path, saved)`
(`crates/toolr-py/python/toolr/testing/_discovery.py`). That function drops every entry that provides a top-level
`tools` package or module, so the host repo root can't stand in for, replace or merge into the fixture's `tools/`.
Site-packages directories are exempt, so that a stray `tools` package installed by a third-party distribution doesn't
take the whole environment off `sys.path`. The exemption is a substring test:

```python
is_site_packages = "site-packages" in entry or "dist-packages" in entry
```

Any entry whose path merely contains that text is exempt, wherever it sits in the path. A repo checked out under such a
directory keeps its root on `sys.path` even though the root provides `tools/`, for example:

- a worktree named `.worktrees/windows-site-packages`;
- a checkout at `~/src/my-site-packages-tool`.

`CommandsTester.discover()` then imports the repo's own `tools/` instead of the fixture's: a fixture `tools/` without
`__init__.py` merges with the repo's namespace `tools/`. Tests fail, or pass against the wrong code.

Reproduced on 2026-11-02 in a toolr worktree at `.worktrees/windows-site-packages`: `uv run pytest tests/registry/`
gave 3 failures and 16 errors. toolr's own `tools/` has no `__init__.py`, so every namespace fixture merged with it.

### History

F-09 (`specs/archive/2026/2026-10-31-tester-discover-errors-design.md`, PR #626) replaced an allow-list (only
`site-packages`/`dist-packages` entries survived, which lost the stdlib) with today's deny-list (drop `tools`
providers). It kept the substring test from the old allow-list unchanged. In the allow-list a false positive only kept
an extra entry. In the deny-list the same false positive defeats the isolation.

## Decision

Stop pruning `sys.path` to decide which `tools` gets imported. Instead, while a `CommandsTester` is active, a
`sys.meta_path` finder (`_FixtureToolsFinder`) sits first and resolves the top-level name `tools` from `search_path`
only:

- `find_spec("tools")` calls `importlib.machinery.PathFinder.find_spec("tools", [search_path])`. If the fixture has no
  `tools`, it raises `ModuleNotFoundError(name="tools")`, so no later finder can find another one. `discover()` already
  treats that as "nothing to walk".
- Every other name, `tools.*` included, returns `None` and goes through the normal finders. Submodules resolve through
  `tools.__path__`, which only names the fixture.
- A namespace fixture's `__path__` is a `_NamespacePath`, which recomputes itself from all of `sys.path` whenever
  `sys.path` changes or the import caches are invalidated (`monkeypatch.syspath_prepend` does both). The finder
  rebuilds it with importlib's own type but a path finder that only looks in `search_path`, so a recompute can't merge
  another `tools/` in. The type stays importlib's because `importlib.resources` rejects any other `__path__` for a
  namespace package.
- `__enter__` inserts the finder at `sys.meta_path[0]`; `__exit__` removes that instance, so nested testers unwind in
  order.
- The private importlib dependency (rebuilding a `_NamespacePath` with a custom path finder) lives in one helper,
  `_fixture_only_namespace_path`, with a comment naming the CPython versions checked (3.11 to 3.15) and the tests that
  guard it.
- `pkgutil.extend_path` scans `sys.path` directly and ignores `sys.meta_path`, so a fixture `tools/__init__.py` that
  calls it still gets the host's `tools/` on its `__path__`. `discover()` therefore walks only directories that resolve
  under `search_path / "tools"`.
- `__exit__` removes the finder first and restores the cwd last, so a deleted original cwd can't leave the finder
  installed for the rest of the process.

`_isolated_sys_path`, renamed `_tester_sys_path` since it no longer isolates anything, keeps every entry: it drops
only `""` (the cwd is `search_path` while the tester is active) and `search_path`, which it puts first. The stdlib,
site-packages, overlays, `.pth` additions, editable installs and `PYTHONPATH` all stay importable. Name checks on
`site-packages` are gone altogether.

## Rejected alternatives

- **Exempt only the running interpreter's site directories** (`site.getsitepackages()`, the user site,
  `sysconfig` `purelib`/`platlib`, compared as whole paths). This was the first fix on this branch. Adversarial review
  showed it drops a site directory the interpreter doesn't own, such as a `uv run --with` overlay
  (`~/.cache/uv/archive-v0/<hash>/.../site-packages`), a `.pth`- or `PYTHONPATH`-chained environment, or tox/nox
  layering, whenever that directory ships a stray top-level `tools`. Every package installed beside it goes too, and a
  fixture module importing one of them fails with `ModuleNotFoundError`. Pruning whole entries is the wrong
  granularity: only one name needs isolating.
- **Match a path component** (`"site-packages" in Path(entry).parts`). Still a name guess: a repo under a directory
  named `site-packages` keeps its root, and it has the same overlay problem for any directory not named that way.
- **Exempt anything under `sys.prefix` / `sys.base_prefix`.** On a system interpreter `sys.prefix` is `/usr`, so a
  checkout at `/usr/src/app` would be exempt.
- **Also keep entries holding `*.dist-info`** (install targets). Fixes the overlay case, but stays a heuristic over
  whole entries, and an editable-install root with metadata beside a `tools/` would be kept.
- **Drop the exemption entirely.** A stray `tools` in site-packages would take the whole environment off `sys.path`.

## Scope and compatibility

- Affects only `toolr.testing.CommandsTester` (and through it `toolr.testing`'s pytest plugin). No runner, manifest
  or Rust change; no schema bump.
- `sys.path` inside the tester is now the saved path with `search_path` first and `""` removed; entries that provide
  `tools` are no longer removed. Code that relied on the host repo root being off `sys.path` for other top-level
  modules now sees them, after the fixture's.
- Closes the case F-09 left open: a regular `tools` package installed in site-packages no longer replaces a fixture
  `tools/` that has no `__init__.py`.
- With no fixture `tools`, `importlib.util.find_spec("tools")` inside the tester raises `ModuleNotFoundError` rather
  than returning `None`. That raise is what keeps another `tools` from being found; the testing docs say so.
- A fixture `tools/__init__.py` that calls `pkgutil.extend_path` still has the host's directories on `tools.__path__`,
  so an explicit `import tools.<host module>` would succeed; `discover()` doesn't walk them.
- The finder bypasses later meta-path hooks for the top-level `tools` module only, pytest's assertion rewriter among
  them. `tools.*` submodules still go through every hook.
- User-visible for `toolr.testing` users: one release note. `docs/writing-commands/testing.md` describes the finder
  instead of the `sys.path` filtering.
