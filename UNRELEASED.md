<!--
UNRELEASED.md — Queued release notes for the next release.

Append narrative entries here as PRs land. On release, the
`_prepare-release.yml` workflow folds the content of this file
into the `### Notes` subsection of both the GitHub release body
and CHANGELOG.md (under the new version's heading), then resets
this file to empty for the next cycle.

Empty between releases is the steady-state — there's no header,
no scaffolding. Just write whatever should appear in the notes.
-->

This release lands the **Rust front-end rewrite** together with a
**workspace split** and a **distribution-channel reshuffle**. The
argparse-driven Python CLI is fully retired; the `toolr` command is
now a native Rust binary, and the PyPI footprint splits into two
packages so the CLI and the Python runtime can be installed
independently.

If you only invoke `toolr ...` from a project that already ships a
`tools/` directory, the smallest migration is:

1. Install the new CLI binary (`pip install toolr`, `installation/install.sh`,
   mise, or a GitHub release archive — see below).
2. Run `toolr project init` from your repo root. This scaffolds the
   new `tools/pyproject.toml` (with `toolr-py` already declared) and a
   `tools/uv.lock` alongside your existing `tools/*.py` scripts.
3. Move any Python dependencies your `tools/*.py` previously pulled in
   (e.g. via the project's main `pyproject.toml` dev group) into the
   `[project.dependencies]` list inside the new `tools/pyproject.toml`,
   then run `toolr project deps sync`.
4. **Commit `tools/pyproject.toml` and `tools/uv.lock` to git.** Both
   are part of the per-project tools venv contract — without them,
   collaborators and CI can't reproduce your tools venv.

The sections below spell out every other place this rewrite is visible
from the outside.

## ⚠ Breaking changes

### `python -m toolr` is gone

- **What changed:** the `[project.scripts] toolr` console entry
  point and `toolr/__main__.py` have been removed. The Python
  package no longer ships a CLI at all — invoking `python -m toolr`
  fails with `No module named toolr.__main__`.
- **Migration:** install the new Rust CLI (via `pip install toolr`,
  `install.sh`, mise, or a GitHub release archive — see below) and
  invoke it as `toolr <args>`. The argument surface is unchanged;
  only the entry point moved.

### `pip install toolr` no longer makes `import toolr` work

- **What changed:** PyPI now hosts **two** packages. `toolr` is a
  binary-only wheel (`bindings = "bin"`) — it drops the `toolr`
  executable into the wheel's `scripts/` directory and has **no
  Python source**. The Python runtime (`import toolr`, `Context`,
  `command_group`, `toolr.utils`, the `_rust_utils` extension)
  lives in a separate package, `toolr-py`.
- **Migration:** if your project's `tools/*.py` scripts do
  `from toolr import ...`, declare `toolr-py` as a dependency of
  the tools venv. The fastest path is `toolr project init` from
  your repo root — it scaffolds `tools/pyproject.toml` with
  `toolr-py` already declared and a matching `tools/uv.lock`.
  Both files belong in git. The CLI on `PATH` will then find
  `toolr-py` when it shells out to execute a command. See the
  [installation docs](https://toolr.readthedocs.io/en/latest/installation/)
  for the full layout.

### mise plugin: external `mise-toolr` repo retired

- **What changed:** the mise plugin used to be hosted out-of-tree
  at `s0undt3ch/mise-toolr`. It is now self-contained at
  `installation/mise/` inside this repo, and the external
  `mise-toolr` repository is retired. The old
  `mise plugin add toolr https://github.com/s0undt3ch/mise-toolr`
  installation stops working.
- **Migration:**

  ```sh
  mise plugin remove toolr   # if previously installed from the old path
  mise plugin add toolr https://github.com/s0undt3ch/ToolR.git#installation/mise
  mise use --global toolr@latest
  ```

### The argparse Python CLI internals were deleted

- **What changed:** `toolr/__main__.py`, `toolr/_parser.py`, and
  `toolr/_registry.py` have been deleted. Anything that imported
  `Parser`, `CommandRegistry`, or other internals from those
  modules will break.
- **Migration:** the user-facing decorator surface
  (`command_group`, `@command`, `CommandGroup`,
  `MANIFEST_SCHEMA_VERSION`) is preserved. It has moved to
  `toolr._decorators` and is re-exported from the top-level
  package, so the public form continues to work:

  ```python
  from toolr import command_group, command, CommandGroup
  ```

  If you were reaching into `toolr._registry` or `toolr._parser`
  directly, there is no replacement — the Rust binary owns
  manifest discovery, argument parsing, and dispatch now. The
  Python runtime is invoked per-command via a JSON spec file
  (`toolr._runner`).

### `testing.py` import surface tightened

- **What changed:** `toolr.testing` previously re-exposed a few
  helpers that leaned on the now-deleted `_parser` /
  `_registry` modules. Those have been replaced or removed as
  part of the three-way test prune; the supported public surface
  is whatever `toolr.testing` exports today.
- **Migration:** if your test suite imported internal helpers
  from `toolr.testing` and the import now fails, lean on the
  documented `Context` / `command_group` factories or open an
  issue describing the use case.

### `rich-argparse` is no longer pulled in

- **What changed:** `rich-argparse` was only used by the deleted
  `_parser.py`. It has been dropped from `toolr-py`'s
  dependencies. Anything that relied on toolr transitively
  bringing it into the tools venv will need to declare it
  explicitly.
- **Migration:** add `rich-argparse` to your own
  `pyproject.toml` if you depend on it for non-toolr code.

## 🚀 New features

- **Rust CLI binary.** `toolr` is now a native binary built from a
  Cargo workspace. Manifest discovery, argument parsing, help
  rendering, and command dispatch all run in Rust. Python is only
  involved at execution time (per-command subprocess via
  `toolr._runner`), so cold-start latency drops dramatically and
  shell completion is no longer gated on Python import overhead.
- **Three install channels, one source of truth.** The same
  `toolr` binary ships through:
    - `pip install toolr` (a new `py3-none-<plat>` binary wheel),
    - `curl ... | sh` via `installation/install.sh`,
    - mise via `installation/mise/`,
    - GitHub Release archives (`toolr-<version>-<target>.tar.gz`,
      with `.sha256` siblings).

  All four are produced from the same workspace build and share a
  single version number.
- **`toolr-py` PyPI package.** A standalone wheel providing
  `import toolr` for user tool scripts — declared in
  `tools/pyproject.toml`, materialised into the tools venv by
  `uv sync`. Decouples "what CLI you have on PATH" from "what
  Python bindings your tool scripts pin."
- **Python 3.14 support.** Added to the test matrix and the
  `toolr-py` classifier list.
- **Per-project `tools/` venv with uv.** The Rust binary
  materialises (and, if needed, bootstraps) a `tools/` venv via
  `uv` before each execute. Includes missing-dependency
  diagnostics, manifest caching, and cache pruning. See the
  rebuilt [installation /
  usage](https://toolr.readthedocs.io/en/latest/) docs for the
  end-to-end story.
- **Native shell completion.** Generated by the Rust frontend
  (clap-based), available for the usual shells.
- **In-repo mise plugin smoke test.** The plugin lives at
  `installation/mise/` and is covered by the same end-to-end
  smoke harness as the other install channels.
- **SLSA build provenance on every shipped artifact.** Every
  wheel (`toolr-*.whl`, `toolr_py-*.whl`), sdist, per-triple
  binary archive (`toolr-<version>-<triple>.tar.gz` /
  `.zip`), and the release notes / patch files carry a
  cryptographically signed attestation generated by
  `actions/attest-build-provenance`. Verify any of them with:

  ```sh
  gh attestation verify <file> --owner s0undt3ch
  ```

  `install.sh` already passes `--verify-attestation=require` to
  reject any archive whose attestation does not validate. See
  GitHub's [artifact attestations
  docs](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations-to-establish-provenance-for-builds)
  for the full verification model.

## 🗺 Migration cheat-sheet

| If you used... | Replace with... |
|---|---|
| `python -m toolr ...` | `toolr ...` (install the CLI via pip / install.sh / mise / release archive) |
| `pip install toolr` to get `import toolr` | `pip install toolr-py` (or run `toolr project init` to scaffold `tools/pyproject.toml` + `tools/uv.lock` with `toolr-py` declared) |
| `mise plugin add toolr https://github.com/s0undt3ch/mise-toolr` | `mise plugin add toolr https://github.com/s0undt3ch/ToolR.git#installation/mise` |
| `from toolr._registry import command_group` | `from toolr import command_group` |
| `from toolr._registry import CommandGroup` | `from toolr import CommandGroup` |
| `from toolr._parser import Parser` (and friends) | No replacement — the Rust binary owns parsing now |
| Relying on `rich-argparse` via toolr | Depend on `rich-argparse` directly |

## 🧱 Internal — for contributors

These changes affect anyone hacking on toolr itself but are
invisible to end users:

- **Cargo workspace split.** Three crates under `crates/`:
    - `toolr-core` — private library (no pyo3, no clap). Manifest
      discovery, AST parsing, manifest cache, venv plumbing.
    - `toolr` — the binary crate (`bindings = "bin"`). Depends on
      `toolr-core` plus `clap` and `termimad`.
    - `toolr-py` — the pyo3 dynlib + Python source
      (`bindings = "pyo3"`, `module-name = "toolr.utils._rust_utils"`).
- **Python source location.** Moved from `python/toolr/` to
  `crates/toolr-py/python/toolr/`. The repo-root `python/`
  directory is gone.
- **Two PyPI wheels, one workspace version.** `toolr` and
  `toolr-py` are released together at the same
  `[workspace.package] version`. Both have their own
  `pyproject.toml` and `cibuildwheel` matrices; CI fans the
  wheel builds out per crate and reassembles them at release.
- **Root `pyproject.toml` is dev-tooling only.** It retains
  `[tool.ruff]`, `[tool.mypy]`, `[tool.pytest.ini_options]`,
  `[tool.uv]`, `[tool.uv.workspace]`, `[dependency-groups]` —
  no `[build-system]`, no `[project]`, no `[tool.maturin]`.
- **No `python` feature flag.** The
  `[features] python = ["pyo3"]` dance and the
  `#[cfg(feature = "python")]` annotations are gone. pyo3 lives
  exclusively in `crates/toolr-py/` as a non-optional
  dependency.
- **`tools/version.py` simplified.** Cargo.toml writes go
  through `cargo set-version` (via cargo-edit) instead of
  hand-rolled regex edits.
- **`rich` is a direct `toolr-py` dependency.** Previously
  transitive through `rich-argparse`.
- **`UNRELEASED.md` → release notes pipeline.** The file you're
  reading is now part of every release: `_prepare-release.yml`
  strips this comment header, exports the body as
  `TOOLR_RELEASE_NOTES`, and the cliff template renders it as
  a `### Notes` section in both the GitHub release body and
  `CHANGELOG.md` under the new version's heading.
- **Dogfooding tools venv.** `tools/pyproject.toml` declares
  `toolr-py` as a workspace dependency — the repo's own
  `tools/*.py` scripts run against the same Python runtime
  users get from PyPI.

### Breaking — entry-point plugins removed

The `toolr.commands` entry-point mechanism for registering third-party
plugins is removed. Plugin authors must instead ship a static
`toolr-manifest.json` at the root of their installed Python package.
toolr's dispatch path is now pure Rust and never spawns Python just to
discover commands.

Migrating a plugin:

1. From inside the plugin's repo, run `toolr self build-manifest <pkg>`
   (replace `<pkg>` with the dotted package name). This writes a
   `toolr-manifest.json` next to your package's `__init__.py`.
2. Include the file in your built wheel. For hatchling, add this to
   `pyproject.toml`:

   ```toml
   [tool.hatch.build.targets.wheel]
   include = ["src/<pkg>/toolr-manifest.json"]
   ```

   For setuptools, add `include src/<pkg>/toolr-manifest.json` to
   `MANIFEST.in`.
3. Wire `toolr self build-manifest <pkg> --check` into CI and as a
   pre-commit hook. The `--check` flag exits non-zero when the
   committed `toolr-manifest.json` no longer matches what would be
   generated from current sources.
4. Delete the now-inert `[project.entry-points.'toolr.commands']`
   section from your plugin's `pyproject.toml`.

If you don't ship the file, your plugin's commands will not appear in
`toolr --help` or `toolr <group> --help`.

### Improved — argparse options with underscores accept both spellings

`toolr` normalises the canonical CLI form for argparse-scanned options
to dashes, so `add_argument('--skip_warm_cache', ...)` shows up in
`--help` and shell completion as `--skip-warm-cache`. The original
underscored spelling is now also accepted at parse time, so muscle
memory from the upstream tool (`--skip_warm_cache`) keeps working
without the user having to know about the rewrite.

### Improved — dispatch detects stale manifests automatically

Adding, removing, or editing `tools/*.py` is now reflected on the very
next `toolr <user-cmd>` or `toolr --help` invocation — no
`toolr project manifest rebuild` needed. Installing or upgrading a
third-party plugin that ships its own `toolr-manifest.json` is
similarly picked up automatically. The check is pure Rust and adds
single-digit milliseconds on a warm cache. When a rebuild fails (for
example a syntax error in `tools/foo.py`), toolr serves the cached
manifest with a warning identifying the offending file rather than
blocking dispatch.
