# CLI Build Command Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `cli::build_command` delegates each built-in subtree to its own builder function, with no change to the
clap tree (F-31).

**Architecture:** Extract-function refactor inside `crates/toolr/src/cli.rs`. Equality is proved by diffing a
`{:#?}` dump of the tree and the `--help` output of every built-in path, before and after.

**Tech Stack:** Rust, clap 4.

**Spec:** [`specs/archive/2026/2026-10-31-cli-build-command-design.md`](./2026-10-31-cli-build-command-design.md)

## Global Constraints

- No behaviour change: subcommand order, args, help text, headings, `hide` flags and defaults stay identical.
- `build_command(manifest: &Manifest) -> Command` keeps its signature.
- Don't touch `build_user_command`.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`. `rustfmt crates/toolr/src/cli.rs`
  only, never `cargo fmt -p`.
- Baseline captures go in a scratch dir outside the repo (`$TMPDIR/cli-refactor/`).

## Review Focus

1. **Subcommand order.** clap lists subcommands in insertion order; user groups must still come before `project`.
   Covered by the dump diff in Task 1, Step 4.
2. **Help heading leaking.** `next_help_heading` is sticky; the root's reset to the default heading before the help
   arg must still happen inside `build_root_command`. Covered by the `toolr --help` diff.
3. **Hidden commands stay hidden and absent from completions.** `builtin_completions` tests plus the dump diff.
4. **A dispatcher group named `project` or `self`.** `user_group_collides` runs before the built-ins are added;
   keep that loop in `build_command`. Main had no test for it; the new
   `user_groups_precede_built_ins_and_reserved_names_are_skipped` test in `cli.rs` covers it.
5. **Comments that explain a neighbouring line** move with it, not to the new function's doc comment.

---

### Task 1: Extract the built-in subtree builders

**Files:**

- Modify: `crates/toolr/src/cli.rs` (`build_command`, lines 133-684 on main at `3a860f41`, and new functions
  above it)
- Test: existing `cli.rs` `mod tests`, `crates/toolr/src/builtin_completions.rs` tests, `crates/toolr/tests/*`

**Interfaces:**

- Produces (all private to `cli.rs`): `const OUTPUT_HEADING: &str`; `fn build_root_command() -> Command`;
  `fn build_project_subcommand() -> Command`; `fn build_project_init_subcommand() -> Command`;
  `fn build_project_venv_subcommand() -> Command`; `fn build_self_subcommand() -> Command`;
  `fn build_self_build_manifest_subcommand() -> Command`; `fn build_self_cache_subcommand() -> Command`;
  `fn build_self_completion_subcommand() -> Command`; `fn build_hidden_subcommands() -> [Command; 3]`.

- [ ] **Step 1: Capture the baseline.** Add a throwaway, uncommitted test to `cli.rs` `mod tests`:
      `dump_builtin_tree`, which writes `format!("{:#?}", cmd)` for a built `build_command(&manifest)` to the path
      in `$TOOLR_TREE_DUMP` (skip when unset). The manifest holds a user group `ci` and a colliding group `self`,
      so the dump also covers group order and the reserved-name skip. `toolr` has no lib target, so run
      `TOOLR_TREE_DUMP=$TMPDIR/cli-refactor/before.txt cargo test -p toolr --bin toolr dump_builtin_tree`. Then
      `cargo build -p toolr` and copy `target/debug/toolr` to `$TMPDIR/cli-refactor/toolr-before`.

- [ ] **Step 2: Extract.** Move each block of `build_command` into the function named in the Interfaces block,
      cutting and pasting without editing it. `build_command` ends up as `build_root_command()`, the user-group loop
      unchanged, then `.subcommand(build_project_subcommand())`, `.subcommand(build_self_subcommand())` and a loop
      over `build_hidden_subcommands()`.

- [ ] **Step 3: Run** `cargo test -p toolr` → PASS, every existing test unchanged.

- [ ] **Step 4: Prove equality.**
    - `TOOLR_TREE_DUMP=$TMPDIR/cli-refactor/after.txt cargo test -p toolr --bin toolr dump_builtin_tree`, then
      `diff before.txt after.txt` → empty.
    - `cargo build -p toolr`. For each of `--help`, `project --help`, `project init --help`, `project venv --help`,
      `project venv {path,shell,run,sync,lock,add,remove} --help`, `project manifest rebuild --help`,
      `project trust --help`, `project untrust --help`, `self --help`, `self build-manifest --help`,
      `self cache {list,prune} --help`, `self trusted --help`, `self completion {print,install} --help`, run the
      old and new binaries from an empty temp dir with `COLUMNS=100` and diff → empty.
    - `toolr __complete <tmp> ""`, `toolr __complete <tmp> project venv ""` and
      `toolr __complete <tmp> self completion print ""` from both binaries → identical.

- [ ] **Step 5:** Delete `dump_builtin_tree`. `rustfmt crates/toolr/src/cli.rs`. Commit
      `refactor(cli): build each built-in subtree in its own function`.

### Task 2: Verification and archive

- [ ] **Step 1:** `cargo xtask build-skill-refs --check` → clean. `toolr pre-commit regen-doc-snippets --check` →
      clean.
- [ ] **Step 2:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green (clippy included:
      no new `too_many_lines` allow needed).
- [ ] **Step 3:**
      `git mv specs/2026-10-31-cli-build-command-design.md specs/2026-10-31-cli-build-command-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive cli build command design and plan`.
