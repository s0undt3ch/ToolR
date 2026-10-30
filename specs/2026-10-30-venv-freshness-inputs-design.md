# The tools venv goes stale when its project config changes

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-04 (high).
- **Plan:** [`2026-10-30-venv-freshness-inputs-plan.md`](./2026-10-30-venv-freshness-inputs-plan.md)

## Problem

`check_freshness` (`crates/toolr-core/src/venv/sync.rs`) compares the
`.toolr-sync-stamp` mtime with `tools/uv.lock` only. When the stamp is newer,
`sync_if_needed` skips uv entirely.

Add a dependency to `tools/pyproject.toml` without re-locking, then:

- `toolr project venv sync` runs only `uv --version`, then prints
  "synced venv at …". `docs/project-config.md` says it does a "full sync
  (re-resolve + apply)".
- Dispatch runs the command on the old venv. The import fails, and the
  runner's hint (`_runner.py::_print_missing_dep_hint`) sends the user to
  `toolr project venv sync`, which does nothing.

The same applies to `python-version` and `requires-python` in
`tools/pyproject.toml`, and to `tools/.python-version`.
`docs/installation/index.md` admits this and tells users to pass `--force`.

Repro: put a logging `uv` shim first on `PATH`, sync once, edit the
dependencies in `tools/pyproject.toml`, then run `toolr project venv sync`.
The log shows only `--version`.

## Decision

Freshness covers every file uv reads from `tools/` to build the venv:

- `tools/uv.lock` (already covered; still required, as today: absent → `Missing`)
- `tools/pyproject.toml`
- `tools/.python-version`, if it exists

The venv is `Fresh` only when the stamp is at least as new as every file
that exists. A missing optional input is ignored. `uv sync` re-locks when
`pyproject.toml` changed, and the stamp is written after it, so one sync
makes the venv fresh again.

The fix lives in `check_freshness`, so all its callers get it: dispatch's
`resync_if_needed`, `toolr project venv sync`, `venv shell`, `venv run`
and `venv run --no-sync`.

`Freshness::Stale` messages say which inputs are covered:
"the tools venv is out of date with tools/pyproject.toml, uv.lock or
.python-version". The wording appears in `dispatch.rs::resync_if_needed`
and in `project.rs` (`venv run --no-sync`).

### Rejected

- **Make explicit `venv sync` always run uv.** `venv sync --quiet` runs
  from the mise enter-hook recipe (`docs/installation/mise.md`) on every
  directory change. Spawning uv each time adds its startup cost to every
  `cd`. `--force` already exists for "sync anyway".
- **Content hash instead of mtime.** It would skip a sync after a
  formatter rewrites `pyproject.toml` without changing it. That needs a
  stored hash and a parse. An extra no-op `uv sync` costs less than that
  machinery.

## Scope and compatibility

- No schema change. No new CLI flags.
- Still not covered: `UV_*` environment settings. `--force` stays the way
  to apply those, and the docs say so.
- Docs:
    - `docs/project-config.md`: replace "full sync (re-resolve + apply)"
      with what `venv sync` actually does. It syncs when
      `pyproject.toml`, `uv.lock` or `.python-version` changed since the
      last sync, and `--force` makes it sync unconditionally.
    - `docs/installation/index.md`: changes to `python-version`,
      `requires-python` and `tools/.python-version` now apply on the next
      run. Only `UV_*` settings need `--force`.
    - `docs/cli.md` `venv sync` section: name the three inputs.
    - `cli.rs` `venv sync` about text and the regenerated
      `docs/cli-files/project-venv-sync-help.txt`, if the text changes.
- `UNRELEASED.md`:

  > Editing `tools/pyproject.toml` or `tools/.python-version` now marks the
  > tools venv as out of date, the same way a changed `tools/uv.lock`
  > does. The next command, or `toolr project venv sync`, re-syncs it.
  > Before, a dependency added to `pyproject.toml` without re-locking was
  > never installed: `venv sync` printed "synced" without running uv.
  > `--force` is now only needed after changing `UV_*` settings.
