# Toolr Cargo workspace split — Design

- **Date:** 2026-05-13
- **Branch:** `design/rust-front-end`
- **Status:** Spec — companion plan: [15-plan-12-workspace-split.md](./15-plan-12-workspace-split.md)
- **Supersedes (in part):** `specs/rust-front-end/10-plan-9-distribution.md`
  Task 1 (the "maturin auto-ships `[[bin]]` in pyo3 wheels" claim is empirically false against maturin 1.8.4).

## Goal

Split the single `toolr-rust-utils` Cargo crate into a three-crate Cargo
workspace so that the standalone Rust binary and the Python pyo3 dynlib
ship as separate, independent artifacts with no shared maturin
configuration, no `python` feature flag dance, and no
`bindings = "pyo3"` / `bindings = "bin"` mode collision.

The next release (`0.20.0`) lands the Rust frontend rewrite *and* this
split together. The Python frontend (argparse-driven `python -m toolr`)
is fully retired in the same release.

## Non-goals

- No `crates.io` publication for `toolr-core` or `toolr-py`. All three
  crates carry `publish = false`.
- No `__main__.py` deprecation shim that locates and execs the binary
  (Plan 9's design point). Users opt into the new CLI by installing
  `toolr` (binary wheel or GH Releases archive); `pip install toolr-py`
  is for `import toolr` inside user tool scripts only.
- No path-traversal `python-source = "../../python"`. Python sources
  move into `crates/toolr-py/python/` so each crate is self-contained.
- No splitting the release into separate workflows or independent
  version streams. Both wheels and the binary archive ship together at
  the same workspace version.

## Decisions captured during brainstorming

1. **Topology:** three crates — `toolr-core` (private library, no pyo3),
   `toolr` (binary), `toolr-py` (pyo3 wrappers).
2. **Python frontend:** fully retire. Delete `__main__.py`, `_parser.py`,
   `_registry.py`; drop `[project.scripts] toolr`.
3. **Layout:** crates live under `crates/<name>/`.
4. **Distribution model:** two PyPI packages, both built from the same
   workspace at the same version:
   - `pip install toolr` → wheel built from `crates/toolr/pyproject.toml`
     with `bindings = "bin"`. Ships the Rust binary at
     `<wheel>.data/scripts/toolr`.
   - `pip install toolr-py` → wheel built from
     `crates/toolr-py/pyproject.toml` with `bindings = "pyo3"`. Ships the
     `_rust_utils.<abi>.so` dynlib and the Python source tree;
     importable as `import toolr`.
   - Plus the same `toolr` binary in GH Releases archives, installed
     via `install.sh` or the mise plugin (identical bits, different
     envelope).
5. **`python/toolr/` location:** moves to
   `crates/toolr-py/python/toolr/`. Repo-root `python/` directory
   disappears.
6. **Root `pyproject.toml`:** stripped of `[build-system]`, `[project]`,
   `[tool.maturin]`. Retains `[tool.ruff]`, `[tool.mypy]`,
   `[tool.pytest.ini_options]`, `[tool.uv]`, `[tool.uv.workspace]`,
   `[dependency-groups]`. Becomes "dev tooling config" only.
7. **Pyo3 feature flag:** `[features] python = ["pyo3"]` and all
   `#[cfg(feature = "python")]` annotations are deleted. pyo3 lives in
   `crates/toolr-py/` as a non-optional, non-feature-gated dependency.
8. **`clap` and `termimad`:** only used by `main.rs`; move to
   `crates/toolr/Cargo.toml`. Not part of `toolr-core`.
9. **Release coupling:** both wheels and the binary archives share a
   single workspace version (`[workspace.package] version`) and are
   published together by the release workflow. Independent build is
   possible (`cargo build -p toolr`, `maturin build -m
   crates/toolr-py/pyproject.toml`) for development.

---

## Section 1 — Architecture & repo shape

```text
toolr/                                      (repo root)
├── Cargo.toml                              workspace root + shared profile/deps/version
├── Cargo.lock                              single lockfile for the workspace
├── crates/
│   ├── toolr-core/                         private library; no pyo3, no CLI
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs                      cache, command, complete, deps_check, discovery,
│   │                                       dynamic, docstrings, execute, hash, manifest,
│   │                                       parser, project, third_party, uv, venv
│   ├── toolr/                              binary crate; depends on toolr-core
│   │   ├── Cargo.toml
│   │   ├── pyproject.toml                  bindings = "bin"
│   │   └── src/
│   │       └── main.rs                     clap CLI, glue, signal handling, subprocess spawn
│   └── toolr-py/                           pyo3 dynlib crate + Python source
│       ├── Cargo.toml
│       ├── pyproject.toml                  bindings = "pyo3"
│       ├── src/
│       │   └── lib.rs                      pyo3 wrappers (formerly src/python_bindings.rs)
│       └── python/
│           └── toolr/                      Python source tree (was python/toolr/)
├── pyproject.toml                          dev-tooling only; no build-backend
├── tests/                                  Rust + Python tests
├── tools/                                  user-side tools (unchanged; gets a new
│                                           tools/pyproject.toml declaring toolr-py dep)
├── dist/  toolr-mise/  docs/  specs/       unchanged
```

### Two distribution channels

| Channel                                       | What it ships                                                              | How it's produced                                                                                |
| --------------------------------------------- | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| GH Releases archives + install.sh + mise      | `toolr` binary (built from `crates/toolr`)                                 | `_build-binary-archive.yml` runs `cargo build -p toolr --release --bin toolr --target <triple>`  |
| PyPI wheel `toolr` (binary wheel)             | `toolr` binary at `<wheel>.data/scripts/toolr`                             | `_build.yml` invokes maturin via cibuildwheel against `crates/toolr/pyproject.toml`              |
| PyPI wheel `toolr-py` (pyo3 wheel)            | `crates/toolr-py/python/toolr/**/*` + `_rust_utils.<abi>.so`               | `_build.yml` invokes maturin via cibuildwheel against `crates/toolr-py/pyproject.toml`           |

`pip install toolr` does not make `import toolr` available; `pip install
toolr-py` is the way to get the bindings. The PyPI-package name → import-name
mismatch follows the pattern used by `pillow` → `PIL`, `python-dateutil`
→ `dateutil`, etc.

### Coupling at release time, not build time

Both crates inherit `version` from `[workspace.package]`, so they
carry the same version string. `release.yml` runs both wheel builds and
the binary-archive build in one workflow and publishes them with the
same version tag. Independent build (development) is fine: `cargo build
-p toolr` and `maturin build -m crates/toolr-py/pyproject.toml` work in
isolation.

---

## Section 2 — Crate breakdown

### `crates/toolr-core/`

Pure Rust library. No pyo3 in its dependency closure. Hosts every
module today's `src/` exports that isn't strictly pyo3 wrappers or
CLI-specific:

```text
crates/toolr-core/src/
├── lib.rs              re-exports from each module
├── cache/  (+ cache.rs counterparts)
├── command/  command.rs
├── complete/
├── deps_check/
├── discovery.rs
├── docstrings/  docstrings.rs
├── dynamic/
├── execute/
├── hash.rs
├── manifest/
├── parser/
├── project.rs
├── third_party/
├── uv/
└── venv/
```

Dependencies inherited from `[workspace.dependencies]`: `tokio`,
`serde`, `serde_json`, `anyhow`, `thiserror`, `blake3`, `chrono`,
`uuid`, `libc`, `email_address`, `pep440_rs`, `walkdir`,
`ruff_python_parser`, `ruff_python_ast`, `tempfile`, `signal-hook`,
`toml`, `dirs`, `glob`, `humansize`, `log`, `reqwest`, `which`. Windows
target carries `winapi`. No `pyo3`. No `clap`. No `termimad`. No
`[features]` block.

### `crates/toolr/`

Binary crate. Owns the CLI and nothing else.

```text
crates/toolr/src/
└── main.rs             clap CLI, signal forwarding, subprocess spawn,
                        glue. The implementation plan decides whether
                        main.rs gets split into `cli/args.rs`,
                        `cli/run.rs`, etc. based on the actual file
                        size after the move.
```

Cargo.toml: `[[bin]] name = "toolr"` at `src/main.rs`. Dependencies:
`toolr-core = { path = "../toolr-core" }`, `clap`, `termimad`,
`anyhow`, `log`. No `[lib]`. No pyo3.

### `crates/toolr-py/`

pyo3 dynlib + Python package source.

```text
crates/toolr-py/
├── Cargo.toml          [lib] cdylib + rlib, name = _rust_utils
├── pyproject.toml      bindings = "pyo3", module-name = toolr.utils._rust_utils
├── src/
│   └── lib.rs          today's src/python_bindings.rs contents: #[pymodule],
│                       #[pyclass]/#[pyfunction] wrappers, pyo3 error
│                       conversions; uses toolr_core::* underneath.
└── python/
    └── toolr/          today's python/toolr/ moves here intact.
```

Cargo.toml dependencies: `toolr-core = { path = "../toolr-core" }`,
`pyo3` (non-optional), `anyhow`. No feature gates.

The `python/toolr/` move (Section 4) deletes the three Python CLI
modules and audits `__init__.py`; the rest of the tree stays as-is.

---

## Section 3 — Configuration files

### Root `Cargo.toml` (workspace)

```toml
[workspace]
members = ["crates/toolr-core", "crates/toolr", "crates/toolr-py"]
resolver = "2"

[workspace.package]
version = "0.20.0"
edition = "2021"
authors = ["Pedro Algarvio <pedro@algarvio.me>"]
license = "Apache-2.0"
repository = "https://github.com/s0undt3ch/toolr"

[workspace.dependencies]
tokio = { version = "1.45", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
thiserror = "1"
blake3 = "1"
chrono = { version = "0.4", default-features = false, features = ["clock", "serde", "std"] }
uuid = { version = "1", features = ["v4", "serde"] }
libc = "0.2"
email_address = "0.2"
pep440_rs = "0.7"
walkdir = "2"
ruff_python_parser = { git = "https://github.com/astral-sh/ruff", tag = "0.14.0" }
ruff_python_ast    = { git = "https://github.com/astral-sh/ruff", tag = "0.14.0" }
tempfile = "3.20"
signal-hook = "0.3"
toml = "0.8"
dirs = "5"
glob = "0.3"
humansize = "2"
log = "0.4"
reqwest = { version = "0.12", default-features = false, features = ["blocking", "rustls-tls"] }
which = "6"
clap = { version = "4", features = ["derive", "env", "string", "wrap_help"] }
termimad = "0.34"
pyo3 = { version = "0.27", features = ["extension-module"] }

[profile.release]
strip = true
```

### `crates/toolr-core/Cargo.toml`

```toml
[package]
name = "toolr-core"
description = "Core domain library for toolr (no pyo3, no CLI)"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
publish = false

[dependencies]
tokio.workspace = true
serde.workspace = true
serde_json.workspace = true
anyhow.workspace = true
thiserror.workspace = true
blake3.workspace = true
chrono.workspace = true
uuid.workspace = true
libc.workspace = true
email_address.workspace = true
pep440_rs.workspace = true
walkdir.workspace = true
ruff_python_parser.workspace = true
ruff_python_ast.workspace = true
tempfile.workspace = true
signal-hook.workspace = true
toml.workspace = true
dirs.workspace = true
glob.workspace = true
humansize.workspace = true
log.workspace = true
reqwest.workspace = true
which.workspace = true

[target.'cfg(windows)'.dependencies]
winapi = { version = "0.3", features = ["handleapi", "namedpipeapi", "processthreadsapi", "winnt", "fileapi", "minwinbase"] }

[dev-dependencies]
tempfile.workspace = true
anyhow.workspace = true
libc.workspace = true
assert_cmd = "2"
```

### `crates/toolr/Cargo.toml`

```toml
[package]
name = "toolr"
description = "toolr command-line interface"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
publish = false

[[bin]]
name = "toolr"
path = "src/main.rs"

[dependencies]
toolr-core = { path = "../toolr-core" }
clap.workspace = true
termimad.workspace = true
anyhow.workspace = true
log.workspace = true
```

### `crates/toolr/pyproject.toml`

```toml
[build-system]
requires = ["maturin>=1.8,<2.0"]
build-backend = "maturin"

[project]
name = "toolr"
description = "toolr command-line interface (Rust binary distribution)"
readme = "../../README.md"
requires-python = ">=3.11"
license = { file = "../../LICENSE" }
classifiers = [
    "Development Status :: 3 - Alpha",
    "Programming Language :: Rust",
    "License :: OSI Approved :: Apache Software License",
]
dynamic = ["version"]

[project.urls]
Repository = "https://github.com/s0undt3ch/toolr"
Documentation = "https://toolr.readthedocs.io"
Issues = "https://github.com/s0undt3ch/toolr/issues"

[tool.maturin]
bindings = "bin"
strip = true
locked = true
```

No `python-source`, no `[project.scripts]`. Wheel content is
`<wheel>.data/scripts/toolr` + dist-info, nothing else.

### `crates/toolr-py/Cargo.toml`

```toml
[package]
name = "toolr-py"
description = "pyo3 bindings for toolr (Python extension module)"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
publish = false

[lib]
name = "_rust_utils"
crate-type = ["cdylib", "rlib"]

[dependencies]
toolr-core = { path = "../toolr-core" }
pyo3.workspace = true
anyhow.workspace = true
```

### `crates/toolr-py/pyproject.toml`

```toml
[build-system]
requires = ["maturin>=1.8,<2.0"]
build-backend = "maturin"

[project]
name = "toolr-py"
description = "Python bindings for the toolr framework (import as `toolr`)"
readme = "../../README.md"
requires-python = ">=3.11,<3.15"
license = { file = "../../LICENSE" }
authors = [{ name = "Pedro Algarvio", email = "pedro@algarvio.me" }]
classifiers = [
    "Programming Language :: Python :: 3 :: Only",
    "Programming Language :: Python :: 3.11",
    "Programming Language :: Python :: 3.12",
    "Programming Language :: Python :: 3.13",
    "Programming Language :: Python :: 3.14",
    "Programming Language :: Rust",
    "License :: OSI Approved :: Apache Software License",
]
dependencies = [
    "msgspec>=0.19.0",
    "rich-argparse>=1.7.0",
    "packaging>=23.0",
]
dynamic = ["version"]

[project.urls]
Repository = "https://github.com/s0undt3ch/toolr"
Documentation = "https://toolr.readthedocs.io"
Issues = "https://github.com/s0undt3ch/toolr/issues"

[tool.maturin]
bindings = "pyo3"
module-name = "toolr.utils._rust_utils"
python-source = "python"
features = []
strip = true
locked = true
```

### Root `pyproject.toml` (post-split)

No `[build-system]`. No `[project]` (so `pip install .` at root fails
loudly, which is correct). Just dev tooling and uv workspace
configuration. Path-references update to point at the new locations:

```toml
[tool.ruff]
line-length = 120
src = ["crates/toolr-core/src", "crates/toolr/src", "crates/toolr-py/src",
       "crates/toolr-py/python", "tests", "tools"]
# ... rest of [tool.ruff] preserved ...

[tool.ruff.lint.per-file-ignores]
"crates/toolr-py/python/**/*.py"                   = [ ... ]   # was "python/**/*.py"
"crates/toolr-py/python/toolr/_context.py"         = [ ... ]
"crates/toolr-py/python/toolr/utils/_rust_utils.pyi" = [ ... ]
"tests/**/*.py"                                    = [ ... ]

[tool.mypy]
mypy_path = "crates/toolr-py/python"   # was "python"
# ...

[tool.pytest.ini_options]
testpaths = ["tests/"]
# ...

[tool.uv]

[tool.uv.workspace]
members = [
    "crates/toolr",
    "crates/toolr-py",
    "tests/support/3rd-party-pkg",
]

[tool.uv.sources]
toolr    = { workspace = true }
toolr-py = { workspace = true }
"3rd-party-pkg" = { workspace = true }

[dependency-groups]
dev = [
    "3rd-party-pkg",
    "toolr-py",
    "attrs>=25.3.0",
    "coverage>=7.8.0",
    "hypothesis>=6.0.0",
    "pytest>=8.3.5",
    "pytest-skip-markers>=1.5.2",
    "pytest-subtests>=0.14.2",
]
docs = [
    "toolr-py",
    "markdown-include>=0.8.1",
    "mkdocs>=1.6.1",
    "mkdocs-awesome-nav>=3.1.2",
    "mkdocs-material>=9.6.16",
    "mkdocstrings[python]>=0.30.0",
    "ruff>=0.12.9",
]
tools = [
    "packaging>=25.0",
]
```

### `tools/pyproject.toml` (new)

Created as part of this work to declare `toolr-py` as a dep for the
project's own dogfooding tools venv (same pattern downstream consumers
will use):

