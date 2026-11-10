# First-run hardening

Five small fixes from the audit, each its own commit.

## Why

- S-03: `project init --yes` / `--force` records trust for the repository silently, so a README
  that says "run `toolr project init --yes`" grants trust without the user seeing it.
- S-04: three message sites print repo-derived paths without `shown()`, so a directory name with
  control characters reaches the terminal raw.
- U-08: outside a project, an unknown command reads as a typo; nothing says no project was found.
- X-03: on Windows `discover_project_root` canonicalises to a verbatim `\\?\` path, which then
  becomes `ctx.repo_root`, the runner's cwd and a `sys.path` entry.
- X-05: on Windows the runner starts without UTF-8 stdio, so CPython 3.11-3.14 pipes `sys.stdout`
  through the ANSI code page.

## Changes

- S-03: after `store.trust(cwd)` succeeds, `init` prints `toolr: trusted <repo>` to stderr (the
  line `project trust` prints), unless `--quiet`.
- S-04: route `self cache list`'s repo column, hashing and freshness errors, `DiscoveryError::MissingPyproject`'s `tools` path
  and the `dispatch.rs` interpreter-not-found / spawn messages through `shown()`.
- U-08: when clap rejects an unknown top-level command and discovery found no project (`NotFound`),
  print clap's error, then `toolr: no toolr project found above <cwd> (looked for
  tools/pyproject.toml)`. A bare `tools/` already gets its own warning.
- X-03: `toolr-core` uses `dunce::canonicalize` in discovery (new dependency, optional, in `full`).
  A `cfg(windows)` test asserts the root carries no `\\?\` prefix.
- X-05: `spawn_runner` sets `PYTHONIOENCODING=utf-8` (stdio only; UTF-8 mode would change `open()`'s
  default and be inherited by every child) unless the caller set a non-empty value.

## Non-goals

S-01 (accepted as-is), U-04 (`ctx.prompt`, other package), UTF-8 mode, any wider path
display refactor.
