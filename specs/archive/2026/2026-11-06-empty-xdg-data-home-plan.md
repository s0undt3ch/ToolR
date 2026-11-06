# Empty XDG variables count as unset: implementation plan

Design: [`2026-11-06-empty-xdg-data-home-design.md`](2026-11-06-empty-xdg-data-home-design.md). Work in the
`fix/empty-xdg-data-home` worktree.

## Task 1: shared helper (test first)

Files: `crates/toolr-core/src/env.rs` (new), `crates/toolr-core/src/lib.rs` (add
`#[cfg(feature = "full")] pub mod env;` in alphabetical position).

Signature: `pub fn non_empty_env_path(name: &str) -> Option<PathBuf>`, built from
`std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)`.

Unit tests in a `#[cfg(test)] mod tests` in `env.rs`, using `crate::test_env::EnvGuard::write()`:

- `unset_variable_is_none`: `remove("TOOLR_TEST_VAR")`, assert `None`.
- `empty_variable_is_none`: `set("TOOLR_TEST_VAR", "")`, assert `None`.
- `set_variable_is_its_path`: `set("TOOLR_TEST_VAR", "/some/dir")`, assert `Some(PathBuf::from("/some/dir"))`.

## Task 2: XDG directory functions

File: `crates/toolr-core/src/uv/mod.rs`. Replace the three `std::env::var_os("XDG_…").map(…)` heads with
`crate::env::non_empty_env_path("XDG_…_HOME").map(|v| v.join("toolr"))`. Update the `toolr_data_dir` doc
comment: "falling back to the platform data directory if `XDG_DATA_HOME` is unset or empty".

Tests next to `toolr_data_dir_uses_xdg_data_home_when_set`, each with `EnvGuard::write()`:

- `toolr_data_dir_treats_empty_xdg_data_home_as_unset`: set `XDG_DATA_HOME=""`; assert the result equals
  `dirs::data_dir().map(|d| d.join("toolr"))` and is absolute.
- `toolr_state_dir_treats_empty_xdg_state_home_as_unset`: set `XDG_STATE_HOME=""`; assert the result equals
  the result with the variable removed.
- `toolr_cache_dir_treats_empty_xdg_cache_home_as_unset`: as for data, against `dirs::cache_dir()`.
- `managed_uv_path_ignores_empty_xdg_data_home`: assert `managed_uv_path()` is absolute and ends with
  `toolr/bin/uv` (or `uv.exe`).

## Task 3: dispatch

File: `crates/toolr/src/dispatch.rs`. Delete the private `non_empty_env_path`; add
`use toolr_core::env::non_empty_env_path;`. Rewrite `dirs_home` to
`non_empty_env_path("HOME").ok_or_else(|| anyhow::anyhow!("$HOME is not set or empty; cannot pick install path"))`
and drop its "avoid a new crate dep" comment (still true, but no longer the reason it is hand-rolled).

Check `crates/toolr/src/dispatch.rs` unit tests for any that exercise `non_empty_env_path` or `dirs_home`
and move or adapt them; the behaviour is covered by Task 1.

## Task 4: integration tests

File: `crates/toolr/tests/completion_install_hint.rs` (reuse its fixture) or a new
`crates/toolr/tests/empty_xdg.rs` if the fixture does not fit.

- `empty_home_fails_completion_install`: `HOME=""`, `toolr self completion install bash`; assert exit
  non-zero and stderr contains `$HOME is not set or empty`.
- `empty_xdg_cache_home_uses_platform_default`: run `toolr self cache list` with `HOME=<tmp>`,
  `XDG_CACHE_HOME=""` and the cwd in another empty tempdir; assert stdout contains the `<tmp>` path (the
  platform default under `HOME`) and the cwd holds no `toolr` entry. Skip on Windows via `#[cfg(unix)]`,
  where `HOME` does not drive `dirs`.

## Task 5: release note

```sh
toolr release-notes add --text "An empty \`XDG_DATA_HOME\`, \`XDG_STATE_HOME\` or \`XDG_CACHE_HOME\` now falls back to the platform default, as the XDG spec says, instead of resolving to a \`toolr\` directory under the current directory. An empty \`HOME\` now makes \`toolr self completion install\` fail with \"\$HOME is not set or empty\"."
toolr release-notes check --base main
```

Commit type `fix(env): treat empty XDG_* variables as unset`.

## Verification

Rust and Python files change only trivially, but the umbrella is required by CLAUDE.md:

```sh
cargo test -p toolr-core env::
cargo test -p toolr-core uv::
cargo test -p toolr --test '*'
mise run test
```

Coverage gate: every changed line sits in `env.rs`, `uv/mod.rs` or `dispatch.rs`, each reached by the tests
above. Confirm with
`cargo llvm-cov --workspace --exclude toolr-py --tests --lcov --output-path target/coverage/lcov.info -- --test-threads=1`
then `cargo xtask patch-coverage --base main`.

No public Python surface changes, so `cargo xtask build-skill-refs --check` stays green. Archive both
spec files to `specs/archive/2026/` as the last commit before the PR.
