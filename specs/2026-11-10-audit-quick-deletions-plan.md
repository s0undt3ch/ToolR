# Audit quick deletions: plan

One commit per finding, in this order. Tests run through `cargo test -p toolr` (and
`-p toolr-core` for the parser and visibility changes); Python is untouched.

1. R-01: `bootstrap.rs`, `tests/user_group_named_init.rs`, release note.
2. R-10 and R-07: `dispatch.rs`, `main.rs`, `parser/build.rs`.
3. U-06 shim: `cli.rs`, `project.rs`, `builtin_completions.rs`, `tests/cli_smoke.rs`, note.
4. X-09: `Cargo.toml`, `crates/toolr-core/Cargo.toml`.
5. P-05, P-08: `.pre-commit-config.yaml`, `.github/workflows/ci.yml`, `release.yml`.
6. R-14: visibility changes in `toolr-core` and `toolr`.
7. R-12: test helpers in `crates/toolr/tests`, `execute_build.rs`.
8. U-15: `cli.rs`, help test, docs snippet regen if affected.
9. X-11, X-10: `Cargo.toml`, Renovate config.
10. Verify with `prek run --all-files`, cargo tests, patch coverage; archive these specs.
