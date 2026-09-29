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

## Adding a supported type

A new `toolr.types` alias starts as a new `SupportedType` variant in
`crates/toolr-core/src/parser/types/supported.rs`. The compiler walks you through the Rust side:
each `non-exhaustive patterns: … not covered` error names the next `match` to extend.

1. Add the variant to `SupportedType`.
2. `SupportedType::doc()`: write its row for the "Supported types" table.
3. `SupportedType::kind()`: map it to a new kind, and add that kind to the
   `supported_type_kinds!` list at the position its table row should take.
4. `SupportedTypeKind::representative()`: return a value of the new variant.
5. Bump `SCHEMA_VERSION` in `crates/toolr-core/src/manifest/model.rs` and return the new value from
   the new kind's `SupportedTypeKind::since_schema()`. `since_schema_golden_tables` is expected to
   fail. Its new row is the new `SCHEMA_VERSION`, not whatever makes the test pass. Also set
   `toolr.MANIFEST_SCHEMA_VERSION` in `crates/toolr-py/python/toolr/_decorators.py` to the same
   value (the lockstep test fails otherwise), and re-run `cargo xtask build-skill-refs`.
6. `apply_value_parser` in `crates/toolr/src/value_parsers.rs`: choose the clap value parser.
7. `SupportedType::is_path()`: say whether clap stores the value as a `PathBuf`. For a path type,
   also add its `(PathForm, PathCheck)` to `path_rule` in `crates/toolr/src/value_parsers.rs`.
   A test fails if the two disagree.

The compiler doesn't check the rest. Tests catch some of it, but not all:

1. `resolve_toolr_types_name` in `crates/toolr-core/src/parser/types/resolve.rs`: map
   `toolr.types.<Name>` to the variant.
2. `crates/toolr-py/python/toolr/types/__init__.py`: define the alias and add it to `__all__`.
3. `_dec_hook` in `crates/toolr-py/python/toolr/_runner.py`: if the Python value isn't a JSON type,
   convert the string the binary sends.
4. Add the name to the three pinned lists: `toolr_types_names_match_python_surface` in
   `parser/types/mod.rs`, `catalogue_covers_every_toolr_types_name` in `supported.rs`, and
   `EXPECTED_TOOLR_TYPES_NAMES` in `tests/test_types_module.py`.
5. Run `cargo xtask build-skill-refs` and commit the regenerated type tables in `docs/` and
   `skills/`.
6. Queue an `UNRELEASED.md` entry. A new type makes the next release a minor one.

## Commits

[Conventional Commits](https://www.conventionalcommits.org/). Examples:

- `feat(cli): add --quiet flag to project venv sync`
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

Hooks include `ruff`, `mypy`, `clippy`, `cargo check`, `rumdl`, `typos`, `actionlint`,
`shellcheck`, plus the project-local hooks (`pin-github-actions`, `regen-doc-snippets`).

## Benchmarking

`scripts/bench.py` measures `<tool> -h` startup latency for every task-runner CLI it finds on
`$PATH` (`toolr`, `invoke`, `python-tools-scripts`, `duty`, `doit`, `nox`). It's a stdlib-only
script — no toolr, no rich, no venv to bootstrap — so it can run in a fresh CI job. Add
`--install` to let it `uv tool install` missing Python tools on demand:

```sh
python3 scripts/bench.py --install
```

Output is a markdown table on stdout (progress goes to stderr); the README's headline benchmark
table comes from this command. Re-run it on a fresh hardware target before changing the README's
numbers.

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
