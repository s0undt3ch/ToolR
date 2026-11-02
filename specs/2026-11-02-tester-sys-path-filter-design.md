# Keep only the interpreter's own site directories in `CommandsTester`'s `sys.path`

- **Status:** proposed
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

An entry is exempt from the `tools`-provider rule only if it is one of the running interpreter's own site
directories, matched as a whole normalised path:

- `site.getsitepackages()`: the global site directories for `sys.prefix` and `sys.exec_prefix`. In a venv
  created with `include-system-site-packages = true` this also lists the base interpreter's directories, Debian's
  `dist-packages` among them, since Debian patches `site.getsitepackages()`. On Windows it includes the prefix itself.
- `site.getusersitepackages()`: the user site directory.
- `sysconfig.get_paths()["purelib"]` and `["platlib"]`: where the installer puts packages, which covers distro layouts
  where `site` and `sysconfig` disagree.

Both sides are compared after `os.path.normcase(os.path.realpath(...))`, so a symlinked prefix, a trailing separator
or a case difference on Windows doesn't matter.

The query lives in a small helper with no branches, `_site_dirs() -> frozenset[str]`, which `_isolated_sys_path` calls
once per call. Tests replace the helper with `monkeypatch` to declare a `tmp_path` directory a site directory.

Everything else about `_isolated_sys_path` stays as it is: `""` and `search_path` are dropped, `search_path` goes
first, entries without a `tools` provider (the stdlib, `lib-dynload`, the stdlib zip, `.pth` additions, src-layout
editable installs, `PYTHONPATH`) are kept in order.

## Rejected alternatives

- **Match a path component** (`"site-packages" in Path(entry).parts`, or the last component only). This fixes the
  two reported shapes, but a repo under a parent directory literally named `site-packages` (or a repo with that name)
  still keeps its root. It is the same name-based guess that caused this bug.
- **Exempt anything under `sys.prefix` / `sys.base_prefix`.** On a system interpreter `sys.prefix` is `/usr`, so a
  checkout at `/usr/src/app` would be exempt and the bug would survive. An in-tree venv at `tools/.venv` would also
  put `sys.prefix` under the repo, though not above the root.
- **Drop the exemption entirely.** A third-party distribution that installs a top-level `tools` package would then
  take the whole site-packages directory off `sys.path`, and every import in the user's `tools/*.py` would fail.
- **Keep the substring test and also check for a `.git` marker or `pyproject.toml`.** Another heuristic; it doesn't
  cover a fixture-like directory with no marker, and it adds filesystem probes for every entry.

## Scope and compatibility

- Affects only `toolr.testing.CommandsTester` (and through it `toolr.testing`'s pytest plugin). No runner, manifest
  or Rust change; no schema bump.
- Behaviour changes only for an entry that provides `tools` and whose path contains `site-packages` or
  `dist-packages` without being an interpreter site directory. Such an entry is now dropped, as any other `tools`
  provider is.
- A site directory of another interpreter, added through `PYTHONPATH` or a `.pth` file (for example `uv run --with`'s
  overlay environment), is no longer exempt. If it holds a top-level `tools` package, it is dropped; a stray `tools`
  provider being hidden is what the isolation is for.
- Not covered, as before: a regular `tools` package installed in the interpreter's site-packages still replaces a
  fixture `tools/` that has no `__init__.py`.
- User-visible for `toolr.testing` users: one release note. `docs/writing-commands/testing.md` changes "except
  `site-packages` directories" to name the interpreter's site directories.
