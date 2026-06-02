# Design: align `toolr project venv` with uv's flag model

- **Date:** 2026-06-02
- **Status:** Design
- **Tracking issue:** [#288 — Wrap remaining uv operations under
  `toolr project venv`](https://github.com/s0undt3ch/ToolR/issues/288)
- **Branch:** `venv-uv-parity`, stacked on `mise-enter-auto-sync` (PR #289)
- **Target release:** 0.22 (same release that introduces the `deps` → `venv`
  rename — the rename and this realignment land together)

## Motivation

The `mise-enter-auto-sync` branch (PR #289, unreleased) renames
`project deps` → `project venv` and introduces `project venv upgrade <pkg>`.
While shipping that, the upgrade subcommand diverged from uv's own surface:
uv expresses package upgrades as **flags on `lock` and `sync`** (`-U` /
`--upgrade` and `-P` / `--upgrade-package`), not as a standalone verb.
Anyone familiar with uv will reach for `venv sync -U <pkg>` first; today
they get a clap error and have to learn a toolr-specific `venv upgrade`
shape.

Issue #288 also asks us to extend the wrapper to cover the rest of the
day-to-day uv project workflow (`add`, `remove`, `lock`), with `pip` as
an open question.

This design folds both into one move: `venv upgrade` is removed,
`venv sync` and a new `venv lock` mirror uv's flag model exactly, and
`venv add` / `venv remove` round out the wrapper. `venv pip` is explicitly
deferred.

## Non-goals

- **`venv pip`** — `uv pip` is an unstructured escape hatch (install,
  uninstall, freeze, compile, list, show, tree, check, sync-from-
  requirements). It bypasses `tools/pyproject.toml` and `tools/uv.lock`,
  the two artifacts toolr exists to manage. Wrapping it means either
  passing through everything (we now own a moving surface) or arbitrarily
  subsetting it. Users who genuinely need it can run
  `uv pip --project tools/ ...` directly. Revisit when a concrete request
  lands.
- **`venv add --editable` / `--dev` / `--optional <group>` / `--extra`** —
  editable deps already flow through `tools/uv-toolr-overrides.toml`
  (`crates/toolr-core/src/venv/editable.rs`); wiring `--editable` through
  `venv add` would collide without a separate design pass. The tools venv
  is itself the "dev environment," so `--dev` has no clear meaning. Out
  of scope; flagged as follow-ups if real needs surface.
- **Linking against uv as a library** — already rejected in
  `specs/archive/2026/2026-06-01-mise-enter-auto-sync-design.md`
  ("Rejected alternatives").

## Command surface

```text
toolr project venv sync     [--force] [--quiet]
                            [-U|--upgrade]
                            [-P|--upgrade-package <pkg>]...

toolr project venv lock     [--quiet]
                            [-U|--upgrade]
                            [-P|--upgrade-package <pkg>]...

toolr project venv add      <package>[@<version>]... [--quiet]
toolr project venv remove   <package>...             [--quiet]
```

Existing subcommands that don't change: `path`, `shell`.

### Removed: `venv upgrade`

Hard removal. The subcommand never shipped — it lives only on the
unmerged `mise-enter-auto-sync` branch — so there is no deprecation
period. Anyone tracking the unreleased branch migrates to
`venv sync -U <pkg>` (or `venv sync -P <pkg>` — see below).

### Flag semantics (mirror uv exactly)

| Flag | Meaning |
|---|---|
| `-U` / `--upgrade` | Re-resolve **all** packages, ignoring the existing `uv.lock` pins. No value. |
| `-P` / `--upgrade-package <pkg>` | Re-resolve only `<pkg>`. Takes a value. **Repeatable** (`-P foo -P bar`). |
| `--quiet` / `-q` | Pass `--quiet` to the uv subprocess. |
| `--force` / `-f` (sync only) | Re-run `uv sync` even when the freshness stamp says the venv is up to date. Not on `lock` (locking always re-resolves). |

`-U` and `-P` are not mutually exclusive — uv accepts both together
(`-U` is the broad sweep, `-P` is "and also definitely re-lock these").
toolr does not add a guard.

### Behavior matrix

| Invocation | What it does |
|---|---|
| `venv sync` | Existing behavior — re-sync if stale (per the freshness stamp), no-op if fresh. |
| `venv sync --force` | Existing behavior — always re-sync. |
| `venv sync -U` | `uv sync --upgrade` — all packages re-locked + installed. |
| `venv sync -P foo` | `uv sync --upgrade-package foo` — replaces today's `venv upgrade foo`. |
| `venv sync -P foo -P bar` | Multiple packages re-locked + installed. |
| `venv lock` | `uv lock` — refresh `tools/uv.lock` only, no install. |
| `venv lock -U` | `uv lock --upgrade`. |
| `venv lock -P foo` | `uv lock --upgrade-package foo`. |
| `venv add foo` | `uv add foo --project tools/` — edits `tools/pyproject.toml`, re-locks, syncs. |
| `venv add foo@0.27 bar` | Multiple packages, mixed pin styles (uv parses the spec). |
| `venv remove foo` | `uv remove foo --project tools/` — edits pyproject, re-locks, syncs. |
| `venv remove foo bar` | Multiple packages. |

After `venv lock` runs but `venv sync` has not, the lock mtime exceeds
the freshness stamp → the next `venv sync` correctly detects
`Freshness::Stale` and applies. This is the intentional split: `lock`
is the "refresh `uv.lock` after editing `pyproject.toml`" verb; `sync`
is the "apply current lock to the venv" verb.

`-U` / `-P` on `venv sync` bypass the freshness short-circuit — the
user is explicitly asking for movement, so `sync_if_needed` runs uv
even when the stamp says fresh.

## Pre-flight guards

Preserve the existing typo guard from `venv_upgrade`
(`pyproject_declares_dependency` in `crates/toolr/src/project.rs`).
Apply it as follows:

| Subcommand / flag | Guard |
|---|---|
| `venv sync -P <pkg>` | `<pkg>` must be declared in `tools/pyproject.toml` (`[project] dependencies` or any `[project.optional-dependencies.*]`). |
| `venv lock -P <pkg>` | Same. |
| `venv sync -U` / `venv lock -U` | No guard — uv re-locks everything. |
| `venv add <pkg>` | **No** pre-block. `uv add` on an already-declared package is how you change a pin; blocking would be wrong. |
| `venv remove <pkg>` | `<pkg>` **must** be declared (otherwise it's a typo or stale habit). Same shape as the `-P` guard. |

All guards run before invoking uv so the error is local, fast, and
actionable.

## Core layer (`crates/toolr-core/src/venv/`)

Replace `run_uv_lock_upgrade` with a more general API. Likely split:
keep `sync.rs` for `lock` + `sync`, introduce `edit.rs` for `add` +
`remove`.

```rust
// sync.rs
pub enum UpgradeMode {
    None,
    All,                       // -U / --upgrade
    Packages(Vec<String>),     // -P / --upgrade-package <pkg> ...
}

pub fn run_uv_lock(
    uv: &UvBinary,
    tools_dir: &Path,
    resolved: &ResolvedVenv,
    upgrade: &UpgradeMode,
    quiet: bool,
) -> Result<ExitStatus>;

// run_uv_sync grows an upgrade arg; call sites that didn't care pass UpgradeMode::None.
pub fn run_uv_sync(
    uv: &UvBinary,
    tools_dir: &Path,
    resolved: &ResolvedVenv,
    upgrade: &UpgradeMode,
    quiet: bool,
) -> Result<ExitStatus>;
```

```rust
// edit.rs (new)
pub fn run_uv_add(
    uv: &UvBinary,
    tools_dir: &Path,
    resolved: &ResolvedVenv,
    specs: &[String],
    quiet: bool,
) -> Result<ExitStatus>;

pub fn run_uv_remove(
    uv: &UvBinary,
    tools_dir: &Path,
    resolved: &ResolvedVenv,
    packages: &[String],
    quiet: bool,
) -> Result<ExitStatus>;
```

All helpers inherit the env discipline already established by
`run_uv_sync`:

- Set `UV_PROJECT_ENVIRONMENT` to `resolved.venv_dir`.
- Unset `VIRTUAL_ENV` (silences uv's mismatch warning when called from
  inside a mise-managed venv).
- Pass `--python <version>` when `resolved.config.python_version` is
  set.
- Pass `--quiet` when requested.

`run_uv_add` and `run_uv_remove` touch the freshness marker on success
— uv ran sync internally, the venv reflects the new state.

`sync_if_needed` is updated to skip the freshness short-circuit when
`upgrade != UpgradeMode::None`.

`run_uv_lock_upgrade` is **removed**; nothing else calls it.

## CLI handler layer (`crates/toolr/src/project.rs`)

`dispatch_project` gains `lock`, `add`, `remove` arms; loses `upgrade`.
Each handler:

1. Discovers the project root (`discover_project_root`).
2. Runs the pre-flight guard for its argument shape.
3. Calls `ensure_venv_ready` (resolves the venv, ensures uv is
   installed).
4. Invokes the appropriate `toolr-core` helper.
5. Surfaces a one-line success message on stdout (or stays silent when
   `--quiet`).
6. Propagates uv's exit code on failure via `anyhow::bail!` with a
   precise message ("`uv lock --upgrade-package foo` failed with exit
   code N").

`venv_upgrade` is deleted. The associated `pyproject_declares_dependency`
/ `dep_name_matches` helpers stay — they're used by the new `-P` and
`remove` guards.

The unattended-mode `--quiet` guard table
(`venv_sync_unattended_quiet_exit`) extends to cover the same benign
markers when invoked via `lock`, `add`, `remove` if/when those grow
`--quiet` paths that need it. In practice the markers are tied to
`ensure_uv` + `discover_project_root`, both of which all four handlers
call, so the same guard applies unchanged.

## Migration hint

`deps_migration_hint` in `project.rs` currently maps
`project deps upgrade …` → `project venv upgrade …`. Update:

```text
project deps sync       →  toolr project venv sync
project deps upgrade …  →  toolr project venv sync -U <pkg>
```

(No `add` / `remove` hints — `project deps add/remove` never existed.)

## Tests

| File | Action |
|---|---|
| `crates/toolr/tests/project_venv_upgrade.rs` | **Delete.** |
| `crates/toolr/tests/project_venv_sync.rs` | Extend with `-U`, `-P single`, `-P repeated`, and `-U` + `-P` combined argv assertions. Stub uv binary captures argv to a file (existing pattern). |
| `crates/toolr/tests/project_venv_lock.rs` | **New.** Mirror `sync` test shape: no-op argv, `-U`, `-P`, `-P` repeated, missing-package guard. |
| `crates/toolr/tests/project_venv_add.rs` | **New.** Argv shape for single / multiple specs, `name@version` pass-through, `--quiet`, success message. |
| `crates/toolr/tests/project_venv_remove.rs` | **New.** Argv shape for single / multiple packages, missing-package guard, success message. |
| `crates/toolr-core/src/venv/sync.rs` (unit) | Replace `run_uv_lock_upgrade_passes_package_and_project_args` with three cases keyed on `UpgradeMode::{None, All, Packages([...])}`. Add identical coverage for `run_uv_sync` with each `UpgradeMode`. |
| `crates/toolr-core/src/venv/edit.rs` (unit) | New module gets argv-capture tests for `run_uv_add` and `run_uv_remove`. |
| `crates/toolr/src/builtin_completions.rs` | Rename `project_venv_offers_path_shell_sync_upgrade` → `project_venv_offers_path_shell_sync_lock_add_remove`; assert the new subcommand list. |

All subprocess tests follow the existing pattern (Unix-only,
`#!/bin/sh` stub that exits with a chosen code and optionally writes
argv to a log).

## Docs

- `docs/cli-files/` snippets regenerate via the existing regen flow
  (`toolr self build-manifest`-adjacent scripts; the snippet path is
  the same one updated by the rename PR).
- Search-and-replace any doc page that names `venv upgrade` → point
  at `venv sync -U <pkg>`.
- New short doc sections (one paragraph each) for `venv lock`,
  `venv add`, `venv remove` in the project-venv reference page.
- `CHANGELOG.md` (Unreleased / 0.22 section) — additions to the same
  BREAKING block the rename already opened:
    - "**`project venv upgrade` removed** — use `project venv sync -U <pkg>`
  / `-P <pkg>` (mirrors uv)."
    - "**New:** `project venv lock` — wraps `uv lock` for refreshing
  `tools/uv.lock` without applying."
    - "**New:** `project venv add <pkg>` — wraps `uv add` against
  `tools/`."
    - "**New:** `project venv remove <pkg>` — wraps `uv remove` against
  `tools/`."

## Risks & mitigations

- **Risk:** uv's CLI surface drifts (e.g., renames `--upgrade-package`).
  **Mitigation:** integration tests pin the expected argv. A uv rename
  surfaces as a red CI before users see breakage.
- **Risk:** `venv add` / `venv remove` mutate `tools/pyproject.toml` —
  a user with uncommitted edits could lose intent in a merge.
  **Mitigation:** uv itself does this; toolr just calls it. No
  additional safeguard for now.
- **Risk:** Pre-flight package-existence guard becomes wrong when uv's
  "is this a dependency" view diverges from our simple TOML scan (e.g.,
  uv adds a new declaration syntax).
  **Mitigation:** the guard is intentionally permissive (lower-cased
  substring + boundary check). If we ever see a false negative, switch
  the guard to "ask uv via a dry-run."

## Out of scope (explicit follow-ups, not in this spec)

- `venv pip` (escape hatch — see Non-goals).
- `--editable` / `--dev` / `--optional` / `--extra` flag mirrors on
  `venv add` (see Non-goals).
- `venv tree` / `venv show` / `venv list` — uv has them, no one has
  asked.
- Auto-running `venv sync` after a manual edit to `tools/pyproject.toml`
  (would need an editor hook or filesystem watcher; not on the table).