```toml
[project]
name = "toolr-tools"
version = "0.0.0"
requires-python = ">=3.11"
dependencies = ["toolr-py"]
```

### Notes

- `Cargo.lock` lives at the workspace root, one file shared by all
  crates.
- `README.md` and `LICENSE` are referenced from each crate's
  `pyproject.toml` via `../../README.md` and `../../LICENSE`. Maturin
  handles relative paths.
- Both wheel `pyproject.toml`s use `dynamic = ["version"]`; maturin
  reads `version` from each crate's `Cargo.toml`, which inherits from
  `[workspace.package]`. Single source of truth.
- The existing `[tool.hatch.version]` block in root `pyproject.toml`
  goes away — it's a leftover from a pre-maturin shape.

---

## Section 4 — Python source tree changes

### Move

`python/toolr/` → `crates/toolr-py/python/toolr/`. Use `git mv` so
history follows. Once moved, the repo-root `python/` directory is
gone.

### Prune (delete as part of Python-frontend retirement)

```text
crates/toolr-py/python/toolr/__main__.py     DELETE (entry point for `python -m toolr`)
crates/toolr-py/python/toolr/_parser.py      DELETE (argparse-based parser)
crates/toolr-py/python/toolr/_registry.py    DELETE (Python command discovery)
```

