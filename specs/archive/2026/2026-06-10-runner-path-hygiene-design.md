# Runner path hygiene: drop the implicit CWD from the interpreter, run from repo root

**Date:** 2026-06-10
**Status:** Proposed (brainstorming approved)
**Depends on:** `specs/2026-06-10-static-only-manifest-design.md` (SEC-01). This branch is stacked on
`static-only-manifest`; it assumes `_introspect.py` is gone and reuses the `untrusted_repo.rs` test harness.
**Closes:** audit finding SEC-02.

## Problem

The Python runner is launched as `python -m toolr._runner` (`crates/toolr-core/src/execute/spawn.rs:12-21`).
The `-m` switch unconditionally prepends the process's current working directory (`''`) to `sys.path`. The
runner does not set the child's cwd and does not manage `sys.path`, so:

1. **CWD shadowing.** Any `.py` file in the directory the user runs `toolr` from can shadow stdlib or
   site-packages modules imported by the runner or by the command — including toolr's own startup imports
   (`msgspec`, `rich`), since `''` is on the path before the runner's code executes.
2. **Latent correctness bug.** `import tools.*` only resolves because `''` happens to be the repo root when
   the user runs `toolr` from there. Run from a subdirectory, `''` is the subdirectory and `tools.*` would
   fail to import.

SEC-02's other reported vector — `_introspect.py` doing `sys.path.insert(0, project_root)` — is removed by
SEC-01 (the whole introspection layer is deleted), so it is out of scope here.

## Key facts established during design

- `requires-python = ">=3.11"` (`crates/toolr/pyproject.toml:9`, `crates/toolr-py/pyproject.toml:9`), so the
  `-P` interpreter flag and `PYTHONSAFEPATH` (both added in 3.11) are always available.
- `repo_root` is already in the spec wire format (`crates/toolr-core/src/execute/spec.rs:52`, read at
  `crates/toolr-py/python/toolr/_runner.py:211`). No `RUNNER_SCHEMA_VERSION` / Python `SCHEMA_VERSION` bump
  is needed.
- `spawn_runner` does not set the child's cwd, so at runner startup `os.getcwd()` is the user's invocation
  directory — available for free, no new spec field.
- After SEC-01, `spawn_runner` is the only `python -m toolr.*` entry point (the introspect spawn is gone), so
  this is a single chokepoint.

## Design

### §1 — Use the `-P` flag, not the env var

Spawn `python -P -m toolr._runner` (`spawn.rs`). `-P` enables safe-path mode for the runner interpreter only;
unlike `PYTHONSAFEPATH=1`, a command-line flag is **not** inherited by child processes that a command spawns
via `ctx.run`, so it never silently changes the path semantics of an author's subprocesses. `-P` applies at
interpreter startup, so it protects the runner's own bootstrap imports (`msgspec`, `rich`) — earlier than any
in-process `sys.path` fix could.

### §2 — Append `repo_root` to `sys.path`

With `''` gone, `import tools.*` no longer resolves. The runner appends `spec.context.repo_root` to
`sys.path` (in `run()`, before `_import_target`). Append — not insert(0) — so stdlib and the venv's
site-packages win; only `tools.*` (which nothing else provides) resolves from the repo. This eliminates
sibling shadowing entirely rather than relocating it from CWD to repo_root. It also fixes the run-from-
subdirectory import bug, because resolution no longer depends on where the user invoked toolr.

Tradeoff (accepted): a venv package literally named `tools` would shadow the repo's `tools/`. Exotic, and the
venv is the repo's own.

### §3 — Run commands from the repo root (chdir on the Rust side)

The runner is spawned with its working directory set to `repo_root` —
`Command::current_dir(repo_root)` in `spawn_runner` (`crates/toolr-core/src/execute/spawn.rs`). The child
therefore starts in the repo root (the make/cargo convention) and never sees the invocation cwd; `ctx.run`
subprocesses inherit it. Because `-P` already removed `''` from `sys.path`, the cwd is decoupled from import
resolution, so this does not reintroduce shadowing. The runner no longer calls `os.chdir`.

