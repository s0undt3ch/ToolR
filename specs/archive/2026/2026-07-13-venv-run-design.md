# `toolr project venv run` — run a command in the managed venv

- **Status:** design
- **Date:** 2026-07-13
- **Issue:** [s0undt3ch/ToolR#373](https://github.com/s0undt3ch/ToolR/issues/373)

## Problem

Running a command-package's tests (or any tool) in the toolr-managed venv is
tribal knowledge. The working invocation is:

```sh
"$(toolr project venv path)/bin/python" -m pytest tools/
```

A contributor has to know that incantation, and it only works if `pytest` was
added to `tools/pyproject.toml`. The backend/app venv usually lacks `toolr-py`,
so `pytest tools/` there fails at import. CI reimplements the same steps (build
a venv, install the tools deps, run pytest by explicit path), so local and CI
drift.

Issue #373 asks for a first-class, discoverable way to run a package's tests in
the right environment.

## Decision: a general runner, not a test command

The issue frames this as `toolr test` — a pytest convenience wrapper — and then
spends most of its length worrying about pytest lock-in, runner-agnosticism, and
"no magic." That anxiety is the signal that the *specific* shape (`test`) is
fighting the *general* need: **run one command in the managed venv**.

toolr already ships the two halves of a general primitive: `project venv path`
(locate it) and `project venv shell` (drop into it interactively). What is
missing is the non-interactive middle. We add exactly that:

```text
toolr project venv run [OPTIONS] -- <CMD> [ARGS...]
```

With this, "test" is just one call of it, CI uses it verbatim, and there is
**zero runner lock-in because there is no runner concept at all**. The
documented test one-liner becomes:

```sh
toolr project venv run -- pytest tools/
```

### Why under `project venv`, not a top-level `toolr run`

`project` is a reserved built-in group (like `self`). `project venv run` can
never collide with a user's command package. A top-level `toolr run` would sit
in the same namespace as user-defined commands and would either shadow, or be
shadowed by, a user's own `run` command/group. So there is **no top-level
`toolr run` alias** — the runner lives at `toolr project venv run` only.

## Command surface

```text
toolr project venv run [--no-sync] [--quiet] -- <CMD> [ARGS...]
```

- Runs `<CMD> [ARGS...]` inside the managed tools venv.
- `--` is the documented separator so pass-through flags (`-k foo`) are
  unambiguous. clap uses `trailing_var_arg(true)` + `allow_hyphen_values(true)`
  so `toolr project venv run pytest -k foo` also works without the explicit
  `--`.
- **At least one argument (the command) is required.** Unlike the issue's
  `toolr test`, there is no default target: a general runner has no sensible
  default. `venv run` with no command is a clap usage error.
- **toolr's own flags must precede the command (or `--`).** Because of
  `trailing_var_arg`, once the first positional is seen everything after it is
  captured for the child — so `toolr project venv run pytest --no-sync` sends
  `--no-sync` to pytest, not to toolr. The `--help` text states this explicitly.
- **`--quiet`** forwards to the auto-sync step (parity with `uv run --quiet` and
  `toolr project venv sync --quiet`); see the output section. `--quiet` only —
  no short `-q`, since nearly every runnable tool defines its own `-q`
  (pytest included) and reserving a toolr `-q` here would be a cognitive clash.

## Execution semantics

Mirror `venv shell`, but non-interactive with a supplied argv:

1. Make the venv ready (see the sync section) and obtain the `ResolvedVenv`.
2. **Validate the venv** (`validate_venv`) before spawning, so a corrupt or
   incomplete venv surfaces a real error instead of the misleading
   "couldn't find `<cmd>`" nudge below. (The default sync path already validates
   inside `ensure_venv_ready`; the `--no-sync` path validates explicitly.)
3. Set `VIRTUAL_ENV` and `TOOLR_VENV` to the venv dir; prepend `<venv>/bin`
   (`Scripts` on Windows) to `PATH`; `env_remove` any conflicting outer
   `VIRTUAL_ENV`. Reuses the existing `venv_bin_dir` / `prepend_to_path` helpers
   in `project.rs`.
4. **Preserve the caller's current working directory** — `venv run` does *not*
   `cd` to the project root (same as `venv shell` and `uv run`). Paths the user
   types are relative to where they stand.
5. Spawn the child inheriting stdio and **pass its exit code straight through**.

Because Rust's `std::process::Command` resolves a bare program name against the
child's `PATH`, `pytest`, `ruff`, and `python -m pytest` all resolve from the
venv automatically. On Windows this relies on `CreateProcess` + `PATHEXT`
appending `.exe` to a bare command name (`Scripts\pytest.exe`) — see the testing
section.

## Sync / freshness policy — auto-sync by default, `--no-sync` for CI

The default behavior mirrors the two commands users will pattern-match against:
`uv run` (which toolr wraps) and the sibling `toolr project venv shell` — **both
auto-sync a stale environment before running.** Matching them is the
least-surprising, internally-consistent choice, and it is *self-healing*: the
mtime-based freshness check (`check_freshness`, comparing the venv marker mtime
against `tools/uv.lock`) is a heuristic that can false-positive after a clone or
a CI cache restore scrambles mtimes; an auto-sync default simply re-syncs (a fast
no-op when truly fresh) instead of hard-failing on the heuristic.

**Default (no flag):** call `ensure_venv_ready` — freshness-gated `uv sync`,
same path as `toolr project venv sync`. A fresh venv is a fast no-op; a stale or
missing one is synced, then the command runs. This is the only path that needs
`uv` + the consent machinery, exactly as `venv shell` already does.

**`--no-sync`:** the CI-deterministic path. Never touch the venv — no `uv`, no
consent, no network. Resolve the venv (`resolve_venv_path`) and gate on the
read-only `check_freshness()`:

- `Missing` → exit non-zero:
  *"the tools venv hasn't been created yet — run `toolr project venv sync`"*
- `Stale` → exit non-zero:
  *"the tools venv is out of date with tools/uv.lock — run `toolr project venv sync` (or drop --no-sync)"*
- `Fresh` → proceed.

Under `--no-sync` the caller has explicitly opted into strictness, so `Stale` is
fatal (the false-positive risk is theirs to own, and the fix is one command
away). There is deliberately **no `--sync` flag** — auto-sync *is* the default,
so a separate opt-in would be redundant, and it would have re-coupled the pure
runner to the uv/consent machinery it otherwise avoids.

## Command-not-found error

The issue asks for a clear error instead of a bare `ModuleNotFoundError`. When
the spawn fails with a not-found OS error (argv[0] isn't on the venv `PATH`),
catch it and emit an honest message that states the fact and offers the *likely*
cause as a question — we do not assert a cause we did not verify:

> toolr: couldn't find `pytest` in the tools venv.
> hint: did you forget to add it to tools/pyproject.toml (then `toolr project venv sync`)?

**Scope:** this nudge fires only when **argv[0] itself** is not found (`pytest`,
`ruff`, a bare tool). For `python -m somemodule` where the *module* is missing,
the `python` executable exists, so Python emits its own `ModuleNotFoundError`
and we pass that through unchanged — intercepting it would require parsing child
stderr, which we deliberately do not do.

## Output

**`venv run` does not echo the command it runs.** `uv run`, `poetry run`, and
`cargo run` all run the child without printing its argv first; matching them is
the least-surprising choice and keeps CI logs clean. The child owns stdout and
stderr; toolr adds nothing to them on the happy path. (The venv is discoverable
via `toolr project venv path` if anyone needs to know exactly where the command
resolved.)

The only toolr-originated output is:

- The **auto-sync step's** own progress on the default path — this is toolr/uv
  output, on stderr, exactly as `venv sync` / `uv run` already emit it.
  **`--quiet`** forwards to that sync to suppress it (parity with
  `uv run --quiet`). Under `--no-sync` there is no sync, so `--quiet` is a
  harmless no-op.
- Error messages (venv `Missing`/`Stale` under `--no-sync`, command-not-found),
  on stderr.

## Files touched

- `crates/toolr/src/cli.rs` — declare the `run` subcommand under `venv`
  (`trailing_var_arg`, `allow_hyphen_values`, `--no-sync`, `--quiet`;
  help text notes the flags-before-command ordering rule).
- `crates/toolr/src/project.rs` — `venv_run()` handler + dispatch arm; factor
  the spawn/activation into a testable helper alongside the existing shell
  helpers. Also **wire discoverability** (fix D): add the
  `toolr project venv run -- pytest tools/` one-liner to `run_project_init`'s
  next-steps output (currently lists `toolr example hello`, `project.rs:170`)
  and to the `venv sync` success hint, so the runner is findable without a
  top-level alias.
- `crates/toolr/tests/project_venv_run.rs` — new `assert_cmd` integration test:
  exit-code passthrough, command-not-found message, arg pass-through via `--`,
  no command echo on the happy path (child stdout/stderr only), and the
  `--no-sync` gate (`Missing` → error, `Stale` → error, `Fresh` → runs). The
  default auto-sync path (and `--quiet` forwarding to it) is covered where
  `end_to_end_sync.rs`-style fixtures already exercise a real `uv` sync.
- `crates/toolr/src/builtin_completions.rs` — add the entry (derived from
  `cli::build_command`).
- Docs: `docs/cli.md` + regenerated `docs/cli-files/project-venv-run-help.txt`
  (via the doc-snippets hook), plus a short "running tests / tools in the
  managed venv" note that replaces the `venv path` one-liner guidance.
- `UNRELEASED.md` — release note.

## AI skill updates

Two layers, both required so the shipped skills teach the new one-liner:

- **Auto-generated refs:** `cargo xtask build-skill-refs` (mechanical; gated by
  `--check` in `mise run test` and CI).
- **Hand-written prose:**
    - `skills/toolr-command-authoring/SKILL.md` — the "how do I run / test my
  commands" workflow moves from the `"$(toolr project venv path)/bin/python"`
  incantation to `toolr project venv run -- pytest tools/`.
    - `skills/toolr-ci-setup/SKILL.md` — the CI recipe for running a package's
  tests in the managed venv uses `toolr project venv run` instead of the
  hand-rolled venv-build-then-pytest steps.
    - Update the matching `tests/triggers.yaml` where a trigger phrase references
  the old invocation.

## Non-goals

- No `toolr test` command and no top-level `toolr run` alias (see rationale
  above).
- No `[tool.toolr]` test configuration — there is no runner concept to
  configure.
- `venv run` does not parse or rewrite child output (beyond the argv[0]
  not-found case).

## Testing / verification scope

Touches Rust and skills → full umbrella `mise run test` (skill-refs drift gate +
`cargo test --workspace` + pytest), plus `prek run --all-files` and
`mkdocs build --strict` for the docs. Regenerate doc snippets and skill refs;
do not hand-edit them.

**Windows (fix H):** bare-command resolution (`Scripts\pytest.exe` via
`PATHEXT`) is new surface. The `project_venv_run.rs` command-not-found and
exit-code-passthrough cases must run in the Windows leg of the integration
matrix (as `cli_smoke.rs` already does), not just Unix — assert against a
command that exists cross-platform (e.g. `python -c ...`) plus a
guaranteed-absent one for the not-found nudge.
