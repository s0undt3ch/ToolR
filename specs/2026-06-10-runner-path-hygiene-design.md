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

### §3 — Run commands from the repo root

Before invoking the command function, the runner `os.chdir(repo_root)`. This gives commands a predictable
working directory regardless of where the user invoked toolr (the make/cargo convention), and makes
`ctx.run` subprocesses run from the repo root by default. Because `-P` already removed `''` from `sys.path`,
the chdir does not reintroduce any import shadowing.

### §4 — Relative path arguments resolve from repo root (documented, not rewritten)

The chdir changes what a relative path **argument** means: `toolr build ./x.py` run from a subdirectory now
resolves `./x.py` against the repo root. We do **not** silently rewrite path args (no magical path building) —
the contract is "paths are resolved from the repo root," documented in the command-authoring guide.

### §5 — Double-gated warning

To catch the one footgun §4 introduces without adding noise, the runner emits a single stderr note only when
**both** hold:

- the invocation cwd differs from the repo root, and
- at least one coerced argument is a relative `pathlib.Path`.

Detection is type-driven, not heuristic: after `_coerce_args`, an argument is "a relative path" iff
`isinstance(value, pathlib.Path) and not value.is_absolute()` (this uniformly covers `pathlib.Path` and all
`toolr.types` path-constrained args, which coerce to `Path`). String arguments are never inspected for
path-likeness. The captured invocation cwd is a **runner-internal local** — it is not added to `Context`
(keeping the public command-author API unchanged) and not added to the wire format.

Example note:

```text
toolr: note: commands run from the repo root (/abs/repo); relative path arguments resolve from there, not /abs/repo/sub
```

## Runner control flow (target state)

In `run(spec)` (`_runner.py:413`):

1. `invocation_cwd = Path.cwd()` (before any chdir).
2. `ctx = _build_context(spec)`.
3. Append `spec.context.repo_root` to `sys.path` (before importing the target).
4. `target = _import_target(spec)`.
5. `_coerce_args(...)` → `var_args` / `kw_args` (or `parent_kwargs` for the dispatch path).
6. Warning check (§5) using the coerced values + `invocation_cwd` vs `repo_root`.
7. `os.chdir(repo_root)`.
8. `target(ctx, ...)`.

## Error handling

- Missing/garbage `repo_root` in the spec is already a `SpecError` path; `os.chdir` failure (repo_root not a
  dir) surfaces as a clear error rather than a silent fallback to the invocation cwd.
- Appending an already-present `repo_root` is a no-op guard (don't append twice).

## Testing

- **CWD shadowing:** run a command from a directory containing a planted `msgspec.py`/`secrets.py` that would
  raise or set a sentinel if imported; assert it is never imported (proves `-P` dropped `''`).
- **Subdir import:** run a command from a repo subdirectory; assert `tools.*` still imports (proves the
  append works and fixes the latent bug).
- **CWD is repo root:** a command that returns `os.getcwd()`; assert it equals the repo root.
- **Warning gating:** (a) relative `Path` arg + run from subdir → note present; (b) run from repo root → no
  note; (c) no path args → no note; (d) absolute path arg → no note; (e) `str` arg that looks like a path →
  no note.
- **No child inheritance:** a command that spawns `python -c "import sys; print('' in sys.path)"` via
  `ctx.run`; assert the child still has normal path semantics (the `-P` flag did not propagate).

## Out of scope

- SEC-01's manifest/provenance work (separate, lower branch in the stack).
- Exposing the invocation cwd as a public `Context` attribute — deferred until a concrete command need
  exists (adding it later is backward-compatible).