### Audit

`crates/toolr-py/python/toolr/__init__.py` likely re-exports symbols
from `_parser` / `_registry`. Anything referencing those modules must
be trimmed; references to `_context`, `_exc`, `testing`, `utils`,
`types`, `_rust_utils` stay. Concrete list produced in the
implementation plan.

### Keep

Everything else under `crates/toolr-py/python/toolr/` stays. These
modules are runtime support consumed by user tool scripts and by the
Rust binary's Python subprocesses:

- `__init__.py` (trimmed)
- `_context.py` / `_context.pyi`
- `_exc.py`
- `py.typed`
- `testing.py`
- `types/`
- `utils/` (`_console.py`, `_docstrings.py`, `_imports.py`, `_logs.py`,
  `_signature.py`, `_rust_utils.pyi`, `command.py`, `__init__.py`)
- `_runner` module(s) if/when Plan 2's runner lands here.

### Path config updates in root `pyproject.toml`

| Setting                                | Before                                       | After                                                                                                                                       |
| -------------------------------------- | -------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| `[tool.mypy] mypy_path`                | `python`                                     | `crates/toolr-py/python`                                                                                                                    |
| `[tool.ruff] src`                      | `["src", "python", "tests", "tools"]`        | `["crates/toolr-core/src", "crates/toolr/src", "crates/toolr-py/src", "crates/toolr-py/python", "tests", "tools"]`                          |
| `[tool.ruff.lint.per-file-ignores]`    | `"python/**/*.py"` and variants              | `"crates/toolr-py/python/**/*.py"` and variants                                                                                             |

