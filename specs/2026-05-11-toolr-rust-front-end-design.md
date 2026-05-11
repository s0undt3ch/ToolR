# ToolR Rust Front-End — Design

- **Date:** 2026-05-11
- **Status:** Design — pending implementation plan
- **Author:** Pedro Algarvio (brainstormed with Claude)

## Summary

Today's `toolr` is a Python entrypoint that parses CLI arguments, discovers
Python modules under `tools/`, and dispatches commands in-process via argparse.
Python boot (~150–300 ms) is paid on every invocation, including `--help` and
shell completion. Toolr also offers no useful CLI feedback when the project's
Python dependencies are missing or no Python is installed.

This design replaces the entrypoint with a Rust binary that owns CLI parsing,
help rendering, shell completion, and command manifest management. Python is
only spawned at command execution time, inside a `tools/`-specific virtualenv
managed by uv. The Rust binary builds a manifest of commands derived from
`tools/**/*.py` via static AST parsing, optionally augmented by a Python-driven
dynamic overlay. Result: `--help` and tab completion become sub-50 ms operations
that work even with project dependencies missing or no system Python installed.

The existing `tools/*.py` authoring API (`command_group(...)`, `@group.command`)
is preserved unchanged.

## Goals

- **A. Instant `--help` and shell completion.** Target: <50 ms cold, <10 ms warm.
  Per-directory dynamic completions driven by the current repo's `tools/`.
- **B. No system Python required to install or use toolr.** Bootstrap path:
  install the toolr binary → toolr asks consent to install uv → uv installs
  Python via `python-build-standalone`.
- **C. Decouple toolr's core from per-repo Python compatibility.** The toolr
  binary is indifferent to which Python version the project uses; only the
  executed tools care.

## Non-goals

- Replacing Python at runtime — tool function bodies remain Python. This is not
  a Ruff-style rewrite of user code.
- Managing the project's own dependencies — toolr only owns `tools/` deps.
- Becoming a build system, generic task runner, or CI orchestrator.
- Re-implementing uv. Toolr depends on uv.

## Hard constraints

- **Backwards compatibility.** Existing `tools/*.py` files using
  `command_group()` and `@group.command` decorators must work unchanged. No
  required migration for users on the existing API.
- **Independent locking.** Tool dependencies and project dependencies must lock
  independently. Changes to one must never churn the other's lock.
- **No Python prerequisite for toolr itself.** The primary install path (the
  standalone binary) must not require any pre-existing Python on the user's
  machine. Python is only required to actually *execute* tools. The pip wheel
  remains available as an alternative delivery channel for users already in
  Python land, but is not the canonical install path.

## Architecture overview

```text
┌──────────────────────────────────────────────────────────────────┐
│ Rust binary  (single executable, `toolr`)                        │
│  - clap-based CLI parsing                                        │
│  - manifest load / hash / regenerate                             │
│  - static AST parser via ruff_python_parser                      │
│  - shell completion (__complete subcommand)                      │
│  - venv + cache management                                       │
│  - subprocess orchestration                                      │
│  - toolr-built-in commands (cache, completion, bootstrap, ...)   │
└──────────────────────────────────────────────────────────────────┘
                              │
                              │ on `toolr <user-cmd>` execute
                              ▼
┌───────────────────────────────────────────────────────────────────┐
│ uv  (binary on PATH, or $XDG_DATA_HOME/toolr/bin/uv if installed) │
│  - downloads python-build-standalone for required Python version  │
│  - creates the tools venv                                         │
│  - syncs tools/uv.lock                                            │
└───────────────────────────────────────────────────────────────────┘
                              │
                              │ tools venv ready
                              ▼
┌──────────────────────────────────────────────────────────────────┐
│ Python  (interpreter inside the tools venv)                      │
│  - python -m toolr._runner                                       │
│  - reads spec from $TOOLR_SPEC_FILE                              │
│  - imports tools.<module>, builds Context, calls the function    │
└──────────────────────────────────────────────────────────────────┘
```

## Components

### Static manifest layer

