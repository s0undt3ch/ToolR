# Remove editable-install Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Delete toolr's `[tool.toolr] editable-install` directive and its `uv pip install -e` machinery.
Editable installs are handled by uv natively via `[tool.uv.sources]`; the bespoke unlocked channel is the
SEC-04 vector and is removed rather than validated.

**Architecture:** Pure deletion — remove `venv/editable.rs`, the `ToolrConfig.editable_install` field, the
call site in `project.rs`, and the docs section. Existing configs that still carry the key keep parsing
(the field just no longer exists; `ToolrConfig` has no `deny_unknown_fields`, so the key is ignored).

**Tech Stack:** Rust (toolr-core), `cargo test`, docs.

**Design:** `specs/2026-06-10-remove-editable-install-design.md`. Read it first.

**Depends on / stacking:** branch `remove-editable-install`, stacked on `runner-path-hygiene` (SEC-02) →
`static-only-manifest` (SEC-01). SEC-01 reshaped `project.rs` (`finalize_sync`); this plan edits the
post-SEC-01 form.

**Conventions (from CLAUDE.md):** Conventional Commits; no `Co-Authored-By` footer; never `--no-verify`; run
`mise run test`; queue release notes in `UNRELEASED.md` (never edit `CHANGELOG.md`); markdown prose ≤120 cols.

---

## File map

**Delete:**

- `crates/toolr-core/src/venv/editable.rs`

**Modify:**

- `crates/toolr-core/src/venv/mod.rs` — drop `pub mod editable;` + the re-export.
- `crates/toolr-core/src/venv/config.rs` — remove `editable_install` field; update two tests.
- `crates/toolr-core/src/project.rs` — remove the editable-install call block + the two `use` names.
- `docs/project-config.md` — remove the `editable-install` section + step; point at `[tool.uv.sources]`.
- `UNRELEASED.md` — Removed note + migration.

