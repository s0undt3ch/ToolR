# toolr-py Dependencies Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `toolr-py` stops compiling and linking toolr-core's HTTP, archive, parser and async stacks (F-15).

**Architecture:** A default-on `full` feature on toolr-core gates every module except `command` and `docstrings`, and
the dependencies only those modules need. toolr-py depends on toolr-core with `default-features = false`. Two prek
hooks keep the lean build compiling and its dependency list short.

**Tech Stack:** Cargo features, prek.

**Spec:** [`specs/2026-10-31-toolr-py-deps-design.md`](./2026-10-31-toolr-py-deps-design.md)

## Global Constraints

- One feature, named `full`, in toolr-core's `default`.
- `toolr` and `xtask` keep toolr-core's default features. Their builds must not change.
- `tokio` is a dev-dependency only.
- No schema change, no `UNRELEASED.md` entry.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`. `rustfmt <file>` on touched files only.

## Review Focus

1. **The lean build compiles with tests.** Feature unification means `cargo test --workspace` never builds it. Task 1,
   `cargo clippy -p toolr-core --no-default-features --all-targets -- -D warnings`.
2. **The wheel still works.** `uv run pytest tests/context` drives `ctx.run` through the extension. Task 2.
3. **Windows.** `ThreadSafeHandle` and the Windows `command.rs` paths must build with `full` off; `winapi` must not
   move to dev-dependencies if `command.rs` uses it outside tests. Task 1, Step 4 (Windows check, or the Windows CI
   job on the PR).
4. **A new non-optional dependency** added to toolr-core later drags its tree back into toolr-py. Task 2's
   `toolr-py-deps` hook catches the named heavy crates.
5. **Dead code under `--no-default-features`.** Helpers in `command` or `docstrings` used only by gated modules would
   warn. Task 1's clippy run shows them; gate or move them, don't `#[allow]` them.

---

### Task 1: Feature-gate toolr-core

**Files:**

- Modify: `crates/toolr-core/Cargo.toml` (`[features]`, `optional = true`, `tokio` and `winapi` moves)
- Modify: `crates/toolr-core/src/lib.rs` (module and re-export gates)
- Modify: `crates/toolr-core/tests/manifest_schema_version_lockstep.rs`, `plugin_parity.rs`,
  `python_floor_lockstep.rs`, `schema_version_lockstep.rs`, `skill_authoring_examples.rs` (`#![cfg(feature = "full")]`)
- Modify: `crates/toolr-py/Cargo.toml` (`default-features = false`)
- Modify: `Cargo.lock` (via `cargo fetch`)

**Interfaces:**

- Produces: toolr-core feature `full` (default). Built without it, toolr-core exports only `CommandConfig`,
  `CommandExecutionError`, `CommandNoOutputTimeoutError`, `CommandTimeoutExceededError`, `run_command_internal`,
  `ThreadSafeHandle` (Windows), `Docstring`, `Example`, `ParseError`, `SimpleDocstringParser` and the `docstrings`
  module.

- [ ] **Step 1: Record the baseline.**
      `cargo tree -p toolr-py -e normal --prefix none | sort -u | wc -l` → 238 (± lockfile drift). Put the number in
      the commit body.

- [ ] **Step 2: See it fail.** Add `[features] default = ["full"]` and `full = []`, gate the `lib.rs` modules, and run
      `cargo clippy -p toolr-core --no-default-features --all-targets -- -D warnings` → the integration tests fail to
      compile (`toolr_core::execute` and friends not found).

- [ ] **Step 3: Implement.** Add `#![cfg(feature = "full")]` to the five integration tests. Make every dependency
      that `command.rs`, `docstrings.rs` and their tests don't need `optional = true` and list it as `dep:<name>` in
      `full`. Move `tokio` from `[dependencies]` to `[dev-dependencies]`. Gate `mod test_env` with
      `#[cfg(all(test, feature = "full"))]` if `command` and `docstrings` don't use it. Set
      `default-features = false` on toolr-py's toolr-core dependency. Run `cargo fetch`.

- [ ] **Step 4: Run.**
    - `cargo clippy -p toolr-core --no-default-features --all-targets -- -D warnings` → clean.
    - `cargo test -p toolr-core --no-default-features` → the `command` and `docstrings` tests pass.
    - `cargo clippy --workspace --all-targets --all-features -- -D warnings` → clean.
    - `cargo check -p toolr-core --no-default-features --tests --target x86_64-pc-windows-msvc` if the target is
      installed; otherwise rely on the Windows CI job. If `winapi` is used only by `command_test.rs`, move it to
      `[target.'cfg(windows)'.dev-dependencies]`.
    - This prints `0`:

      ```sh
      cargo tree -p toolr-py -e normal --prefix none \
        | grep -cE '^(tokio|reqwest|hyper|rustls|ruff_python_parser|ruff_python_ast|zip|tar) '
      ```

    - `cargo tree -p toolr-py -e normal --prefix none | sort -u | wc -l` → the new count, for the commit body.

- [ ] **Step 5: Commit.** `build(toolr-py): link only toolr-core's command and docstring modules`, with the before
      and after crate counts in the body.

### Task 2: Guard the lean build

**Files:**

- Modify: `.pre-commit-config.yaml` (two hooks in the local `repo` block, after `clippy`)

**Interfaces:**

- Consumes: the `full` feature from Task 1.

- [ ] **Step 1: Add the hooks.**
    - `id: cargo-check-toolr-core-lean`, `name: clippy (toolr-core without default features)`,
      `entry: cargo clippy -p toolr-core --no-default-features --all-targets -- -D warnings`, `language: system`,
      `pass_filenames: false`, `files: ^(crates/toolr-core/.*\.rs|Cargo\.toml|Cargo\.lock|crates/[^/]+/Cargo\.toml)$`.
    - `id: toolr-py-deps`, `name: toolr-py links no CLI-only crates`, `language: system`, `pass_filenames: false`, same
      `files:` as `cargo-lock`, with a comment naming F-15 and this entry:

      ```yaml
      entry: bash -c '! cargo tree -p toolr-py -e normal --prefix none | grep -E "^(tokio|reqwest|hyper|rustls|ruff_python_parser|ruff_python_ast|zip|tar) "'
      ```

- [ ] **Step 2: Prove the guard bites.** Temporarily drop `default-features = false` from toolr-py's dependency, run
      `prek run toolr-py-deps --all-files` → FAIL listing the crates. Revert, run again → PASS.

- [ ] **Step 3: Check the extension.** `uv sync --all-extras --dev --reinstall-package toolr-py`, then
      `uv run pytest tests/context tests/decorators` → green.

- [ ] **Step 4: Commit.** `ci(prek): keep toolr-py's toolr-core build lean`

### Task 3: Verification and archive

- [ ] **Step 1:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green.
- [ ] **Step 2:** `git mv specs/2026-10-31-toolr-py-deps-design.md specs/2026-10-31-toolr-py-deps-plan.md
      specs/archive/2026/`; fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive toolr-py deps design and plan`.
