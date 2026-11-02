# In-Tree Venv Cache Dir Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Dispatching a command against an in-tree venv writes `meta.json` to the out-of-repo cache slot, not to
`tools/meta.json` (F-18).

**Architecture:** `run_command` in `crates/toolr/src/dispatch.rs` resolves the sidecar directory with
`toolr_core::project::meta_cache_dir`, the helper `finalize_sync` already uses, instead of `venv.parent()`.

**Tech Stack:** Rust (edition 2024), `assert_cmd` integration tests.

**Spec:** [`specs/archive/2026/2026-10-31-in-tree-venv-cache-dir-design.md`](./2026-10-31-in-tree-venv-cache-dir-design.md)

**Stacks on:** `fix/venv-sync-pyproject` (F-04). Its `resync_if_needed`/`check_freshness` changes are already in this
branch; nothing here touches them.

## Global Constraints

- No runner spec change, so no `RUNNER_SCHEMA_VERSION`/`SCHEMA_VERSION` bump.
- Cache-located venvs keep their sidecar at `<cache>/toolr/<repo-key>/meta.json`; only the in-tree path moves.
- A failure to resolve or touch the sidecar stays a warning, never a dispatch error.
- Format touched files only (`rustfmt crates/toolr/src/dispatch.rs crates/toolr/tests/cli_smoke.rs`); no whole-crate
  `cargo fmt`.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`.

## Review Focus

1. **The exact reported shape:** an in-tree repo runs one command, and `tools/meta.json` must not exist afterwards.
   Task 1, `dispatch_in_tree_venv_keeps_meta_json_out_of_tools`.
2. **The sidecar lands where sync puts it**, so `toolr self cache list` shows the repo. Same test.
3. **The fixture really reaches the touch block.** `preflight_fixture` has no `uv.lock`, so `check_freshness` returns
   `Missing` and dispatch goes straight to the touch with no sync. `common::toolr()` sets `TOOLR_TRUST_ALL=1`. The new
   test must fail before the fix on the `tools/meta.json` assertion; if it passes, the fixture isn't reaching the
   touch.
4. **Cache-located venvs are unchanged.** `toolr_core::project::tests::meta_cache_dir_for_cache_venv_is_venv_parent`
   already pins that `meta_cache_dir` returns `venv_dir.parent()` for them.

---

### Task 1: Route the dispatch-time touch through `meta_cache_dir`

**Files:**

- Modify: `crates/toolr/src/dispatch.rs` (`run_command`, the `touch_or_backfill` block after the venv is resolved)
- Test: `crates/toolr/tests/cli_smoke.rs`

**Interfaces (consumed):**

- `toolr_core::project::meta_cache_dir(repo_root: &Path, resolved: &ResolvedVenv) -> anyhow::Result<PathBuf>`
- `toolr_core::cache::touch_or_backfill(cache_dir: &Path, repo_path: &Path, toolr_version: &str, python_version: &str)
  -> Result<(), MetaError>`
- [ ] **Step 1: Write the test** `dispatch_in_tree_venv_keeps_meta_json_out_of_tools` in `cli_smoke.rs`
  (`#[cfg(unix)]`, next to `dispatch_emits_clear_error_when_venv_python_is_missing`):
    - `let tmp = preflight_fixture(&[]);` then `preflight_cmd(tmp.path()).args(["ci", "hello"]).output()`. The fake
      python exits 1; ignore the exit code.
    - `assert!(!tmp.path().join("tools/meta.json").exists(), …)` with stderr in the message.
    - Collect `preflight_cache_home(tmp.path()).join("toolr")/*/meta.json`. Assert exactly one, and that its
      `repo_path` (parsed with `serde_json`) equals `tmp.path().canonicalize()`.
    - `preflight_cmd(tmp.path()).args(["self", "cache", "list"])` → stdout contains the canonical repo path.
- [ ] **Step 2: Run it and confirm it fails.**
      `cargo test -p toolr --test cli_smoke dispatch_in_tree_venv_keeps_meta_json_out_of_tools` → fails on the
      `tools/meta.json` assertion.
- [ ] **Step 3: Implement.** Replace the `if let Some(venv) = &venv_dir && let Some(cache_dir) = venv.parent()` block
      with one keyed on `resolved_venv`: `if let Some(resolved) = &resolved_venv`, resolve
      `meta_cache_dir(&repo_root, resolved)`, then call `touch_or_backfill` with `&resolved.python_version`. Either
      error prints `toolr: warning: failed to touch cache meta.json: {e}`. Rewrite the block's comment to say why
      dispatch shares `meta_cache_dir` with sync (an in-tree venv's parent is `tools/`).
- [ ] **Step 4: Run** the Step 2 command → PASS. Then `cargo test -p toolr --test cli_smoke` and
      `cargo test -p toolr-core project::` → PASS.
- [ ] **Step 5: Commit.** `fix(dispatch): keep an in-tree venv's meta.json out of tools/`

### Task 2: Release notes, verification, archive

- [ ] **Step 1:** Add the spec's `UNRELEASED.md` text verbatim. Commit
      `docs(release): note in-tree venvs no longer leave tools/meta.json`.
- [ ] **Step 2:** `mise run test`, polling the output every 30-60 s → green. `prek run --all-files` → green. No docs
      change, so no `mkdocs build`.
- [ ] **Step 3:**
      `git mv specs/2026-10-31-in-tree-venv-cache-dir-design.md specs/2026-10-31-in-tree-venv-cache-dir-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and the design's `Stacks on:` link, and set the design's status to implemented. Commit
      `docs(specs): archive in-tree venv cache dir design and plan`.
