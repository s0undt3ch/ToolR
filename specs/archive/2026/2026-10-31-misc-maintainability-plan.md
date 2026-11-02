# Misc Maintainability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Delete a test file that duplicates unit tests, and stop the parser calling the canonical `@group.command`
form "legacy" (F-43 items 1 and 3).

**Architecture:** A file deletion and a rename inside `crates/toolr-core/src/parser/`. Items 2 and 4 need no code; the
spec records why.

**Tech Stack:** Rust.

**Spec:** [`specs/archive/2026/2026-10-31-misc-maintainability-design.md`](./2026-10-31-misc-maintainability-design.md)

## Global Constraints

- No behaviour change. The parser's output for every fixture stays byte-identical.
- Don't touch `@group.command` handling in Python, the docs or the skills.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`. `rustfmt <file>` on touched files only.

## Review Focus

1. **Coverage kept.** The deleted tests' checks exist in `install.rs`: `host_asset_returns_a_known_triple_or_none` and
   `asset_url_points_at_latest_download`. Task 1, Step 1 confirms both still exist and pass.
2. **No stray "legacy" left for this form.** Task 2, Step 3 greps for it. Other uses of "legacy" in the crate (old
   manifest formats, the deprecated `command_group` method) stay.
3. **Renamed tests still run.** `cargo test` counts must match before and after Task 2.
4. **Concurrent branches.** Other open branches may edit `parser/commands.rs`. Keep the rename to identifiers and
   comments so a rebase conflict is mechanical.
5. **Manifest output unchanged.** Task 2 runs the parser and `plugin_parity` tests.

---

### Task 1: Delete `uv_install_offline.rs`

**Files:**

- Delete: `crates/toolr/tests/uv_install_offline.rs`

**Interfaces:** none.

- [x] **Step 1: Confirm the replacements.**
      `cargo test -p toolr-core --lib uv::install::tests::host_asset_returns_a_known_triple_or_none uv::install::tests::asset_url_points_at_latest_download`
      → both pass. `git grep -n uv_install_offline` → only the file itself.
- [x] **Step 2:** `git rm crates/toolr/tests/uv_install_offline.rs`.
- [x] **Step 3:** `cargo test -p toolr --no-run` → builds.
- [x] **Step 4: Commit.** `test(uv): drop offline install tests that duplicate unit tests`

### Task 2: Rename `LegacyVar` to `Binding`

**Files:**

- Modify: `crates/toolr-core/src/parser/commands.rs` (enum variant, `parse_legacy_command`, doc comments at the
  `extract_commands`, `CommandDecorator` and `parse_legacy_command` sites, five test names, panic messages)
- Modify: `crates/toolr-core/src/parser/groups.rs` (two comments)

**Interfaces:**

- Produces: `CommandDecorator::Binding { var: String, explicit_name: Option<String>, name_conflict: bool }` and
  `fn parse_binding_command(expr: &Expr) -> Option<CommandDecorator>`. Both are private to `commands.rs`.
- [x] **Step 1: Baseline.** `cargo test -p toolr-core --lib parser:: 2>&1 | tail -3` → note the pass count.
- [x] **Step 2: Rename** as the spec lists: variant, function, the five test names, panic messages and comments.
- [x] **Step 3: Check.**
    - `git grep -n -i 'legacy' crates/toolr-core/src/parser/commands.rs crates/toolr-core/src/parser/groups.rs` →
      no hit refers to the `@<var>.command` form.
    - `cargo test -p toolr-core --lib parser::` → same pass count as Step 1.
    - `cargo test -p toolr-core --test plugin_parity` → PASS.
    - `cargo clippy -p toolr-core --all-targets -- -D warnings` → clean.
- [x] **Step 4: Commit.** `refactor(parser): call @group.command the binding form, not legacy`

### Task 3: Verification and archive

- [x] **Step 1:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green.
- [x] **Step 2:** `git mv specs/2026-10-31-misc-maintainability-design.md
      specs/2026-10-31-misc-maintainability-plan.md specs/archive/2026/`; fix the plan's `Spec:` link and set the
      design's status to implemented. Commit `docs(specs): archive misc maintainability design and plan`.