Why Rust, not Python: the dispatch layer already owns the spawn, knows `repo_root`, and setting the child cwd
at spawn time means the runner process is never momentarily in the invocation cwd.

### §4 — Relative path arguments resolve from repo root (documented, not rewritten)

The chdir changes what a relative path **argument** means: `toolr build ./x.py` run from a subdirectory now
resolves `./x.py` against the repo root. We do **not** silently rewrite path args (no magical path building) —
the contract is "paths are resolved from the repo root," documented in the command-authoring guide.

### §5 — Double-gated warning (on the Rust side)

To catch the one footgun §4 introduces without adding noise, toolr emits a single stderr note only when
**both** hold:

- the invocation cwd differs from the repo root, and
- at least one path-typed argument was given a relative value **on the command line**.

This lives in Rust (`crates/toolr/src/execute_build.rs::relative_path_warning`), not the Python runner,
because the dispatch layer has everything needed and more: the invocation cwd (its own process cwd, pre-spawn),
`repo_root`, each arg's `resolved_type` (`SupportedType::Path` / `AbsolutePath` / `ResolvedPath`), the extracted
`PathBuf` values, and clap's `ValueSource`. Detection is type-driven (only path-typed args — never a `str`
that merely looks path-like) and source-precise: `ValueSource::CommandLine` only, so a relative **default**
never warns (something the old `isinstance`-after-coercion check in Python could not distinguish). It also
covers the dispatcher's own args in the dispatch path. The invocation cwd is never added to `Context` or the
wire format.

Example note:

```text
toolr: note: commands run from the repo root (/abs/repo); relative path arguments resolve from there, not /abs/repo/sub
```

## Division of responsibility (target state)

Rust (`dispatch` → `execute_build` → `spawn_runner`), before the runner starts:

1. `relative_path_warning(cmd, matches, repo_root, cwd)` — emit the §5 note if gated (in `build_spec` /
   `build_dispatch_spec`).
2. `spawn_runner(python, spec_path, repo_root)` — `-P`, `current_dir(repo_root)`, `TOOLR_SPEC_FILE`.

Python runner `run(spec)` — its only remaining cwd/path job:

3. `_append_repo_root(spec.context.repo_root)` — append (not prepend) so stdlib + site-packages win.
   `PYTHONPATH` cannot express this (it prepends ahead of stdlib + site-packages), which is exactly why the
   append stays in-process rather than being passed as an env var.

## Error handling

- Missing/garbage `repo_root` in the spec is already a `SpecError` path; an unreadable `repo_root` surfaces as
  a normal `spawn_runner` I/O error (with the existing "run `toolr project venv sync`" hint nearby), not a
  silent fallback.
- Appending an already-present `repo_root` is a no-op guard (don't append twice).

## Testing

- **Warning gating (Rust, `execute_build` unit tests):** `relative_path_warning` returns a note for
  (relative path arg + cwd≠repo_root), and `None` for: cwd==repo_root; an absolute path arg; a `str` arg whose
  value looks path-like. (Pure function — takes cwd as a param, so no env dependence.)
- **`-P` argv (Rust, `spawn.rs` unit test):** `spawn_runner` builds `["-P", "-m", "toolr._runner"]`.
- **Subdir import / append (Python, `tests/runner/test_path_hygiene.py`):** run a command from a repo
  subdirectory via `run()`; assert `tools.*` imports (proves the append works and fixes the latent bug).
- **Coverage boundary:** the chdir's runtime effect and the `-P` shadowing effect both require a real spawn
  (interpreter startup / `current_dir`), so they aren't unit-tested in-process — same documented boundary as
  the original `-P` end-to-end note (the only venv-backed harness binds `toolr` from PATH). Verified manually
  against the branch binary; CI exercises the real spawn.

## Out of scope

- SEC-01's manifest/provenance work (separate, lower branch in the stack).
- Exposing the invocation cwd as a public `Context` attribute — deferred until a concrete command need
  exists (adding it later is backward-compatible).