### What doesn't change

- `tools/` (the project's own dogfooding tools) stays at the repo root.
  It gains a new `tools/pyproject.toml` declaring `toolr-py` as a dep
  — same pattern downstream consumers will use.
- `tests/support/3rd-party-pkg/` stays as a uv workspace member.
- `docs/`, `specs/`, `dist/`, `toolr-mise/` are unaffected.

---

## Section 5 — CI & release flow

Six workflow files touched; no new workflow file.

### `_build.yml` — reusable wheel build (parameterized)

Today hard-codes building from root `pyproject.toml`. Becomes
parameterized on which pyproject to build via cibuildwheel's
`CIBW_CONFIG_FILE`:

```yaml
on:
  workflow_call:
    inputs:
      display-name:           required: true   type: string
      release-tarball-name:   required: true   type: string
      platform-matrix:        required: true   type: string
      pyproject-path:         required: true   type: string   # NEW
      cache-seed:             required: true   type: string
```

Inside the job:

```yaml
- uses: pypa/cibuildwheel@<sha>
  env:
    CIBW_CONFIG_FILE: ${{ inputs.pyproject-path }}
  with:
    package-dir: ${{ inputs.release-tarball-name }}
```

Caller workflows fan out and call it twice (once per pyproject).

### `_build-binary-archive.yml`

Two-line change to add `-p toolr` so cargo targets the binary crate
inside the workspace:

```diff
- run: cargo build --release --locked --bin toolr --target ${{ matrix.target.triple }}
+ run: cargo build --release --locked -p toolr --bin toolr --target ${{ matrix.target.triple }}
```

Same change applied to the `cross` invocation. Matrix, archive layout,
SLSA attestation, sha256, and mise-plugin smoke stay.

### `_prepare-release.yml`

Version bump now operates on `[workspace.package] version` in root
`Cargo.toml` rather than `[project] version` in root `pyproject.toml`.
The `toolr version bump` subcommand updates that location. Both wheels
inherit via `dynamic = ["version"]` on their `[project]` blocks.

`CHANGELOG.md` / `git-cliff` flow stays identical (operates on tags,
not on pyproject).

### `release.yml`

Fan-out becomes:

```text
prepare-release:        uses ./.github/workflows/_prepare-release.yml
test-*:                 uses ./.github/workflows/_test.yml          (unchanged)
build-binary-wheel-*:   uses ./.github/workflows/_build.yml
                        with: pyproject-path: crates/toolr/pyproject.toml
build-py-wheel-*:       uses ./.github/workflows/_build.yml
                        with: pyproject-path: crates/toolr-py/pyproject.toml
build-binary-archive:   uses ./.github/workflows/_build-binary-archive.yml
publish-release:        downloads artifacts and publishes to PyPI
```

`publish-release` runs two trusted-publisher publish steps — one for
PyPI project `toolr`, one for PyPI project `toolr-py`. PyPI configures
trust per-project; the maintainer adds a trusted-publisher entry to
`pypi.org/p/toolr-py` as a one-time setup task.

### `ci.yml`

Mirrors `release.yml`:

- `prepare-ci`, `pre-commit`, `prepare-release`, `test-*` — structure
  unchanged.
- `build-linux/windows/macos` split into `build-binary-wheel-*` and
  `build-py-wheel-*`, each calling `_build.yml` with the right
  `pyproject-path`.
- `docs` — uv-sync resolves through `[tool.uv.workspace]`; mkdocs gets
  the live `toolr-py` package automatically.
- `publish` — downloads both wheel artifact sets and pushes each to
  TestPyPI via separate `pypa/gh-action-pypi-publish` steps (different
  per-project OIDC trust config).

### `install-smoke.yml`

Already fits the model; one job grows a second check:

| Smoke job                | What it tests after the split                                                                                                                                       |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `smoke-install-sh`       | `install.sh` fetches GH Release archive; runs `toolr --version`. Unchanged.                                                                                         |
| `smoke-install-ps1`      | Same for Windows. Unchanged.                                                                                                                                        |
| `smoke-pip-wheel`        | Two checks per OS/arch: `pip install toolr` then `toolr --version` (binary wheel); `pip install toolr-py` then `python -c "import toolr; import toolr.utils._rust_utils"`. |
| `smoke-mise-plugin`      | Unchanged — mise plugin fetches the same GH Release archive.                                                                                                        |

### Caches

`_build.yml` and `_build-binary-archive.yml` already accept `cache-seed`
as a required input. Callers in `ci.yml` and `release.yml` continue to
pass `${{ needs.prepare-ci.outputs.cache-seed }}`. The two `_build.yml`
callers (binary-wheel vs py-wheel) pass the same seed; cache scoping is
handled by mixing `inputs.pyproject-path` into the workflow's internal
`cache_key_prefix`.

### What doesn't change

- step-security/harden-runner allowlists.
- attestation / SLSA wiring — both wheels and the binary archive set get
  `attest-build-provenance`.
- `dist/mise-plugin/` and `toolr-mise/` — fetch from GH Releases, not
  from a wheel.

### One-time maintainer actions (outside code)

1. Reserve PyPI project `toolr-py` (first publish or pre-reserve the
   name). Maintainer task on PyPI; outside the scope of this repo and already completed.
2. Configure trusted publisher on `pypi.org/p/toolr-py` pointing at
   this repo's `release.yml`. Maintainer task on PyPI and already completed.
3. Same on `test.pypi.org/p/toolr-py` for `ci.yml`'s TestPyPI publish. Maintainer task on PyPI and already completed.
4. Release notes for `0.20.0`: "`pip install toolr` no longer provides
   `import toolr`; use `pip install toolr-py`."

---

## Section 6 — Testing strategy

Three buckets — Rust, Python, distribution — and a three-way pruning
rule for the Python CLI tests.

### Rust tests

Live next to the code they exercise; one set per crate.

```text
crates/toolr-core/
├── src/                       #[cfg(test)] unit tests inline
└── tests/                     integration tests for core APIs

crates/toolr/
├── src/                       small inline units
└── tests/                     assert_cmd-based CLI behaviour tests:
                               argv parsing, signal handling, subprocess
                               spawn, exit codes

crates/toolr-py/
├── src/                       inline tests for pyo3 wrapper boilerplate
                               (purely Rust-side)
└── tests/                     probably empty — Python-side covers the
                               pyo3 surface
```

Run with `cargo test --workspace` (one invocation, all three crates).
Coverage continues via `cargo tarpaulin --workspace`.

### Python tests

Stay at repo root in `tests/`. Three-way pruning of the existing tests:

**1. Migrate to `crates/toolr/tests/` (Rust integration via `assert_cmd`).**
Tests that assert CLI *behavior* — exit codes, stream content, signal
handling, argv parsing edge cases. The Python invocation changes to a
Rust invocation; the assertions stay. Examples:

- "`toolr --help` lists registered groups."
- "`toolr <group> <command> --bogus-flag` exits 2 with usage on stderr."
- "`toolr --version` matches workspace version."
- "Signal forwarding: parent receives SIGINT → child Python subprocess
  gets SIGINT."
- "Exit code from user command propagates."

**2. Migrate in place under `tests/` (Python subprocess against the
Rust binary).**
Tests that need a Python fixture environment and assert Python-side
effects of running the CLI. The driving call changes from `python -m
toolr ...` (or in-process `_parser.parse_args`) to `subprocess.run(
["toolr", ...] )`. Examples:

- "Command discovery: with `tools/ci.py` defining `@command def
  foo(...)`, running `toolr ci foo` invokes that function with the
  right args."
- "Context object received by the user function matches the call's
  argv/env."
- "Exception in user code surfaces with the correct exit code and
  traceback."
- "`testing.py` helpers used by tool authors work end-to-end."

**3. Delete.**
Genuinely implementation-coupled tests with no behavioral content
worth preserving:

- Tests importing `toolr._parser` or `toolr._registry` and calling
  internal functions directly (those modules cease to exist).
- Tests asserting argparse-specific error message wording verbatim. The
  Rust binary emits clap-style messages with different wording;
  preserving these assertions would just be churn.
- Tests of the Python-side command registry's internal data structures.

The implementation plan produces the concrete per-file list after a
grep pass.

Run with `pytest tests/` after `uv sync --dev` at the workspace root.
`[tool.uv.workspace]` ensures `toolr-py` is installed into the dev venv
as a path-link; changes to `crates/toolr-py/**` are picked up by the
next test run. `coverage` driven by `.coveragerc`; paths in
`.coveragerc` flip from `python/toolr/` to `crates/toolr-py/python/toolr/`.

### Distribution tests (new)

New `tests/distribution/` directory holds wheel-shape assertions, run
*after* a wheel has been built (consumes the wheel artifact from
cibuildwheel/maturin):

```text
tests/distribution/
├── __init__.py
├── conftest.py                fixture that locates the wheel under
                               $WHEELHOUSE or builds one on the fly
├── test_toolr_wheel.py        asserts the `toolr` (binary) wheel contains
                               <wheel>.data/scripts/toolr, no Python source,
                               and a py3-none-<platform> tag.
└── test_toolr_py_wheel.py     asserts the `toolr-py` wheel contains
                               toolr/utils/_rust_utils.<abi>.so, plus the
                               expected Python source files (positive list),
                               and no __main__.py / _parser.py / _registry.py
                               (negative list — catches accidental un-prune).
```

Mechanism: `zipfile.ZipFile(wheel_path).namelist()` against a frozen
expected/forbidden list per wheel. Fast (<1s), no install required,
runs as part of `_build.yml` immediately after maturin produces the
wheel.

These tests are the lock that catches Plan 9's class of bug ("the wheel
claimed to ship X but didn't") and prevents accidental re-shipping of
the deleted Python CLI modules.

### Cross-wheel integration smoke

A single `tests/distribution/test_cross_wheel.py` that:

1. Creates a fresh venv via `tempfile`.
2. `pip install`s the locally-built `toolr-py` wheel into it.
3. Spawns the locally-built `toolr` binary (from
   `target/release/toolr`) as a subprocess against a tiny fixture
   `tools/` tree.
4. Asserts the subprocess output looks correct.

Marked `@pytest.mark.distribution`; skippable locally for fast iteration.

### Updates to `_test.yml`

1. Build step: `cargo build --release` (workspace) replaces today's
   single-package build. All three crates compile; binary lands at
   `target/release/toolr`.
2. Tarpaulin invocation: `cargo tarpaulin --workspace --tests
   --skip-clean ...` — `--workspace` picks up all three crates.

Matrix and structure unchanged.

### Updates to `install-smoke.yml`

`smoke-pip-wheel` job adds the `toolr-py` assertion:

```bash
python -c "
import toolr
from toolr.utils import _rust_utils
print('pyo3 dynlib OK:', _rust_utils.__file__)
"
```

### What doesn't change

- `pytest-skip-markers`, `pytest-subtests`, `hypothesis`, `coverage`,
  `attrs` — same dev deps.
- Test data under `tests/support/` keeps its current shape;
  `tests/support/3rd-party-pkg/` stays a uv workspace member.
- CI matrix (Linux/macOS/Windows × Python 3.11–3.14) shape is
  independent of the split.

---

## Open questions for the implementation plan (not design-level)

These are mechanical questions answered by grepping or by trial during
implementation, not by design choices:

1. Concrete list of `#[cfg(feature = "python")]` annotations to remove
   and where their guarded code goes (Section 2 design says "all into
   `toolr-py`"; plan confirms by grepping).
2. Concrete list of CLI tests in `tests/` and which of the three
   buckets each falls into (Section 6 design says "three-way prune";
   plan confirms by grepping).
3. Whether `main.rs` is large enough to warrant being split into
   `cli/args.rs`, `cli/run.rs`, etc. (Section 2 says "we'll discover
   when moving").
4. Cibuildwheel's wheel-tag behavior for `bindings = "bin"` (whether
   the resulting wheel ends up `py3-none-<plat>` or `cpXY-cpXY-<plat>`),
   which affects whether we keep cibuildwheel's per-Python build loop
   or pin it to one Python (Section 5 mentions this; trial during plan
   execution decides).
5. Whether `_runner` module exists today in `python/toolr/` or lands as
   part of the rewrite (Plan 2 says it should; Section 4 keeps space
   for it either way).

---

## Acceptance criteria

The split is "done" when:

- `cargo build --workspace --release` succeeds and produces three
  crate outputs; `target/release/toolr` is the standalone binary.
- `maturin build -m crates/toolr/pyproject.toml --release` produces a
  wheel containing `<wheel>.data/scripts/toolr` and nothing else
  beyond dist-info.
- `maturin build -m crates/toolr-py/pyproject.toml --release` produces
  a wheel containing `toolr/utils/_rust_utils.<abi>.so` and the
  trimmed `crates/toolr-py/python/toolr/**` tree.
- `pip install <toolr.whl>` puts `toolr` on PATH; running it shows
  `--version` matching `0.20.0`.
- `pip install <toolr-py.whl>` enables `import toolr` and `import
  toolr.utils._rust_utils` in a fresh Python.
- `pytest tests/` passes after the three-way pruning; the deleted
  Python CLI tests are either migrated or removed.
- `cargo test --workspace` passes.
- `tests/distribution/test_toolr_wheel.py` and
  `tests/distribution/test_toolr_py_wheel.py` pass.
- `install.sh` from `dist/install.sh` against the next release archive
  installs and runs the binary.
- mise plugin (`dist/mise-plugin/`) installs the binary from the same
  release.
- `_build.yml`, `_build-binary-archive.yml`, `release.yml`, `ci.yml`,
  `install-smoke.yml` all pass under the split shape.

---

## References

- Brainstorming session: 2026-05-13.
- Branch: `design/rust-front-end`.
- Roadmap: `specs/rust-front-end/01-roadmap.md`.
- Plan 9 (distribution): `specs/rust-front-end/10-plan-9-distribution.md`
  — this design supersedes Plan 9's Task 1 ("maturin auto-ships
  `[[bin]]` in pyo3 wheels"), which is empirically false against
  maturin 1.8.4.

---

## Amendments learned during execution

This section captures clarifications and deviations from the original
design that emerged during implementation (Stages 1–11). The main
body of the spec above remains the canonical description of the
end-state shape; this section explains the nuances.

### `_decorators.py` preserves user-facing API from `_registry.py`

The Python frontend retirement (Stage 8) deleted `_parser.py` and
`_registry.py`. However, `_registry.py` contained two distinct
concerns:

- CLI-internal classes (`CommandRegistry`, argparse plumbing) — gone.
- **User-facing decorators** (`command_group`, `@command`,
  `CommandGroup`, `MANIFEST_SCHEMA_VERSION`,
  `_get_command_group_storage`) — these are part of the
  `toolr-py` public API consumed by user tool scripts (every
  `tools/*.py` does `from toolr import command_group, command`).

The surviving public-API portion now lives in a new module
`crates/toolr-py/python/toolr/_decorators.py`. The public surface
(`from toolr import command_group, command`) is unchanged.

### README and LICENSE need crate-local symlinks

Each per-crate `pyproject.toml` references the repo-root README and
LICENSE. The design spec originally specified `readme = "../../README.md"`
and `license = { file = "../../LICENSE" }` with path-traversal.

In practice, maturin's PEP 517 metadata backend (which `uv sync`
invokes via `prepare_metadata_for_build_editable`) rejects
path-traversal in those fields with `project.<field> must be a safe
relative path inside the project`. The fix is a crate-local
relative symlink:

```text
crates/toolr/README.md       -> ../../README.md
crates/toolr/LICENSE         -> ../../LICENSE
crates/toolr-py/README.md    -> ../../README.md
crates/toolr-py/LICENSE      -> ../../LICENSE
```

Each `pyproject.toml` then references `readme = "README.md"` and
`license = { file = "LICENSE" }` (crate-local), with the symlink
resolving to the repo-root copy. No content duplication.

### Two per-crate sdists, not one workspace-wide sdist

The design spec originally implied a single source tarball (built at
the workspace root) that both wheel builds would consume.

After Stage 7's review revealed that the root `pyproject.toml`
no longer has a `[build-system]` table, `uv build --sdist` at the
workspace root fails with `Multiple top-level packages discovered
in a flat-layout`. The fix is to build **two per-crate sdists**:

```bash
uv build --sdist --package toolr      # crates/toolr sdist
uv build --sdist --package toolr-py   # crates/toolr-py sdist
```

`_prepare-release.yml` exposes two outputs: `binary-release-tarball-name`
and `py-release-tarball-name`. `_build.yml` consumes one per call;
`CIBW_CONFIG_FILE` is no longer needed because each sdist has its own
`pyproject.toml` at root, which cibuildwheel discovers natively.

`release.yml`'s `publish-release` job downloads both sdists into `dist/`.

### `execute_build.rs` (clap→ExecutionSpec) lives in `crates/toolr/`

The design spec's Section 2 declared "no `clap`" in `toolr-core`. The
file `src/execute/build.rs` (now `crates/toolr/src/execute_build.rs`)
translates `clap::ArgMatches` into the `ExecutionSpec` runtime type.
Because it consumes a clap type, it cannot live in `toolr-core`
without re-introducing the clap dependency. The translator was
moved to `crates/toolr/src/execute_build.rs`. The pure
`ExecutionSpec` type and its runtime stay in `toolr-core`.

### `.cargo/config.toml` for macOS pyo3 cdylib link flags

Maturin sets `-undefined dynamic_lookup` itself when it drives the
build, allowing the `extension-module` cdylib to leave Python
symbols unresolved at link time (the dynamic linker resolves them
against the host CPython at import time).

Plain `cargo build` (outside maturin) does not set these flags.
The plan requires `cargo build --workspace --release` to succeed
without going through maturin, so the workspace ships a
`.cargo/config.toml`:

```toml
[target.aarch64-apple-darwin]
rustflags = ["-C", "link-arg=-undefined", "-C", "link-arg=dynamic_lookup"]

[target.x86_64-apple-darwin]
rustflags = ["-C", "link-arg=-undefined", "-C", "link-arg=dynamic_lookup"]
```

Linux and Windows are unaffected (their dynamic linkers handle
undefined extension-module symbols natively).
