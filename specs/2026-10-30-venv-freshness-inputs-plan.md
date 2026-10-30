# Venv Freshness Inputs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A change to `tools/pyproject.toml` or `tools/.python-version` makes the tools venv stale, so dispatch and
`toolr project venv sync` run `uv sync` (F-04).

**Architecture:** `check_freshness` compares the sync stamp with the newest of `uv.lock`, `pyproject.toml` and an
optional `.python-version`. Every caller inherits the fix. Messages and docs name the three inputs.

**Tech Stack:** Rust (toolr-core, toolr), `assert_cmd`, the `VenvFixture` uv stub in `crates/toolr/tests/common/`.

**Spec:** [`specs/2026-10-30-venv-freshness-inputs-design.md`](./2026-10-30-venv-freshness-inputs-design.md)

## Global Constraints

- `tools/uv.lock` absent → `Missing`, as today. `pyproject.toml` or `.python-version` absent → ignored.
- Precedence stays `Missing`, then `Broken`, then `Stale` or `Fresh`.
- `venv sync` without `--force` stays freshness-gated (mise enter hook).
- Stale wording, used everywhere a `Stale` message is printed:
  `out of date with tools/pyproject.toml, uv.lock or .python-version`.
- No schema change. Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`. `rustfmt <file>` on
  touched files only.

## Review Focus

1. **The reported repro, through the binary.** Sync, edit `pyproject.toml`, `venv sync` → uv's argv log has `sync`.
   Task 1, `sync_runs_uv_after_pyproject_edit`.
2. **One sync is enough.** After the re-sync, a second `venv sync` must not run uv again (the stamp is written after
   uv may rewrite `uv.lock`). Task 1, `second_sync_after_pyproject_edit_is_a_no_op`.
3. **Dispatch re-syncs too,** not just `venv sync`. Task 1, `pyproject_edit_resyncs_before_dispatch`.
4. **No `.python-version`** is the common case and must stay `Fresh`. Task 1, `absent_python_version_is_ignored`.
5. **`venv run --no-sync`** must refuse a pyproject-stale venv with the new wording. Task 1,
   `no_sync_refuses_after_pyproject_edit`.

---

### Task 1: Freshness covers pyproject.toml and .python-version

**Files:**

- Modify: `crates/toolr-core/src/venv/sync.rs` (`check_freshness`, its doc comment, the `Freshness::Stale` and
  `FRESHNESS_MARKER` doc comments)
- Modify: `crates/toolr/src/dispatch.rs` (`resync_if_needed` message and doc comment)
- Modify: `crates/toolr/src/project.rs` (`venv run --no-sync` `Stale` message, ~588-600)
- Modify: `crates/toolr/src/cli.rs` (`venv sync` about text and `--force` help, ~355-361)
- Test: `sync.rs` `mod tests`; `crates/toolr/tests/project_venv_sync.rs`;
  `crates/toolr/tests/dispatch_venv_freshness.rs`; `crates/toolr/tests/project_venv_run.rs`

**Interfaces:** none produced. `check_freshness(resolved: &ResolvedVenv, tools_dir: &Path) -> Freshness` keeps its
signature.

- [ ] **Step 1: Write the tests:**
    - `sync.rs` unit tests (reuse `dummy_resolved` and `venv_with_interpreter`; sleep 20 ms between writes, as the
      existing mtime tests do):
        - `pyproject_newer_than_marker_reports_stale`: lock, then marker, then `pyproject.toml` → `Stale`.
        - `python_version_newer_than_marker_reports_stale`: same with `.python-version`.
        - `absent_python_version_is_ignored`: lock and `pyproject.toml`, then marker, no `.python-version` → `Fresh`.
        - `missing_lock_still_reports_missing_with_pyproject_present`: `pyproject.toml` and marker, no lock →
          `Missing`.
    - `project_venv_sync.rs` (`#[cfg(unix)]`, `VenvFixture`):
        - `sync_runs_uv_after_pyproject_edit`: `venv sync` once; clear the argv log; sleep 20 ms; rewrite
          `tools/pyproject.toml` with an extra dependency; `venv sync` → argv log has a `sync` line.
        - `second_sync_after_pyproject_edit_is_a_no_op`: continue the above; clear the log; `venv sync` → no `sync`
          line.
    - `dispatch_venv_freshness.rs`: add `Project::bump_pyproject()` next to `bump_lock()`.
        - `pyproject_edit_resyncs_before_dispatch`: mirror `stale_venv_is_synced_before_dispatch` with
          `bump_pyproject()`.
        - Update `stale_venv_without_uv_refuses_to_run` to assert the new wording.
    - `project_venv_run.rs`: `no_sync_refuses_after_pyproject_edit`: synced fixture, edit `pyproject.toml`,
      `venv run --no-sync -- true` → fails, stderr contains the new wording.

- [ ] **Step 2: Run them and confirm the new ones fail.**
  `cargo test -p toolr-core --lib venv::sync` and
  `cargo test -p toolr --test project_venv_sync --test dispatch_venv_freshness --test project_venv_run`.

- [ ] **Step 3: Implement.** In `check_freshness`, after the `Broken` check, take the newest mtime of `uv.lock`,
  `pyproject.toml` (if present) and `.python-version` (if present); compare with the marker as now. Update the three
  messages to the Global Constraints wording and the `venv sync` help text to name the inputs.

- [ ] **Step 4: Run** the Step 2 commands → PASS.

- [ ] **Step 5: Commit.** `fix(venv): treat pyproject.toml and .python-version edits as a stale venv`

### Task 2: Docs, release notes, verification, archive

**Files:**

- Modify: `docs/project-config.md` (~113-120), `docs/installation/index.md` (~168-172), `docs/cli.md` (~90-97),
  `UNRELEASED.md`
- Regenerate: `docs/cli-files/project-venv-sync-help.txt` via `toolr pre-commit regen-doc-snippets`
- [ ] **Step 1:** Make the doc edits listed in the spec's "Scope and compatibility". Add the spec's `UNRELEASED.md`
  text verbatim.
- [ ] **Step 2:** `toolr pre-commit regen-doc-snippets` (run as `cargo run -p toolr -- pre-commit regen-doc-snippets`
  if the installed `toolr` is older) → the help snippet matches the new text. Commit
  `docs(venv): describe which tools/ files make the venv stale`.
- [ ] **Step 3:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` and
  `uv run mkdocs build --strict` → green.
- [ ] **Step 4:** `git mv specs/2026-10-30-venv-freshness-inputs-design.md
  specs/2026-10-30-venv-freshness-inputs-plan.md specs/archive/2026/`; fix the plan's `Spec:` link and set the
  design's status to implemented. Commit `docs(specs): archive venv freshness inputs design and plan`.
