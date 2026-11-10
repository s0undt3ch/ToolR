# Contributing to ToolR

Thanks for considering a contribution. ToolR is a small project with a focused surface; bug
reports, doc fixes, and well-scoped feature PRs are all welcome.

## Repo layout

A Cargo workspace with four crates plus the Python source:

| Crate                | What it is                                                                                                              |
| -------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `crates/toolr-core/` | Pure-Rust library. Parser, manifest, freshness, argparse scanner, completion engine, cache. No `pyo3`.                  |
| `crates/toolr/`      | The binary. `clap` CLI, dispatch, subprocess control.                                                                   |
| `crates/toolr-py/`   | `pyo3` dynlib + the Python source at `crates/toolr-py/python/toolr/`. Ships as the `toolr-py` wheel.                    |
| `crates/xtask/`      | Workspace automation, run as `cargo xtask <task>`.                                                                      |

CI builds two PyPI wheels at the same workspace version:

- `toolr` — maturin `bindings = "bin"`. The Rust binary, no Python.
- `toolr-py` — maturin `bindings = "pyo3"`. The Python package plus the `_rust_utils` extension module.

A GitHub release archive of the standalone binary ships alongside.

`toolr-py` builds `toolr-core` without its default `full` feature, so it links only the `command` and
`docstrings` modules. A new `toolr-core` dependency must be `optional = true` and listed as `dep:<name>` in
`full`. To use another `toolr-core` module from `toolr-py`, ungate it in `lib.rs` and make its dependencies
non-optional, then add them to the allowlist in the `toolr-core-lean-deps` pre-commit hook.

## Dev setup

