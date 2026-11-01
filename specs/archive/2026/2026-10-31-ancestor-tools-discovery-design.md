# Require `tools/pyproject.toml` to find the project root

- **Status:** implemented
- **Origin:** whole-project review finding F-12 (medium).
- **Plan:** [`2026-10-31-ancestor-tools-discovery-plan.md`](./2026-10-31-ancestor-tools-discovery-plan.md)

## Problem

`discover_project_root` (`crates/toolr-core/src/discovery.rs`) walks up from the working directory and stops at the
first ancestor that has a `tools/` directory. Any `tools/` counts, including a Python sub-package that has nothing to do
with toolr.

Every caller then takes that directory as the repository: the bootstrap and freshness steps
(`crates/toolr/src/bootstrap.rs`), `load_or_empty` (`crates/toolr/src/main.rs`), dispatch, every `project` subcommand,
tab completion (`complete/freshness.rs::resolve_manifest_at_tab`) and `ensure_venv_ready`. The bootstrap steps return
early when `tools/pyproject.toml` is missing, so nothing explains what went wrong.

Repro, at `25e7ffdb`:

```text
repo/
├── tools/
│   ├── pyproject.toml
│   └── demo.py          # command_group("demo") with a `hello` command
└── src/pkg/tools/       # an unrelated package directory
```

From `repo/src/pkg`, `toolr demo hello` prints `error: unrecognized subcommand 'demo'` and `toolr --help` lists no user
groups. From `repo/` both work.

The root is also the trust key (`crates/toolr-core/src/trust.rs`) and the venv cache key, so `toolr project trust` from
`src/pkg` trusts the wrong directory.

## Decision

A `tools/` without `tools/pyproject.toml` is not a toolr tools directory. This is a breaking change, accepted.

- Walking up, `discover_project_root` returns the nearest directory whose `tools/pyproject.toml` is a file
  (`is_file()`). What else `tools/` holds (`__init__.py`, `*.py`, a manifest) doesn't matter.
- If none is found, discovery fails. `DiscoveryError::MissingPyproject { start, tools }` names the nearest `tools/` seen
  on the way and says toolr needs `tools/pyproject.toml`, pointing at `toolr project init` (which writes it before
  anything calls discovery). `NotFound` remains for "no `tools/` at all", with a message that also points at `toolr
  project init`.
- Callers that propagate the error (dispatch, `toolr project trust`/`untrust`, `project manifest rebuild`, the venv
  commands) report it as is. `load_or_empty` (`crates/toolr/src/main.rs`) prints it as a warning only when the argv
  names a command that isn't a built-in, so `toolr demo hello` in a bare-`tools/` repo explains the following
  `unrecognized subcommand`. Plain `toolr`/`toolr --help` stay quiet, since a bare `tools/` (the Windows runners'
  `C:\tools`) may sit above any directory. Bootstrap, freshness and tab completion treat it as "no project", and `venv
  sync --quiet` (the mise `enter` hook) still exits silently through its "locating project root" marker.
- A private `discover_below(start, ceiling)` holds the walk; unit tests pass their tempdir as the ceiling so host
  directories can't change their result.
- With no project there is no tools venv, so dispatch's fallback (run commands with `$TOOLR_PYTHON`, `python3` or
  `python` from `PATH`, #588) is unreachable and removed, with `resolve_python` and every use of `$TOOLR_PYTHON`.
  `MIN_PYTHON_MINOR` and `python3_minor` stay for `toolr project init`.
- `toolr.testing`'s pytest plugin (`_pytest_plugin.py::_discover_repo_root`) follows the same rule, checking with
  `os.path.isfile` because `Path.is_file` raises `PermissionError` for an untraversable `tools/` before Python 3.14.

### Rejected alternatives

- **Require `tools/pyproject.toml` only for dispatch, keeping bare repos (#588) for help and completion.** Two sets of
  rules for one question; a bare repo still couldn't build its first manifest (`project manifest rebuild` needed the
  pyproject to resolve the venv anyway).
- **Prefer a `tools/` marked by `pyproject.toml` or `.toolr-manifest.json`, bounded by `.git`.** A marked outer
  `tools/` hijacked bare inner ones outside git (tarballs, other VCSs, nested example directories), and the gitignored
  manifest made the root and trust key depend on local cache state.
- **Skip every `tools/` that holds `__init__.py`.** Broke projects with `tools/__init__.py` beside
  `tools/pyproject.toml` whenever any bare `tools/` sat above them.
- **Skip a `tools/` with `__init__.py` and no `pyproject.toml`.** A package `tools/` without `pyproject.toml` under a
  bare ancestor still lost its commands, and bare repos failed under a bare ancestor `tools/`.

## Scope and compatibility

- No manifest or runner schema change.
- **Breaking:** repos whose `tools/` has no `pyproject.toml` stop working until they add one (`toolr project init`).
  Their commands, `$TOOLR_PYTHON` and the `PATH` interpreter fallback are gone.
- Any `tools/` without `pyproject.toml` between the working directory and the project is now skipped, so its root,
  venv cache key and trust key resolve to the project above.
- Production changes: `crates/toolr-core/src/discovery.rs`, `crates/toolr-core/src/execute/python.rs`,
  `crates/toolr/src/{dispatch,main,bootstrap}.rs`, `_pytest_plugin.py`.
- Test fixtures that built a bare `tools/` now write `tools/pyproject.toml` (`common::write_project`), and
  hand-written manifests are stamped fresh (`common::write_fresh_manifest`) so the freshness check keeps them. Tests
  that only exercised the fallback are removed.
- Docs: `docs/concepts.md` ("The `tools/` directory") and `docs/installation/index.md` (the no-pyproject case).
- `UNRELEASED.md`: a **Breaking** entry; the queued #588 `PATH`-interpreter entry is removed, since that feature never
  shipped.