**Leave untouched (unrelated "editable" references):** `venv/validate.rs` (maturin `.pth` editable detection),
`tools/pyproject.toml` (the commented `[tool.uv.sources]` example — the recommended pattern),
`crates/toolr/tests/cli_smoke.rs:73` (a comment about the test venv; confirm it doesn't exercise the directive).

---

## Task 1: Remove the call site in `project.rs`

**Files:**

- Modify: `crates/toolr-core/src/project.rs`

- [ ] **Step 1: Delete the editable-install block**

Remove lines ~66-72 in `ensure_venv_ready` (the `let outcomes = perform_editable_installs(...)` call and the
following `warn_failures(&outcomes);`). Keep the `finalize_sync` call and everything else.

- [ ] **Step 2: Drop the now-unused imports**

In the `use crate::venv::{...}` at lines ~12-15, remove `perform_editable_installs` and `warn_failures`. Keep
`ResolvedVenv, UpgradeMode, compute_repo_key, resolve_venv_path, sync::sync_if_needed, validate::validate_venv`.

- [ ] **Step 3: Do not build yet** — `mod.rs` still references the module; build after Task 2/3. (The deletions
      are interdependent, like SEC-01's Phase 1.)

- [ ] **Step 4: Commit (deferred build)**

```bash
git add crates/toolr-core/src/project.rs
git commit -m "refactor(venv): stop running post-sync editable installs"
```

---

## Task 2: Delete the `editable` module

**Files:**

- Delete: `crates/toolr-core/src/venv/editable.rs`
- Modify: `crates/toolr-core/src/venv/mod.rs`
- [ ] **Step 1: Remove the file**

```bash
git rm crates/toolr-core/src/venv/editable.rs
```

- [ ] **Step 2: Update `mod.rs`**

Remove `pub mod editable;` (line ~5) and the re-export line
`pub use editable::{EditableOutcome, perform_editable_installs, warn_failures};` (line ~14).

- [ ] **Step 3: Commit (deferred build)**

```bash
git add crates/toolr-core/src/venv/mod.rs
git commit -m "refactor(venv): delete the editable-install module"
```

---

## Task 3: Remove the `editable_install` config field

**Files:**

- Modify: `crates/toolr-core/src/venv/config.rs`

- [ ] **Step 1: Remove the field**

Delete the field and its doc comment from `ToolrConfig`:

```rust
    /// Opt-in editable installs run post-`uv sync`. E.g. `["."]`.
    #[serde(default)]
    pub editable_install: Vec<String>,
```

- [ ] **Step 2: Update the tests**
- In `defaults_when_table_is_absent`, delete the line `assert!(cfg.editable_install.is_empty());`.
- In `parses_in_tree_venv_location`, remove the `editable-install = ["."]` line from the TOML body **and** the
  assertion `assert_eq!(cfg.editable_install, vec![".".to_string()]);`.
- Add a new test proving a legacy config with the key still parses (the key is now ignored, not an error):

```rust
#[test]
fn legacy_editable_install_key_is_ignored_not_an_error() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    write_pyproject(
        &tools,
        "[project]\nname=\"x\"\nversion=\"0\"\n\n[tool.toolr]\neditable-install = [\".\", \"git+https://example/x\"]\n",
    );
    // Must not error — `ToolrConfig` has no deny_unknown_fields, so the
    // removed key is silently ignored.
    let cfg = load_toolr_config(&tools).unwrap();
    assert_eq!(cfg.venv_location, VenvLocation::Cache);
}
```

- [ ] **Step 3: Build the workspace**

Run: `cargo build --workspace 2>&1 | tail -20`
Expected: clean. Fix any remaining references the compiler flags (there should be none outside the files in
this plan).

- [ ] **Step 4: Test the crate**

Run: `cargo test -p toolr-core venv 2>&1 | tail -20`
Expected: PASS (config + project + venv tests green; the deleted editable tests are gone).

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-core/src/venv/config.rs
git commit -m "refactor(venv): drop the editable-install config field (legacy key ignored, not an error)"
```

---

## Task 4: Documentation

**Files:**

- Modify: `docs/project-config.md`

- [ ] **Step 1: Replace the section**

Delete the `### editable-install` section (around lines 68-76) and the numbered step "Apply each entry in
`editable-install` via `uv pip install -e`" (around line 120). Replace with a short note:

> Editable installs are configured the uv-native way, via `[tool.uv.sources]` in `tools/pyproject.toml`
> (declare the package as a dependency and point its source at a local path with `editable = true`). uv
> installs it as part of `uv sync`, recorded in `uv.lock`. toolr no longer performs separate editable
> installs.

Include the migration TOML snippet from the design.

- [ ] **Step 2: Regenerate doc snippets if any captured output changed**

Run: `prek run --all-files 2>&1 | tail -20` (covers the regen-doc-snippets + rumdl + mkdocs hooks).
If `regen-doc-snippets` rewrote a `docs/**/*.txt`, commit it too. Then:
Run: `uv run mkdocs build --strict 2>&1 | tail -5` (must succeed).

- [ ] **Step 3: Commit**

```bash
git add docs/
git commit -m "docs(config): document editable installs via [tool.uv.sources]; drop editable-install"
```

---

## Task 5: Release note

**Files:**

- Modify: `UNRELEASED.md`

- [ ] **Step 1: Add a Removed entry** (never edit `CHANGELOG.md`)

```markdown
### Removed

- The `[tool.toolr] editable-install` directive is removed. toolr no longer runs `uv pip install -e` itself;
  declare editable dependencies the uv-native way via `[tool.uv.sources]` (e.g.
  `foo = { path = "./packages/foo", editable = true }`), which `uv sync` installs and records in `uv.lock`.
  A `tools/pyproject.toml` that still lists `editable-install` keeps loading — the key is ignored.
```

- [ ] **Step 2: Commit**

```bash
git add UNRELEASED.md
git commit -m "docs(unreleased): note removal of editable-install (SEC-04)"
```

---

## Task 6: Full verification & audit status

- [ ] **Step 1: Umbrella suite**

Run: `mise run test` (poll long runs every 30–60s).
Expected: skill-ref gate, `cargo test --workspace`, and `pytest` all green.

- [ ] **Step 2: Confirm the channel is gone**

Run: `grep -rn 'perform_editable_installs\|editable_install\|EditableOutcome' crates/`
Expected: no matches.
Run: `grep -rn 'pip.*install.*-e' crates/`
Expected: no matches in shipped code (test fixtures that invoke uv directly, if any unrelated, are fine —
inspect each hit).

- [ ] **Step 3: Audit status (MAIN working tree, not this worktree — `audit/` is untracked there)**

Flip SEC-04 to `Done: remove-editable-install` in `audit/2026-06-10/README.md` and the SEC-04 finding file.
If you are a worktree sub-agent, skip this and report — the orchestrator will update the audit.

> Done when: `mise run test` is green; `editable.rs` and the config field are gone; a legacy
> `editable-install` key still parses (ignored); docs point at `[tool.uv.sources]`.

---

## Self-review notes (author)

- **Design coverage:** delete module (Task 2), field (Task 3), call site (Task 1), docs (Task 4), release note
  (Task 5), graceful-legacy-parse test (Task 3 Step 2). ✓
- **Build ordering:** Tasks 1–3 are interdependent; the build/test gate is Task 3 Steps 3–4, not after each.
  Flagged inline. ✓
- **No new code / no escape hatch:** removal only; no config key or env lever added (none would be
  trustworthy). ✓
- **Non-breaking parse:** relies on `ToolrConfig` lacking `deny_unknown_fields` — verified in
  `config.rs`. ✓

```text
