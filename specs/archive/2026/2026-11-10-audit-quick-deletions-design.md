# Audit quick deletions: design

## Why

An audit found one real bug (`init` reserved by the bootstrap skip list but not by the CLI) and a
set of small dead or redundant pieces. Each change is independent, so each is its own commit and
can be rejected alone.

## Changes

- **R-01.** `bootstrap.rs` `BUILTINS` lists `init`, which stopped being a top-level command. A user
  group named `init` is legal (`RESERVED_GROUPS` is `self`, `project`) but skips the manifest
  build on a fresh clone, so it fails with "unrecognized subcommand". Drop `init` from the list;
  replace the unit test that pinned the wrong behaviour; add an integration test with a user group
  named `init` and no manifest. Release note.
- **R-10.** Remove the unreachable `path.len() < 2` branch in `dispatch`, the unused `_manifest`
  parameter of `dispatch_help_from_argv`, and the stale tarpaulin wording in the test-module docs.
- **R-07 (dead code only).** Remove the never-read `registered` set in `parser/build.rs`.
- **U-06 (shim only).** Remove the hidden `project deps` migration shim (removed in 0.22, still
  present at 0.35) and its smoke test and completion assertion. Release note. The remaining U-06
  items (deprecated `arg()` kwargs, `command_group()` method form) belong to another package.
- **X-09.** Drop the unused `log` dependency; trim tokio features to what the tests use.
- **P-05.** Drop the redundant `cargo-check` hook (clippy covers it) and the needless
  `needs: prepare-ci` on the `pre-commit` job.
- **P-08.** `persist-credentials: false` on the `docs` job checkout; drop `environment: release`
  from the pipeline-gate job.
- **R-14.** Narrow ten `pub fn`s with no external caller to `pub(crate)`, or delete if unused.
- **R-12.** Replace the five `cargo_bin` aliases with `common::toolr`, and leave the
  `set_var` in `execute_build.rs` as it was: the variable names are unique per test, so a lock
  gained nothing. The six `fixture()` helpers are left as they are.
- **U-15.** List `-V, --version` in `--help`.
- **X-11.** Dropped: Renovate already tracks and groups the ruff crates (PRs #573, #638, #717).
- **X-10 (profile).** Tried `lto = "fat"` and `codegen-units = 1` in `[profile.release]`, then
  reverted: release builds went from 1m42s to 2m40s for a 19% smaller binary. Never
  `panic = "abort"`: the profile is workspace-wide and would abort the Python process on a
  `toolr-py` panic.

## Non-goals

`panic = "abort"`, crate trimming (TLS providers, ICU, termimad), bundling parser parameters into
a struct, the other U-06 removals.
