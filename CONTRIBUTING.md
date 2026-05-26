# Contributing to ToolR

Thanks for considering a contribution. ToolR is a small project with a focused surface; bug
reports, doc fixes, and well-scoped feature PRs are all welcome.

## Repo layout

A Cargo workspace with three crates plus the Python source:

| Crate                | What it is                                                                                                              |
| -------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `crates/toolr-core/` | Pure-Rust library. Parser, manifest, freshness, argparse scanner, completion engine, cache. No `pyo3`.                  |
| `crates/toolr/`      | The binary. `clap` CLI, dispatch, subprocess control.                                                                   |
| `crates/toolr-py/`   | `pyo3` dynlib + the Python source at `crates/toolr-py/python/toolr/`. Ships as the `toolr-py` wheel.                    |

CI builds two PyPI wheels at the same workspace version:

- `toolr` — maturin `bindings = "bin"`. The Rust binary, no Python.
- `toolr-py` — maturin `bindings = "pyo3"`. The Python package plus the `_rust_utils` extension module.

A GitHub release archive of the standalone binary ships alongside.

## Dev setup

You need [mise](https://mise.jdx.dev/). Everything else (Rust, Python, `uv`, `prek`) installs from the repo's `mise.toml`:

```sh
curl https://mise.run | sh        # if you don't have mise yet
mise install                      # pinned tool versions
uv sync --all-extras --dev        # Python deps
prek install --install-hooks      # pre-commit hooks
```

Run the dev binary against the dogfood `tools/` directory:

```sh
cargo run -p toolr -- --help
cargo run -p toolr -- self build-manifest toolr_example_plugin
```

For the release-shaped binary (used by benchmarks and the install smoke tests):

```sh
cargo build -p toolr --release
./target/release/toolr --help
```

## Tests

| Suite                                  | Run with                          | Lives at                               |
| -------------------------------------- | --------------------------------- | -------------------------------------- |
| Rust unit tests                        | `cargo test -p toolr-core`        | `crates/toolr-core/src/**/*.rs`        |
| Rust integration tests                 | `cargo test -p toolr --test '*'`  | `crates/toolr/tests/*.rs` (`assert_cmd`) |
| Python unit tests                      | `uv run pytest`                   | `tests/**/*.py`                        |
| Distribution lock-tests (opt-in, slow) | `uv run pytest -m distribution`   | `tests/distribution/`                  |

The Rust integration tests spawn the built `toolr` binary via `assert_cmd`. Don't shadow them with
Python-level subprocess tests unless the behaviour can't be exercised in Rust.

## RUNNER_SCHEMA_VERSION ↔ SCHEMA_VERSION lock-step

The Rust binary and the `toolr-py` Python runtime communicate over a versioned JSON spec. Two constants must stay in lock-step:

- `RUNNER_SCHEMA_VERSION` in `crates/toolr-core/src/execute/spec.rs`
- `SCHEMA_VERSION` in `crates/toolr-py/python/toolr/_runner.py`

Both carry doc comments listing which changes require a bump and which don't. Read those before
changing either the Rust serde structs or the Python `RunnerSpec` class. A CI gate fails the build
when the two values disagree.

## Commits

[Conventional Commits](https://www.conventionalcommits.org/). Examples:

- `feat(cli): add --quiet flag to project deps sync`
- `fix(parser): skip dot-prefixed dirs in list_python_files`
- `docs(internals): correct the third_party_hash File-shape bullet`

Repo policies:

- **Don't `--no-verify`** without a stated reason in the commit body. Pre-commit failures are signals, not obstacles.
- **Don't manually edit `CHANGELOG.md`** — `git-cliff` generates it on release from the conventional-commit history.

## Pre-commit hooks

`prek install --install-hooks` (above) wires the gate. Manually:

```sh
prek run --all-files
prek run rumdl --files docs/internals/manifest.md
```

Hooks include `ruff`, `mypy`, `clippy`, `cargo check`, `rumdl`, `codespell`, `typos`, `actionlint`,
`shellcheck`, plus the project-local hooks (`pin-github-actions`, `regen-doc-snippets`).

## Benchmarking

`toolr bench compare` measures `<tool> -h` startup latency for every task-runner CLI it finds on
`$PATH` (`toolr`, `invoke`, `python-tools-scripts`, `duty`, `doit`, `nox`). Add `--install` to let
it `uv tool install` missing Python tools on demand, and `--markdown` to render the result as a
table:

```sh
toolr bench compare --install --markdown
```

The README's headline benchmark table comes from this command. Re-run it on a fresh hardware target
before changing the README's numbers.

## Filing bugs

Open a [GitHub issue](https://github.com/s0undt3ch/ToolR/issues/new) with:

- ToolR version (`toolr --version`)
- OS + shell
- Minimal `tools/*.py` (or repro repo URL) that triggers the bug
- Expected vs actual output

For suspected security issues, use
[GitHub Security Advisories](https://github.com/s0undt3ch/ToolR/security/advisories/new) instead of
a public issue.

## License

[Apache-2.0](https://github.com/s0undt3ch/ToolR/blob/main/LICENSE). No sign-off required.