A pure-Rust scan that combines two filesystem-only sources of truth:

1. AST parsing of `tools/**/*.py` via `ruff_python_parser`.
2. Static manifest fragments shipped by third-party command packages,
   discovered via glob over installed package directories (see
   [Third-party packages — static manifest convention](#third-party-packages--static-manifest-convention)).

Together these produce the bulk of the command manifest. What it extracts from
`tools/**/*.py`:

- `group = command_group("name", "title", docstring=...)` calls with literal
  arguments → group definitions.
- `@group.command` decorators on functions → command definitions.
- Function signatures: argument names, type annotations, defaults, varargs,
  keyword-only flags.
- Module-level `__doc__` (substituted into `command_group(docstring=__doc__)`).
- Function docstrings, including `Args:` sections (existing
  `toolr-rust-utils::docstrings` already parses these).
- Top-level imports per file → recorded for missing-dependency diagnostics
  (see [Missing-dependency diagnostics](#missing-dependency-diagnostics)).
- Local enum / `typing.Literal[...]` definitions → resolved for argument value
  completion. Cross-file resolution within `tools/` is supported by building a
  symbol table over all parsed files.

What it does **not** see:

- Dynamic registrations in `tools/*.py` (loops, conditional decorators,
  `getattr`, runtime factory functions).
- Third-party command packages that have **not** adopted the static manifest
  convention (legacy packages requiring `importlib.metadata` introspection).
- Type annotations referencing names imported from outside `tools/`.

These cases fall to the dynamic layer.

### Third-party packages — static manifest convention

Third-party command packages have historically required Python introspection
(`importlib.metadata`) to discover, because their commands register at import
time. This design lifts that requirement by defining a convention: packages
ship a `toolr-manifest.json` file at the root of their installed package
directory, which toolr discovers via a single glob over site-packages.

**The convention.** A toolr command package places `toolr-manifest.json` at
the root of its top-level Python package:

```text
site-packages/
  my_toolr_pkg/
    __init__.py
    toolr-manifest.json    ← here
    commands.py
```

Each manifest must include `"toolr_schema_version": <int>` as a top-level key
— this serves two purposes:

1. **Schema versioning.** Toolr supports reading manifests at all schema
   versions it knows about, applying in-process migrations as needed. A
   package built against schema v1 keeps working when toolr's current schema
   is v3.
2. **Accidental-pickup guard.** A file named `toolr-manifest.json` that
   lacks the version key (or has a value newer than the running toolr binary
   knows) is rejected with a clear diagnostic, not silently merged.

At package build time, toolr provides a build helper (`toolr.build` Python
module) that auto-generates the manifest from the package's `command_group`
and `@group.command` declarations. Package authors do not write the JSON by
hand. See the next section.

**Discovery (Rust, no Python).** Toolr globs:

```text
<tools-venv>/lib/python*/site-packages/*/toolr-manifest.json
```

For each match, parse + validate `toolr_schema_version`, merge into the
static manifest. Performance: the glob hits N packages with one `stat()` each
(sub-microsecond), and only touches files that actually exist — ~0.5 ms total
on a 500-package venv.

**Editable installs (known limitation).** `pip install -e .` typically writes
a `.pth` file in site-packages rather than copying the package directory, so
the simple glob misses the source repo's `toolr-manifest.json`. v1 documents
this as a known limitation: packages installed editable fall through to the
dynamic manifest layer (Python introspection) like legacy packages do.
Resolving this with `.pth` walking is future work.

**Backwards compatibility.** Packages that don't ship a `toolr-manifest.json`
fall back to the dynamic manifest layer (Python introspection). Over time, as
packages adopt the convention, more third-party discovery becomes a pure
filesystem operation, and the dynamic layer becomes the legacy path.

### Build helper for package authors

Package authors generate `toolr-manifest.json` using the `toolr.build` Python
module, which ships as part of the `toolr` Python package. It introspects the
package's decorator declarations in the current Python environment and emits
a schema-versioned JSON file.

**Canonical invocation:**

```console
python -m toolr.build my_toolr_pkg
```

Default output: `<package_dir>/toolr-manifest.json`. Override with
`--output <path>`.

**Programmatic API:**

```python
from toolr.build import build_manifest

build_manifest(
    package_name="my_toolr_pkg",
    output_path=None,             # default: <pkg-dir>/toolr-manifest.json
    schema_version=None,           # default: current toolr schema
)
```

The helper:

- Imports the named package in the active Python environment.
- Walks the registry populated by `command_group(...)` and `@group.command`
  decorators.
- Emits the manifest at the current schema version (or one explicitly pinned).
- Validates the result against the manifest schema before writing.

**CLI convenience wrapper:**

`toolr self build-manifest <package-name>` is a thin wrapper around
`python -m toolr.build <package-name>` for users who prefer the toolr CLI as
their entrypoint. It locates a Python interpreter (active venv, PATH, or a
`--python` override), runs the build helper in that environment, and reports
the result.

**Recommended workflow for package authors.**

1. Commit the generated `toolr-manifest.json` to the package's repo. Same
   reasoning as for the project manifest: reviewable diffs, instant
   discoverability for consumers on a fresh `pip install`, no first-run
   regen cost.
2. Add a pre-commit hook that runs `python -m toolr.build my_pkg --check`
   to fail if the committed manifest drifts from what would be regenerated.
3. Ensure `toolr-manifest.json` is included as package data in the wheel
   build (`include = ["toolr-manifest.json"]` or equivalent in the build
   backend's package-data configuration).

**Future work.** Build-backend plugins for hatchling, setuptools, and others
that hook into the wheel build to regenerate and include the manifest
automatically. Out of scope for v1.

### Dynamic manifest layer

A Python-driven introspection pass that runs inside the tools venv. Used only
for cases that cannot be resolved statically:

- Third-party packages that have **not** adopted the static manifest
  convention, or that are installed editable (legacy path).
- Dynamically-registered commands in `tools/*.py` that the static parser
  cannot see (loops, conditional decorators, factory functions).
- Argument value completers that require runtime evaluation.
- Type-resolved enum/Literal values for types imported from outside `tools/`.

When the dynamic layer runs:

- On any `toolr <cmd>` execution, if the dynamic manifest is missing or stale
  relative to the installed package set.
- During the shipped pre-commit hook.
- Explicitly via `toolr project manifest rebuild`.

It does **not** run at Tab completion time. Completion always serves the
existing cached manifest (static + dynamic layers), falling back to "no
dynamic completions" for newly-installed legacy packages until the next
rebuild.

### Missing-dependency diagnostics

`import` names are not package names (`import yaml` → `pyyaml`,
`import cv2` → `opencv-python`, `import sklearn` → `scikit-learn`). Toolr does
not attempt to map import names back to package names — that's a maintenance
burden for marginal value and inevitably wrong on some cases.

Instead, toolr surfaces a generic, actionable diagnostic that points the user
at the right next step without claiming knowledge it doesn't have:

- **Pre-flight detection** (cheap, filesystem-only, no Python). For each
  top-level import recorded by the static parser, toolr probes the venv's
  `site-packages` for `<module>/__init__.py` or `<module>.py`. Present →
  import will succeed. Missing → fail fast with:
  `import \`<module>\` not found in tools venv. A dependency may be missing — run \`toolr project deps sync\` and check tools/pyproject.toml.`
- **Out of scope for pre-flight.** Inline imports inside function bodies,
  conditional imports, dynamic plugin loaders, packages that use
  module-level `__getattr__` to lazily expose submodules. These cases will
  fail with a normal Python `ImportError` at execute time.
- **Post-mortem interception.** When the Python subprocess exits with an
  `ImportError`, toolr captures the error message and appends the same
  `toolr project deps sync` suggestion. The original Python traceback is
  preserved.

User contract: "common missing-deps surface a clear pre-flight error;
everything else surfaces a clear post-mortem error." Toolr never claims to
know which package provides a given import.

### Manifest file

- **Path:** `tools/.toolr-manifest.json`.
- **Tracked in git** by default. A shipped pre-commit hook keeps it fresh.
- **Format:** JSON for editability and debugging. Top-level `schema_version`
  field for forward-compatible evolution.
- **Contents:**
    - `schema_version`: integer.
    - `static_hash`: hash over `tools/**/*.py` contents used for fast freshness
    checks.
    - `dynamic_hash`: hash over the installed package set (versions in the venv)
    used to detect when the dynamic layer needs regeneration.
    - `groups`: array of group definitions.
    - `commands`: array of command definitions, each tagged with
    `origin: "static" | "dynamic"`.
    - Each command carries its arguments (types, defaults, help text), parsed
    docstring, source module, and required imports.

A shipped pre-commit hook config:

```yaml
- id: toolr-manifest
  name: Regenerate toolr manifest
  entry: toolr project manifest rebuild
  language: system
  pass_filenames: false
  files: ^tools/.*\.py$
```

### Shell completion

Implemented using the standard "static script delegates to binary" pattern, the
same approach as `kubectl`, `gh`, `cargo`, `rustup`, `uv`.

Static scripts (installed once via `toolr self completion install [shell]`) call
`toolr __complete <cwd> <args>` on Tab. The Rust binary:

1. Walks upward from `$PWD` to find the nearest `tools/` directory (or
   configured project root).
2. Hashes `tools/**/*.py` content.
3. If hash matches `manifest.static_hash` → serve completions from the cached
   manifest. Sub-millisecond.
4. If hash mismatches → re-parse `tools/**/*.py` in Rust on the fly, serve from
   that, optionally write back an updated manifest asynchronously. Sub-50 ms
   typical.
5. Dynamic-layer entries are always served from the cached manifest; staleness
   there is corrected at execute time, not Tab time.

Per-directory completions follow naturally because each invocation runs in the
user's current `cwd`. The same binary, in different repos, sees different
manifests.

### Tools venv (M3, isolated)

Tool dependencies are declared in `tools/pyproject.toml` and locked in
`tools/uv.lock`. This is a self-contained uv project, fully independent from
the root project's `pyproject.toml` / `uv.lock`.

```toml
# tools/pyproject.toml — minimal example
[project]
name = "toolr-tools"
version = "0"
requires-python = ">=3.11"
dependencies = ["packaging"]

[tool.toolr]
# Where the tools venv materializes. Default: "cache".
venv-location = "cache"   # or "in-tree"

# Repo-code editable install — opt-in, best-effort.
editable-install = []     # e.g. ["."] to install repo as editable
```

Venv location:

- **Default (cache):** `$XDG_CACHE_HOME/toolr/<repo-key>/venv/`, where
  `<repo-key>` is a stable hash of the fully-resolved repo path (symlinks
  followed) + the tools' python version + toolr's major version.
- **Opt-in in-tree:** `tools/.venv/`, set via
  `[tool.toolr] venv-location = "in-tree"`. Useful for users who want their
  editor's venv auto-detection to find it.

Repo-code access:

- Off by default. Tools must not silently see project source.
- `[tool.toolr] editable-install = ["."]` triggers a post-sync
  `uv pip install -e .` (or equivalents) into the tools venv.
- Best-effort: if the editable install fails (broken root pyproject, missing
  build deps), toolr logs a clear warning and continues. Tools that needed the
  repo code will surface a normal Python `ImportError` at execute time.

### Python-side runtime and dependency story

The toolr binary is Rust and has no Python dependencies of its own. Everything
the Python side needs — `Context`, `command_group`, `@group.command`, the
runner shim (`toolr._runner`), the logging/console helpers — lives in the
`toolr` **Python package**, which continues to be published on PyPI alongside
the binary.

**Rule:** the `toolr` Python package is a required dependency of every
project's tools venv. Toolr the binary refuses to operate on a venv that does
not have a compatible `toolr` package installed.

This is enforced at `toolr project deps sync` time:

- Toolr inspects `tools/pyproject.toml` for a `toolr` dependency entry. If
  missing or specifying an incompatible version, sync fails with:
  `toolr: tools/pyproject.toml must declare a \`toolr>=X.Y\` dependency. Add it and retry.`
  (A `toolr project init` scaffold writes this automatically.)
- After `uv sync`, toolr verifies `<tools-venv>/lib/python*/site-packages/toolr/`
  exists. If not, fails fast — the user has likely declared toolr with
  `--no-deps` or some other misconfiguration.

Transitive deps of `toolr` (msgspec, rich, etc.) arrive into the tools venv
automatically as part of the normal resolution. They are available to the
runner shim and to user tool code without any further declaration.

**What never happens.** Toolr never installs anything into the
`python-build-standalone` interpreter itself. PBS Python is a global, shared
resource managed by uv; installing into its site-packages would pollute every
project on the machine. All Python dependencies live in the per-project tools
venv exclusively.

**Versioning.** The `toolr` Python package and the `toolr` Rust binary share a
release cadence and version number, even though they are technically separate
artifacts. The binary embeds a minimum-supported-toolr-package version; if the
installed package is older, sync fails with a clear upgrade message.

### Python and uv bootstrap

Toolr does not handle `python-build-standalone` directly. All Python install,
venv creation, and locking is delegated to uv.

uv discovery sequence on first need:

1. **PATH check.** If `uv --version` succeeds and meets the minimum supported
   version, use it.
2. **Toolr-managed uv check.** If a previous run installed uv at
   `$XDG_DATA_HOME/toolr/bin/uv`, use it.
3. **Consented install.** Interactive prompt:

   ```text
   toolr needs uv (https://docs.astral.sh/uv/) and didn't find it on PATH.
     [I] Install it for me at ~/.local/share/toolr/bin/uv
     [M] I'll install it manually (see docs.astral.sh/uv/getting-started/installation)
   ```

   Non-interactive mode (CI, `--yes`, env `TOOLR_AUTO_INSTALL_UV=1`) proceeds
   without prompting. The toolr-managed uv is installed only to
   `$XDG_DATA_HOME/toolr/bin/uv`, never to `~/.local/bin` (which is user-owned
   PATH).
4. **Refusal.** If the user declines and uv is not on PATH, commands that need
   uv fail with a clear message linking to install docs. Commands that don't
   need uv (e.g., `toolr --help`, `toolr self completion install`, tab completion of
   static-layer commands) keep working.

Once uv is resolved:

- `uv python install <version>` provides the Python interpreter.
- `uv sync --project tools/` materializes the tools venv from
  `tools/pyproject.toml` + `tools/uv.lock`.
- If `editable-install` is configured, `uv pip install -e <path>` follows.

The Python interpreter version is taken from `tools/pyproject.toml`
(`requires-python`) or an explicit `[tool.toolr] python-version`.

### Execute model (S1)

CLI invocation flow:

1. Rust parses argv against the manifest. Validates arguments. Renders help.
2. Rust ensures the tools venv exists and is in sync (cheap mtime check against
   `tools/uv.lock`; full `uv sync` only on drift).
3. Rust writes a spec JSON to a tempfile (`tempfile::NamedTempFile`, mode
   0600).
4. Rust spawns `<tools-venv>/bin/python -m toolr._runner` with environment
   `TOOLR_SPEC_FILE=<path>`.
5. Stdin / stdout / stderr are inherited untouched. Rich's TTY detection works
   transparently.
6. The runner shim (`toolr._runner`) reads `$TOOLR_SPEC_FILE`, imports the
   declared module, constructs a `Context` from the spec, calls the target
   function with parsed arguments.
7. Subprocess exit code propagates to Rust → propagates to the shell.
8. The tempfile is auto-deleted when the Rust handle is dropped (including on
   panic).

Spec JSON shape (illustrative):

```json
{
  "schema_version": 1,
  "group": "ci",
  "command": "generate_build_matrix",
  "module": "tools.ci",
  "function": "generate_build_matrix",
  "args": { "name": "Alice" },
  "context": {
    "repo_root": "/path/to/repo",
    "verbosity": "normal",
    "timestamps": false,
    "log_level": "INFO"
  }
}
```

Why this transfer mechanism: portable, doesn't clobber stdin (which user tools
may need to read from), debuggable post-mortem, no argv/env size limits.
Microsecond overhead is irrelevant next to Python boot. See the
brainstorming dialogue for the comparison matrix.

**Wire format.** JSON. Rust side serializes with `serde_json`; Python side
deserializes with `msgspec.json.decode(data, type=SpecSchema)` — which is
~3× faster than the stdlib `json` module and provides structured schema
validation in the same call. msgspec is a transitive dependency of the
`toolr` Python package (see
[Python-side runtime and dependency story](#python-side-runtime-and-dependency-story)),
which is itself a required tools-venv dependency, so msgspec is always
available to the runner. Switching to a binary format (msgpack, etc.) would
save ~100 µs on a ~150–300 ms Python boot — invisible in practice — so JSON
stays for readability and debuggability.

A later optimization, Unix-only, could pass the spec via an inherited file
descriptor (`TOOLR_SPEC_FD=3`) for fully ephemeral, in-memory delivery. The
tempfile path remains the documented default.

### Cache management

Each cached venv writes a `meta.json` at creation:

```json
{
  "repo_path": "/abs/path/to/repo",
  "toolr_version": "1.0.0",
  "python_version": "3.13.1",
  "created_at": "2026-05-11T12:00:00Z",
  "last_used_at": "2026-05-11T12:34:56Z"
}
```

`last_used_at` is updated cheaply on each toolr invocation (one mtime touch).

CLI surface:

- `toolr self cache list` — show all cached venvs with origin repo, size, and
  last use.
- `toolr self cache prune` — remove orphans (repo path no longer exists) and
  stale (last_used_at older than configurable threshold, default 30 days).
- `toolr self cache prune --all` — delete everything in the toolr cache.

Passive hint: on any invocation, if the toolr cache exceeds a configurable size
threshold (default 1 GB) or has more than N orphan entries (default 10), emit a
single-line suggestion: `toolr: cache has 14 stale entries (~2 GB). Run \`toolr self cache prune\` to clean up.` No automatic deletion.

### CLI surface — toolr-built-in commands

All toolr-built-in commands live under one of two reserved namespaces:
**`toolr self <...>`** for operations on toolr-the-binary's own state, and
**`toolr project <...>`** for operations on the current repo's `tools/`. The
top level is reserved for user-defined commands from `tools/` (plus the hidden
`__complete` endpoint and standard `--version` / `--help` flags).

This gives a clean mental model:

Where | Owns
---|---
`toolr <user-group> <...>` | User-defined commands from `tools/`.
`toolr self <...>` | Toolr's own state, never the current repo.
`toolr project <...>` | Operations on the current repo's `tools/`.
`toolr __complete <...>` | Hidden, used by shell completion scripts.
`toolr --version`, `--help` | Standard flags, work at any level.

User commands cannot collide with built-ins because the `self` and `project`
namespaces are reserved.

**`toolr self <...>`** — operations on toolr-the-binary's own state. Never
touch the current repo.

- `toolr self cache list | prune | prune --all` — global venv cache management
  (see [Cache management](#cache-management)).
- `toolr self completion install [shell]` — install shell completion script
  for `bash`, `zsh`, or `fish` into the standard location for that shell.
- `toolr self completion print [shell]` — print the completion script to
  stdout (for users who want to manage installation themselves).
- `toolr self update` (future, not in v1) — update the toolr binary in place.

**`toolr project <...>`** — operations on the current repo's `tools/`. All of
these implicitly walk up from `$PWD` to locate the repo's `tools/` directory.

- `toolr project manifest rebuild` — regenerate `tools/.toolr-manifest.json`,
  both static and dynamic layers. Invoked by the shipped pre-commit hook.
- `toolr project deps sync` — force `uv sync` of the current repo's tools
  venv from `tools/pyproject.toml` + `tools/uv.lock`.
- `toolr project venv path` — print the absolute path to the tools venv.
- `toolr project venv shell` — spawn a subshell with the tools venv
  activated.

Future additions under `toolr project <...>` to consider, deliberately omitted
from v1 unless we decide otherwise:

- `toolr project init` — scaffold `tools/` in a fresh repo.
- `toolr project deps add | remove | upgrade <pkg>` — convenience wrappers
  over `uv add/remove/sync --upgrade` from inside `tools/`.
- `toolr project manifest show | validate` — pretty-print or CI-check the
  manifest without rewriting it.

### Distribution (D1)

Primary install path: standalone binary, no Python prerequisite.

Channels:

- **GitHub releases** with platform archives:
  `toolr-x86_64-unknown-linux-gnu.tar.gz`, `toolr-aarch64-apple-darwin.tar.gz`,
  `toolr-x86_64-pc-windows-msvc.zip`, etc.
- **`curl -fsSL https://...install.sh | sh`** style installer fetching the
  right archive for the host.
- **mise plugin** at `toolr-mise/` in this repo (already exists). Update to
  fetch the new Rust binary archives.
- **pip wheel** still published. The wheel packages the same Rust binary plus
  the Python runner shim, so `pip install toolr` continues to work for users
  already living in Python land. Maturin's `bin` target supports this.
- **brew tap** — later.

### Backwards compatibility

- Existing `tools/*.py` files using `command_group()` and `@group.command`
  decorators must continue to work without modification.
- Existing third-party command packages registered via `importlib.metadata`
  entry points continue to work — they are discovered via the dynamic layer.
- The Python package `toolr` still installs via pip. Its API (`Context`,
  `command_group`, `@group.command`, exceptions, console utilities) is
  preserved.
- Replaced: the argparse-driven Python entrypoint. `python -m toolr` continues
  to work as a thin shim that locates the `toolr` binary on PATH and execs it
  (printing a one-time deprecation note). Programmatic callers that imported
  `toolr.__main__:main` directly must migrate to invoking the binary.

## Risks and open questions

- **Dynamic registrations in user code.** Some real-world tools may rely on
  patterns the static parser cannot see (loops registering commands, factory
  functions returning command callables). These fall to the dynamic layer
  exclusively, which means they will not appear in `--help` or completion until
  the dynamic layer runs. Documented limitation; expected to be uncommon in
  practice.
- **Cross-platform tempfile cleanup.** `tempfile::NamedTempFile` is well-tested
  on Unix; on Windows the cleanup-on-drop semantics differ. Need to confirm
  that crashed Rust binaries do not leak spec tempfiles on Windows in real CI.
- **Static parser scope.** v1 supports local enum/`Literal` resolution within
  `tools/`. Cross-package type resolution (e.g., types imported from the
  project or third-party packages) requires either dynamic-layer assistance or
  a larger static analysis investment.
- **uv version compatibility.** uv evolves rapidly. Toolr must pin a minimum
  uv version and verify it on first use. Major-version migrations may force a
  toolr update.
- **macOS XDG defaults.** macOS does not set `XDG_DATA_HOME` by default. Toolr
  treats `~/.local/share/toolr/` as the default if `XDG_DATA_HOME` is unset on
  any platform, which works fine on macOS in practice and stays consistent
  across platforms.

## Future work (out of scope for v1)

- **Daemon mode (S3).** A long-running Python subprocess kept warm to amortize
  boot across multiple invocations within a session. Activated by
  `TOOLR_DAEMON=1` or similar. ~5 ms per call after the first.
- **FD-pipe spec transfer (Unix).** Replace tempfile with inherited file
  descriptor for fully ephemeral spec delivery on Unix platforms.
- **Custom completer functions at Tab.** Per-argument runtime completers
  declared on commands (e.g., `@arg("branch", completer=git_branches)`). v1
  serves only static values; making dynamic completers tab-responsive requires
  paying Python boot cost on Tab or a daemon.
- **Cross-package type resolution.** Resolving types imported from outside
  `tools/` for argument-value completion.
- **Self-update.** `toolr self update` to refresh the toolr binary in-place.
- **brew tap.**
