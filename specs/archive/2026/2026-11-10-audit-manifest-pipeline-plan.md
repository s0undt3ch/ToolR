# One manifest rebuild pipeline: plan

Design: [`2026-11-10-audit-manifest-pipeline-design.md`](2026-11-10-audit-manifest-pipeline-design.md).
The diff and coverage base is the quick-deletions branch.

## Tasks

1. **Characterisation tests.** Add tests for every path that isn't pinned yet, all passing on
   today's code:
    - `crates/toolr/tests/freshness_dispatch.rs`: fresh-clone build error (fatal, with and
      without a venv), untrusted fresh-clone build error (warns), stale-manifest cause line,
      `project manifest rebuild` error text, and a first-run manifest that stays fresh and
      untouched.
    - `crates/toolr-core/src/manifest_build/rebuild.rs`: the explicit rebuild waits for the
      rebuild lock.
2. **Delete `__build-static-manifest` (R-13).**
    - Add `common::build_manifest(root)`, which runs `project manifest rebuild`.
    - Repoint the fixture call sites in `complete_*.rs`, `end_to_end_sync.rs` and
      `clap_text_sanitised.rs`.
    - Drop the command-only rows in `cli_smoke.rs` and `root_quiet_builtins.rs`, the hidden list
      in `builtin_completions.rs` and the test list in `cli.rs`.
    - Remove the command from `cli.rs`, `dispatch.rs` and `docs/cli.md`.
    - Add a release note.
3. **Core pipeline (R-04).** In `manifest_build/rebuild.rs`:
    - Add `build`, `refresh`/`Refreshed`/`Refresh`, and `rebuild_manifest_full` returning
      `BuiltManifest`.
    - Add `BuiltManifest::warnings()` in `parser/build.rs`.
    - Change `BuildError::into_anyhow` to unwrap `Build`, and have `build_static_manifest` use it.
    - Move the carry-forward unit tests from `bootstrap.rs` into core.
4. **Callers.**
    - `bootstrap.rs`: `ensure_manifest` over `refresh`, delete `try_rebuild`,
      `carry_forward_cached_entries` and the bootstrap step.
    - `main.rs`: one call and one warning loop.
    - `project.rs`: use the new `rebuild_manifest_full`.
    - `complete/freshness.rs`: `refresh(root, None, false)`, and delete
      `preserve_non_static_entries`.
    - Update the characterisation expectations for the new error text, and add a release note.
5. **Verify.**
    - `prek run --all-files`
    - `cargo test -p toolr-core` and `cargo test -p toolr` (output to a file, poll it)
    - `cargo llvm-cov` plus `cargo xtask patch-coverage` against the base branch
6. **Line counts** into the design, then archive the spec.

## Tests

The existing freshness, concurrency, untrusted-repo, completion and plugin-warning suites cover
the rest. All of them must pass unchanged, except where a row only exercised the deleted command.