You need [mise](https://mise.jdx.dev/). Everything else (Rust, Python, `uv`, `prek`) installs from the repo's `mise.toml`:

```sh
curl https://mise.run | sh        # if you don't have mise yet
mise install                      # pinned tool versions
uv sync --all-extras --dev        # Python deps
prek install --install-hooks      # pre-commit hooks
```

Run `toolr project trust` once in your checkout. toolr won't run this repository's `tools/*.py` until
you do.

Integration tests start toolr through `common::toolr()` (`crates/toolr/tests/common/mod.rs`), which
turns the trust check off because every test drives a throwaway repository. Use it instead of
`Command::cargo_bin("toolr")`. Trust tests use `TrustEnv`, which keeps the check on.

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

The completion tests drive real shells. bash 4.4+, zsh and fish come from your system package manager.
The bash tests also find the newest mise-installed `conda:bash` (`mise use -g conda:bash@5.3`) or any bash listed in
`TOOLR_TEST_BASH` (a PATH-style list). With none, `a_bash_4_4_or_newer_was_found` fails;
set `TOOLR_SKIP_BASH_COMPLETION_TESTS=1` to accept skipping them (local runs only; CI always fails without a bash).
The PowerShell completion tests are `#[ignore]`d by default, because few dev machines have `pwsh`.
Run them on purpose, with a missing shell failing them:

```sh
mise exec powershell@7.6.9 -- cargo test -p toolr --test complete_powershell -- --ignored
```

`crates/toolr-py/python/toolr/_context.pyi` is hand-written, not generated. `tests/context/test_context_stub.py`
runs mypy's stubtest against it, so changing `Context` or another public name in `_context.py` needs a matching
stub edit.

### Reproducing a failure from its seed

The Python suite runs in random order with `pytest-randomly`. Every run prints its seed in the header
(`Using --randomly-seed=1234`), including in the CI log. To replay that order locally:

```sh
uv run pytest --randomly-seed=1234
```

`-p no:randomly` restores the default order. To replay the reverse of that order, which is a quick
way to find a test that leaks state:

```sh
uv run pytest --co -q -p no:randomly | grep :: | tail -r > ids.txt   # `tac` on Linux
uv run pytest -p no:cacheprovider -p no:randomly "@ids.txt"
```

Run it in the foreground: a backgrounded shell starts children with `SIGINT` ignored, which fails
`tests/e2e/test_signals.py`.

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
8. `SupportedType::path_completion()`: say how TAB completes it (any path, files, directories or
   executables), or `None` for a non-path type. clap's `ValueHint` comes from the same answer.

The compiler doesn't check the rest. Tests catch some of it, but not all:

1. `resolve_toolr_types_name` in `crates/toolr-core/src/parser/types/resolve.rs`: map
   `toolr.types.<Name>` to the variant.
2. `crates/toolr-py/python/toolr/types/__init__.py`: define the alias and add it to `__all__`.
3. `_dec_hook` in `crates/toolr-py/python/toolr/_runner.py`: if the Python value isn't a JSON type,
   convert the string the binary sends.
4. Add the name to the three pinned lists: `toolr_types_names_match_python_surface` in
   `parser/types/mod.rs`, `catalogue_covers_every_toolr_types_name` in `supported.rs`, and
   `EXPECTED_TOOLR_TYPES_NAMES` in `tests/types/test_types_module.py`.
5. Run `cargo xtask build-skill-refs` and commit the regenerated type tables in `docs/` and
   `skills/`.
6. Queue a release note with `toolr release-notes add`. A new type makes the next release a minor one.

## Commits

[Conventional Commits](https://www.conventionalcommits.org/). Examples:

- `feat(cli): add --quiet flag to project venv sync`
- `fix(parser): skip dot-prefixed dirs in list_python_files`
- `docs(internals): correct the third_party_hash File-shape bullet`

Repo policies:

- **Don't `--no-verify`** without a stated reason in the commit body. Pre-commit failures are signals, not obstacles.
- **Don't manually edit `CHANGELOG.md`** — `git-cliff` generates it on release from the conventional-commit history.

## Release notes

User-visible changes queue a note in `release-notes/`:

```sh
toolr release-notes add --text "…"            # new note, slug from the branch name
toolr release-notes add --append --text "…"   # add to the branch's existing note
toolr release-notes check --base <parent>     # the CI gate, run locally
```

CI fails a PR with a `feat`, `fix`, `revert` or breaking commit and no note. A `Release-Notes: skip`
trailer on any commit exempts the PR. The full rules are in [`release-notes/README.md`](https://github.com/s0undt3ch/ToolR/blob/main/release-notes/README.md).

## Recovering a half-published release

`release.yml` runs from `main` only; its first job fails on any other ref. The `publish-release` job
runs, in order: Test PyPI dress rehearsal, push the release commit and tag, attest, create the GitHub
release, publish to PyPI. This section mirrors `.github/workflows/release.yml` and
`.github/workflows/_prepare-release.yml`; change them together. Find where the run failed, then:

- **Before "Push the release commit and tag"** (build, tests, Test PyPI, or `main` moved): only the Test
  PyPI dress-rehearsal files are public. Fix the cause and dispatch the release again; uploads use
  `skip-existing`.
- **After the push** (attest, GitHub release or PyPI failed): `vX.Y.Z` is tagged, and re-running
  `publish-release` fails at the push step by design. Don't delete the tag. Check what exists
  (`gh release view vX.Y.Z`, the PyPI project page), then finish the missing parts. This works for 7 days
  after the failed run, the `retention-days: 7` of the notes, patch and tarball artifacts.
    1. Download the artifacts. `<binary-tarball>` and `<py-tarball>` are the `binary-release-tarball-name`
       and `py-release-tarball-name` outputs of the run page's `prepare-release` job; the notes file is
       always `release-notes.md`.

       ```sh
       run=<run-id>
       gh run download "$run" -p 'cibw-wheel-*' -D wheels && mkdir -p dist release-assets \
         && mv wheels/*/* dist/ && rmdir wheels/* wheels
       gh run download "$run" -n <binary-tarball> -D dist
       gh run download "$run" -n <py-tarball> -D dist
       gh run download "$run" -n release-notes.md -D .
       gh run download "$run" -p 'toolr-archive-*' -D archives \
         && mv archives/*/* release-assets/ && rmdir archives/* archives
       ```

    2. No GitHub release: run the same command as the `Create GitHub release` step in
       `release.yml`, with `RELEASE_VERSION=X.Y.Z`, `RELEASE_NOTES=release-notes.md` and `--prerelease`
       in place of `--latest` for an `-rc` version.
    3. Files missing on PyPI: Trusted Publishing only works from the workflow. Upload the missing files
       from `dist/` with
       `TWINE_USERNAME=__token__ TWINE_PASSWORD=<token> twine upload --skip-existing dist/<file>`,
       minting a project-scoped token at <https://pypi.org/manage/account/token/> and revoking it
       afterwards. This publishes without the PEP 740 attestations `pypa/gh-action-pypi-publish` adds.
    4. "Attest final release artifacts" needs the workflow's OIDC token and can't be redone by hand, so a
       recovered release ships without those attestations.
    5. `publish-packslip` runs after a failed `publish-release` and re-signs with `--clobber`. Once the
       release exists, "Re-run failed jobs" is fine, or re-run only `publish-packslip`.
- **Can't finish** (artifacts expired, PyPI rejected a file, or the missing attestations matter): leave
  that version and cut the next patch version. PyPI never accepts a deleted file name again.

## Pre-commit hooks

`prek install --install-hooks` (above) wires the gate. Manually:

```sh
prek run --all-files
prek run rumdl --files docs/internals/manifest.md
```

Hooks include `ruff`, `mypy`, `clippy`, `rumdl`, `typos`, `jactionlint`,
`shellcheck`, plus the project-local hooks, which run the `toolr pre-commit` commands under
`tools/precommit/` (for example the `regen-doc-snippets` hook).
`pin-github-actions` comes from `s0undt3ch/pre-commit-hooks`.

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
