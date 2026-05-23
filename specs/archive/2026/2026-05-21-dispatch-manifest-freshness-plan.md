# Dispatch Manifest Freshness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Detect stale `tools/.toolr-manifest.json` on every dispatch and rebuild in-process (pure Rust, no Python), persisting the result. Drop the legacy `toolr.commands` entry-point plugin mechanism in favor of statically shipped `toolr-manifest.json` files.

**Architecture:** Replace the `dynamic_hash` field (sorted dist-info hash) with `third_party_hash` (hash of the `site-packages/*/toolr-manifest.json` glob result + content). Introduce a shared `freshness::compare(cached, tools_dir, venv_dir) -> FreshnessVerdict` used by both tab completion and a new `bootstrap::ensure_manifest_fresh` step in `main.rs`. Soft-fail rebuild errors with a one-line warning naming the offending file; keep cached manifest in that case.

**Tech Stack:** Rust workspace (`toolr-core`, `toolr`, `toolr-py`), `thiserror`, `blake3`, `glob`, pytest for Python.

**Spec:** `specs/2026-05-21-dispatch-manifest-freshness-design.md`.

---

## Phase 1 — Foundation refactor (no dispatch behavior change)

### Task 1: Rename `Manifest.dynamic_hash` → `third_party_hash`

**Files (Modify):**

- `crates/toolr-core/src/manifest/model.rs:19`
- `crates/toolr-core/src/manifest/tests.rs:7,50`
- `crates/toolr-core/src/parser/build.rs:140`
- `crates/toolr-core/src/dynamic/merge.rs:16-17,76,137,139,149`
- `crates/toolr-core/src/dynamic/rebuild.rs:8,27,39,74,130`
- `crates/toolr-core/src/dynamic/hash.rs:3,18,83,84,93,94,102,122,123` (these are all function-name references; the sweep below renames them in one pass and Task 3 then swaps the implementation)
- `crates/toolr-core/src/complete/freshness.rs:66`
- `crates/toolr-core/src/complete/tests.rs:10,173,275,425`
- `crates/toolr-core/src/third_party/tests.rs:217`
- `crates/toolr-core/src/third_party/model.rs:4` (doc comment)
- `crates/toolr/src/main.rs:81`
- `crates/toolr/src/dispatch.rs:397,604,711`
- `crates/toolr/src/cli.rs:731,757,781,812`
- `crates/toolr/src/builtin_completions.rs:211`
- `crates/toolr/tests/cli_smoke.rs:30,106,206,234,236,332,442,670` (test fixtures and one test name)
- `crates/toolr/tests/dispatch_coverage.rs:48`
- `crates/toolr/tests/dynamic_e2e.rs:52,87`

**Steps:**

- [ ] **Step 1: Rename the model field**

In `crates/toolr-core/src/manifest/model.rs:14-22`, rename `dynamic_hash` to `third_party_hash`. Update the doc comment:

```rust
/// Top-level manifest document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    /// Hash over `tools/**/*.py` contents — used for fast freshness checks.
    pub static_hash: String,
    /// Hash over the sorted list of `site-packages/*/toolr-manifest.json`
    /// files (path + content) discovered in the tools venv. Empty when
    /// the venv has no third-party plugin manifests (or no venv at all).
    #[serde(default)]
    pub third_party_hash: String,
    pub groups: Vec<Group>,
    pub commands: Vec<Command>,
}
```

- [ ] **Step 2: Sweep all references workspace-wide**

Run the rename across every file enumerated above. The change is purely lexical (`dynamic_hash` → `third_party_hash`, including the function `compute_dynamic_hash` → `compute_third_party_hash`) except for the test function `execute_time_auto_rebuild_kicks_in_when_dynamic_hash_is_empty` in `cli_smoke.rs:206` — leave that test name alone for now; it gets deleted in Task 4.

Use:

```bash
grep -rln 'dynamic_hash' crates/ | xargs sed -i '' 's/dynamic_hash/third_party_hash/g'
```

Then revert the test name:

```bash
sed -i '' 's/execute_time_auto_rebuild_kicks_in_when_third_party_hash_is_empty/execute_time_auto_rebuild_kicks_in_when_dynamic_hash_is_empty/' crates/toolr/tests/cli_smoke.rs
```

- [ ] **Step 3: Compile check**

Run: `cargo check --workspace`
Expected: `Compiling …; Finished`. No errors.

- [ ] **Step 4: Run full test suite**

Run: `cargo test --workspace`
Expected: all green. (The `compute_dynamic_hash` function name is unchanged — only the field renamed. Tests still pass.)

- [ ] **Step 5: Commit**

```bash
git add -u
git commit -m "refactor(manifest): rename dynamic_hash to third_party_hash"
```

---

### Task 2: Add `Origin::ThirdParty` variant

The spec's "preserve cached third-party entries on `StaticDrift`" requires
distinguishing entries that originated from `tools/*.py` (regenerated on
every static rebuild) from entries that came from a glob-merged
`toolr-manifest.json` (only regenerated on third-party drift). The
current `Origin` enum is just `Static | Dynamic` — third-party entries
are stamped `Origin::Static` (`crates/toolr-core/src/third_party/merge.rs:76,94`).
Add a `ThirdParty` variant and stamp it where third-party fragments are merged.

**Files:**

- Modify: `crates/toolr-core/src/manifest/model.rs:228-233`
- Modify: `crates/toolr-core/src/third_party/merge.rs:76,94`
- Modify: `crates/toolr-core/src/third_party/tests.rs:256` (assertion update)
- Modify: any other call sites that pattern-match `Origin` exhaustively (likely zero with `_` arms in place — verify with grep)

**Steps:**

- [ ] **Step 1: Extend the enum**

In `crates/toolr-core/src/manifest/model.rs:228-233`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Entry parsed from `tools/*.py` by the static AST parser, or
    /// generated by the argparse dispatch grafting.
    Static,
    /// Entry produced by the Python introspection pass (`_introspect.py`).
    /// Not regenerable without spawning Python.
    Dynamic,
    /// Entry merged from a third-party package's `toolr-manifest.json`
    /// shipped under `<tools-venv>/lib/python*/site-packages/<pkg>/`.
    /// Regenerated on every glob-merge.
    ThirdParty,
}
```

- [ ] **Step 2: Stamp `Origin::ThirdParty` in the third-party merge**

In `crates/toolr-core/src/third_party/merge.rs:76,94`, change the two `origin: Origin::Static,` lines to `origin: Origin::ThirdParty,`.

- [ ] **Step 3: Update the third-party test assertion**

In `crates/toolr-core/src/third_party/tests.rs:256`, change `Origin::Static` to `Origin::ThirdParty`. The two test-fixture builders at lines 267 and 278 keep `Origin::Static` because those fixtures represent the project-side static parser's output (not the merge output).

- [ ] **Step 4: Find any exhaustive matches**

```bash
grep -rn 'match.*origin\|matches!.*Origin' crates/
```

For each hit, verify the match has an `_` arm or covers all three variants. Add a `Origin::ThirdParty => …` arm wherever needed. (Default expectation: most call sites use `matches!(o, Origin::Dynamic)` which is fine as-is.)

- [ ] **Step 5: Compile and test**

Run: `cargo test --workspace`
Expected: green.

- [ ] **Step 6: Commit**

```bash
git add -u
git commit -m "feat(manifest): add Origin::ThirdParty for glob-merged entries"
```

---

### Task 3: Swap `compute_third_party_hash` implementation

After Task 1 the function is named `compute_third_party_hash` but still
hashes `*.dist-info` directories. This task replaces the body so it
actually hashes the third-party manifest glob and updates the tests
accordingly. The function name and all call sites are already
correct; only the body and tests need to change.

**Files:**

- Modify: `crates/toolr-core/src/dynamic/hash.rs` (replace body + tests)
- Verify (no edits expected): `crates/toolr-core/src/dynamic/mod.rs`, `crates/toolr-core/src/dynamic/rebuild.rs`, `crates/toolr-core/src/dynamic/merge.rs` (Task 1's sweep already renamed identifiers and doc comments)

**Steps:**

- [ ] **Step 1: Write failing tests for the new third-party hash**

Replace the test module at the bottom of `crates/toolr-core/src/dynamic/hash.rs` with these scenarios. Use TempDir to build a fake venv layout `lib/python3.13/site-packages/<pkg>/toolr-manifest.json`.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Create a venv-shaped tree with the named packages, each shipping
    /// a `toolr-manifest.json` whose contents are `<pkg_name>-<content_tag>`.
    fn venv_with_manifests(entries: &[(&str, &str)]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let site = tmp.path().join("lib").join("python3.13").join("site-packages");
        for (pkg, content) in entries {
            let pkg_dir = site.join(pkg);
            fs::create_dir_all(&pkg_dir).unwrap();
            fs::write(pkg_dir.join("toolr-manifest.json"), content).unwrap();
        }
        tmp
    }

    #[test]
    fn empty_venv_returns_stable_empty_hash() {
        let tmp = TempDir::new().unwrap();
        let h = compute_third_party_hash(tmp.path()).unwrap();
        // Deterministic: re-running on the same empty layout must match.
        assert_eq!(h, compute_third_party_hash(tmp.path()).unwrap());
    }

    #[test]
    fn adding_a_manifest_changes_hash() {
        let a = venv_with_manifests(&[]);
        let b = venv_with_manifests(&[("foo", "{}")]);
        assert_ne!(
            compute_third_party_hash(a.path()).unwrap(),
            compute_third_party_hash(b.path()).unwrap()
        );
    }

    #[test]
    fn modifying_manifest_content_changes_hash() {
        let a = venv_with_manifests(&[("foo", r#"{"v":1}"#)]);
        let b = venv_with_manifests(&[("foo", r#"{"v":2}"#)]);
        assert_ne!(
            compute_third_party_hash(a.path()).unwrap(),
            compute_third_party_hash(b.path()).unwrap()
        );
    }

    #[test]
    fn removing_a_manifest_changes_hash() {
        let a = venv_with_manifests(&[("foo", "{}"), ("bar", "{}")]);
        let b = venv_with_manifests(&[("foo", "{}")]);
        assert_ne!(
            compute_third_party_hash(a.path()).unwrap(),
            compute_third_party_hash(b.path()).unwrap()
        );
    }

    #[test]
    fn unrelated_dist_info_does_not_change_hash() {
        // The win over the old dist-info-based hash: unrelated package
        // churn must not invalidate the manifest.
        let a = venv_with_manifests(&[("foo", "{}")]);
        let site = a.path().join("lib").join("python3.13").join("site-packages");
        // Manifest hash before adding the unrelated dist-info dir.
        let before = compute_third_party_hash(a.path()).unwrap();
        fs::create_dir(site.join("unrelated_pkg-1.0.0.dist-info")).unwrap();
        let after = compute_third_party_hash(a.path()).unwrap();
        assert_eq!(before, after);
    }
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p toolr-core --lib dynamic::hash::tests`
Expected: compile error — `compute_third_party_hash` undefined. Good.

- [ ] **Step 3: Implement `compute_third_party_hash`**

Rewrite `crates/toolr-core/src/dynamic/hash.rs` (keep the file path; replace the body):

```rust
//! Hash the set of third-party `toolr-manifest.json` files in the tools venv.
//!
//! Used as `Manifest.third_party_hash`. When this value differs from the
//! one stamped into the manifest, third-party plugin state has changed
//! (add, remove, or content modification) and the manifest must be
//! regenerated before the next command executes.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use blake3::Hasher;

use crate::third_party::glob::glob_manifests;

/// Compute a deterministic hash of the third-party plugin manifests
/// installed under `venv_root`.
///
/// The hash covers, in glob-sorted order, each
/// `site-packages/<pkg>/toolr-manifest.json` path together with the
/// blake3 of its contents. Adds, removes, or content edits all change
/// the hash. Unrelated `.dist-info` churn does not.
pub fn compute_third_party_hash(venv_root: &Path) -> Result<String> {
    // `glob_manifests` returns paths already sorted for determinism.
    let paths = glob_manifests(venv_root)
        .with_context(|| format!("globbing third-party manifests under {}", venv_root.display()))?;
    let mut hasher = Hasher::new();
    for path in &paths {
        let path_bytes = path.to_string_lossy();
        hasher.update(path_bytes.as_bytes());
        hasher.update(b"\0");
        let contents = fs::read(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let mut file_hasher = Hasher::new();
        file_hasher.update(&contents);
        hasher.update(file_hasher.finalize().as_bytes());
        hasher.update(b"\0");
    }
    Ok(hasher.finalize().to_hex().to_string())
}
```

- [ ] **Step 4: Quick verification pass on callers**

```bash
grep -n compute_third_party_hash crates/toolr-core/src/dynamic/mod.rs \
                                  crates/toolr-core/src/dynamic/rebuild.rs \
                                  crates/toolr-core/src/dynamic/merge.rs
```

Confirm Task 1's sweep already touched the re-export, the two
`merged.third_party_hash = compute_third_party_hash(venv_root)?` call
sites in `rebuild.rs`, and the doc comments. No edits expected.

- [ ] **Step 5: Run the new hash tests**

Run: `cargo test -p toolr-core --lib dynamic::hash::tests`
Expected: 5 passed.

- [ ] **Step 6: Run the rest of the workspace**

Run: `cargo test --workspace`
Expected: still green. The `dynamic_e2e.rs` test that asserts `m.third_party_hash` is non-empty should still pass since a fake venv with a `toolr-manifest.json` produces a non-empty hash. If `dynamic_e2e.rs:52-87` was set up with `.dist-info` dirs only (no `toolr-manifest.json`), update it to drop a `toolr-manifest.json` in one of the package dirs.

- [ ] **Step 7: Commit**

```bash
git add -u
git commit -m "feat(dynamic): hash third-party manifests instead of dist-info"
```

---

### Task 4: Remove execute-time dynamic-layer freshness

The execute-time `ensure_dynamic_layer_fresh` (`dispatch.rs:377-403`) exists only to spawn Python and pick up entry-point plugin changes. With entry points dropped (Task 5) and dispatch-time freshness covering third-party changes (Tasks 6–9), this code becomes dead weight.

**Files:**

- Modify: `crates/toolr/src/dispatch.rs:101` (remove call site)
- Modify: `crates/toolr/src/dispatch.rs:377-403` (delete function)
- Modify: `crates/toolr-core/src/dynamic/rebuild.rs:55-...` (delete `rebuild_dynamic_only`)
- Modify: `crates/toolr-core/src/dynamic/mod.rs:13` (remove `rebuild_dynamic_only` from re-exports)
- Modify: `crates/toolr/tests/cli_smoke.rs:206-...` (delete `execute_time_auto_rebuild_kicks_in_when_dynamic_hash_is_empty` test and supporting fixtures)

**Steps:**

- [ ] **Step 1: Delete the call site**

In `crates/toolr/src/dispatch.rs:101`, remove the line:

```rust
ensure_dynamic_layer_fresh(&repo_root, manifest)?;
```

Also remove any `use` statements that become unused as a result.

- [ ] **Step 2: Delete the function body**

Delete `fn ensure_dynamic_layer_fresh(...) -> anyhow::Result<()>` and its body in `crates/toolr/src/dispatch.rs:377-403`.

- [ ] **Step 3: Delete `rebuild_dynamic_only`**

In `crates/toolr-core/src/dynamic/rebuild.rs`, find `pub fn rebuild_dynamic_only(...)` (line 55) and delete the function. Keep `rebuild_manifest_full` — it's still used by `bootstrap.rs` and `project::manifest::rebuild`.

- [ ] **Step 4: Update the re-export**

In `crates/toolr-core/src/dynamic/mod.rs:13`:

```rust
pub use rebuild::{RebuildOutcome, rebuild_manifest_full};
```

(Drop `rebuild_dynamic_only`.)

- [ ] **Step 5: Delete the execute-time auto-rebuild integration test**

In `crates/toolr/tests/cli_smoke.rs:206-...`, delete the entire `execute_time_auto_rebuild_kicks_in_when_dynamic_hash_is_empty` test function and any helpers used only by it.

- [ ] **Step 6: Compile and test**

Run: `cargo check --workspace && cargo test --workspace`
Expected: green.

- [ ] **Step 7: Commit**

```bash
git add -u
git commit -m "refactor(dispatch): remove execute-time dynamic-layer freshness"
```

---

### Task 5: Drop entry-point plugin loading in toolr-py

**Files:**

- Modify: `crates/toolr-py/python/toolr/_introspect.py` (delete `_load_entry_points`, remove its call, remove related rationale comments)
- Modify: `crates/toolr-py/python/toolr/testing.py` (remove `skip_loading_entry_points`, `entry_points_patcher`, `_default_entry_points_patcher`, and the `_load_entry_points` invocation)
- Modify: any toolr-py tests that exercise entry-point loading

**Steps:**

- [ ] **Step 1: Enumerate test files that exercise entry points**

Run:

```bash
grep -rln 'entry_point\|skip_loading_entry_points\|toolr.commands' crates/toolr-py/
```

List the files. Each one needs to be either deleted (if the test is entirely about entry-point behavior) or updated (if the test uses the testing harness and happens to mention the flag).

- [ ] **Step 2: Remove `_load_entry_points` from `_introspect.py`**

Open `crates/toolr-py/python/toolr/_introspect.py`. Delete:

- The entire `_load_entry_points` function (current lines 127-148).
- The `_load_entry_points(warnings)` call in `build_payload` (current line 154).
- The "Task 4's entry-point pass for legacy third-party packages" comment (current lines 117-120 region — keep the surrounding code, just trim the obsolete rationale).
- Update the module docstring (lines 4-6) to drop the "enumerates `importlib.metadata` entry points" phrasing.
- [ ] **Step 3: Update `testing.py`**

In `crates/toolr-py/python/toolr/testing.py`:

- Remove the `skip_loading_entry_points: bool = field(default=False, repr=False)` field.
- Remove `entry_points_patcher: _patch = field(init=False, repr=False)`.
- Remove the `_default_entry_points_patcher` method.
- Remove any references to starting/stopping `entry_points_patcher` in setup/teardown.
- Remove the `_load_entry_points` import and call inside the helper that builds payloads.
- [ ] **Step 4: Update or delete affected tests**

For each file from Step 1: if its purpose is entry-point behavior, delete the file. Otherwise drop the `skip_loading_entry_points=True` constructor argument and any assertions about entry-point warnings.

- [ ] **Step 5: Run the Python test suite**

Run: `cd crates/toolr-py && uv run --active pytest`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add -u
git commit -m "feat(toolr-py)!: drop toolr.commands entry-point plugin support"
```

---

## Phase 2 — Shared freshness comparison

### Task 6: Create `freshness` module in toolr-core

**Files:**

- Create: `crates/toolr-core/src/freshness/mod.rs`
- Create: `crates/toolr-core/src/freshness/compare.rs`
- Create: `crates/toolr-core/src/freshness/tests.rs`
- Modify: `crates/toolr-core/src/lib.rs` (add `pub mod freshness;`)

**Steps:**

- [ ] **Step 1: Add the module declaration**

In `crates/toolr-core/src/lib.rs`, add `pub mod freshness;` in the alphabetical position (between `execute` and `hash`):

```rust
pub mod execute;
pub mod freshness;
pub mod hash;
```

- [ ] **Step 2: Create `freshness/mod.rs`**

```rust
//! Shared freshness comparison for both dispatch and tab completion.
//!
//! Both paths must answer the same question: "is the cached
//! `tools/.toolr-manifest.json` still good?" They differ only in what
//! they do with the answer (dispatch rebuilds + persists; tab
//! completion rebuilds in-memory only or, for third-party drift,
//! accepts a slightly stale completion result).
//!
//! Drift is reported on two axes — local-tools (`.py` content) and
//! third-party plugin manifests — and collapsed into a single
//! `FreshnessVerdict` whose variants are ordered by "stronger rebuild
//! needed."

mod compare;

#[cfg(test)]
mod tests;

pub use compare::{FreshnessVerdict, compare};
```

- [ ] **Step 3: Write failing tests in `freshness/tests.rs`**

```rust
use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::freshness::{FreshnessVerdict, compare};
use crate::manifest::Manifest;

fn make_tools(tmp: &Path, files: &[(&str, &str)]) {
    let tools = tmp.join("tools");
    fs::create_dir_all(&tools).unwrap();
    for (rel, content) in files {
        let path = tools.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }
}

fn make_venv(tmp: &Path, plugins: &[(&str, &str)]) {
    let site = tmp
        .join("venv")
        .join("lib")
        .join("python3.13")
        .join("site-packages");
    fs::create_dir_all(&site).unwrap();
    for (pkg, content) in plugins {
        let pkg_dir = site.join(pkg);
        fs::create_dir(&pkg_dir).unwrap();
        fs::write(pkg_dir.join("toolr-manifest.json"), content).unwrap();
    }
}

fn manifest_for(tmp: &Path) -> Manifest {
    use crate::hash::hash_tools_dir;
    use crate::dynamic::compute_third_party_hash;
    Manifest {
        schema_version: crate::manifest::SCHEMA_VERSION,
        static_hash: hash_tools_dir(&tmp.join("tools")).unwrap(),
        third_party_hash: compute_third_party_hash(&tmp.join("venv")).unwrap(),
        groups: vec![],
        commands: vec![],
    }
}

#[test]
fn returns_fresh_when_both_axes_match() {
    let tmp = TempDir::new().unwrap();
    make_tools(tmp.path(), &[("a.py", "x = 1\n")]);
    make_venv(tmp.path(), &[("foo", "{}")]);
    let cached = manifest_for(tmp.path());
    let venv = tmp.path().join("venv");
    let verdict = compare(Some(&cached), &tmp.path().join("tools"), Some(&venv)).unwrap();
    assert!(matches!(verdict, FreshnessVerdict::Fresh));
}

#[test]
fn returns_static_drift_when_py_file_changed() {
    let tmp = TempDir::new().unwrap();
    make_tools(tmp.path(), &[("a.py", "x = 1\n")]);
    make_venv(tmp.path(), &[("foo", "{}")]);
    let cached = manifest_for(tmp.path());
    fs::write(tmp.path().join("tools").join("a.py"), "x = 2\n").unwrap();
    let venv = tmp.path().join("venv");
    let verdict = compare(Some(&cached), &tmp.path().join("tools"), Some(&venv)).unwrap();
    assert!(matches!(verdict, FreshnessVerdict::StaticDrift));
}

#[test]
fn returns_third_party_drift_when_plugin_manifest_added() {
    let tmp = TempDir::new().unwrap();
    make_tools(tmp.path(), &[("a.py", "x = 1\n")]);
    make_venv(tmp.path(), &[]);
    let cached = manifest_for(tmp.path());
    let site = tmp.path().join("venv").join("lib").join("python3.13").join("site-packages");
    let pkg = site.join("foo");
    fs::create_dir(&pkg).unwrap();
    fs::write(pkg.join("toolr-manifest.json"), "{}").unwrap();
    let venv = tmp.path().join("venv");
    let verdict = compare(Some(&cached), &tmp.path().join("tools"), Some(&venv)).unwrap();
    assert!(matches!(verdict, FreshnessVerdict::ThirdPartyDrift));
}

#[test]
fn collapses_both_axes_to_third_party_drift() {
    let tmp = TempDir::new().unwrap();
    make_tools(tmp.path(), &[("a.py", "x = 1\n")]);
    make_venv(tmp.path(), &[("foo", "{}")]);
    let cached = manifest_for(tmp.path());
    fs::write(tmp.path().join("tools").join("a.py"), "x = 2\n").unwrap();
    let pkg = tmp.path().join("venv").join("lib").join("python3.13").join("site-packages").join("foo");
    fs::write(pkg.join("toolr-manifest.json"), r#"{"v":2}"#).unwrap();
    let venv = tmp.path().join("venv");
    let verdict = compare(Some(&cached), &tmp.path().join("tools"), Some(&venv)).unwrap();
    assert!(matches!(verdict, FreshnessVerdict::ThirdPartyDrift));
}

#[test]
fn unrelated_dist_info_returns_fresh() {
    let tmp = TempDir::new().unwrap();
    make_tools(tmp.path(), &[("a.py", "x = 1\n")]);
    make_venv(tmp.path(), &[("foo", "{}")]);
    let cached = manifest_for(tmp.path());
    let site = tmp.path().join("venv").join("lib").join("python3.13").join("site-packages");
    fs::create_dir(site.join("unrelated-1.0.0.dist-info")).unwrap();
    let venv = tmp.path().join("venv");
    let verdict = compare(Some(&cached), &tmp.path().join("tools"), Some(&venv)).unwrap();
    assert!(matches!(verdict, FreshnessVerdict::Fresh));
}

#[test]
fn no_cache_forces_rebuild_via_third_party_drift() {
    let tmp = TempDir::new().unwrap();
    make_tools(tmp.path(), &[("a.py", "x = 1\n")]);
    make_venv(tmp.path(), &[]);
    let venv = tmp.path().join("venv");
    let verdict = compare(None, &tmp.path().join("tools"), Some(&venv)).unwrap();
    // No cache means we have nothing to compare to; force the strongest
    // rebuild so the caller produces a fresh manifest including third-party.
    assert!(matches!(verdict, FreshnessVerdict::ThirdPartyDrift));
}

#[test]
fn missing_venv_treated_as_empty_third_party() {
    let tmp = TempDir::new().unwrap();
    make_tools(tmp.path(), &[("a.py", "x = 1\n")]);
    // Cached manifest claims an empty third-party set.
    let cached = Manifest {
        schema_version: crate::manifest::SCHEMA_VERSION,
        static_hash: crate::hash::hash_tools_dir(&tmp.path().join("tools")).unwrap(),
        third_party_hash: crate::dynamic::compute_third_party_hash(tmp.path()).unwrap(),
        groups: vec![],
        commands: vec![],
    };
    let verdict = compare(Some(&cached), &tmp.path().join("tools"), None).unwrap();
    assert!(matches!(verdict, FreshnessVerdict::Fresh));
}
```

- [ ] **Step 4: Implement `compare.rs` to make the tests pass**

```rust
//! Single-source-of-truth freshness comparison.

use std::path::Path;

use anyhow::{Context, Result};

use crate::dynamic::compute_third_party_hash;
use crate::hash::hash_tools_dir;
use crate::manifest::Manifest;

/// Outcome of comparing the live filesystem state to a cached manifest.
///
/// Variants are ordered by "stronger rebuild needed." When both axes
/// drift simultaneously, `compare` returns `ThirdPartyDrift` because the
/// third-party rebuild path is a superset that also re-parses
/// `tools/*.py`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessVerdict {
    /// Cached manifest matches the live state on both axes.
    Fresh,
    /// `tools/*.py` content has drifted; third-party manifests match.
    /// Call `build_static_manifest` and preserve cached third-party + dynamic entries.
    StaticDrift,
    /// At least one third-party `toolr-manifest.json` changed (and
    /// possibly the local tools too). Call
    /// `build_static_manifest_with_venv` and preserve cached dynamic
    /// entries; third-party entries come from the fresh glob.
    ThirdPartyDrift,
}

/// Compare cached manifest hashes against the live filesystem.
///
/// `venv_dir` is optional. When `None`, the third-party hash is computed
/// as if the venv were empty (deterministic empty-set hash). A `None`
/// `cached` always returns `ThirdPartyDrift` so the caller produces a
/// fresh manifest from scratch.
pub fn compare(
    cached: Option<&Manifest>,
    tools_dir: &Path,
    venv_dir: Option<&Path>,
) -> Result<FreshnessVerdict> {
    let Some(cached) = cached else {
        return Ok(FreshnessVerdict::ThirdPartyDrift);
    };

    let live_static = hash_tools_dir(tools_dir)
        .with_context(|| format!("hashing {}", tools_dir.display()))?;
    let live_third_party = match venv_dir {
        Some(v) => compute_third_party_hash(v)?,
        None => compute_third_party_hash(Path::new("/__nonexistent__"))
            .unwrap_or_else(|_| String::new()),
    };

    if cached.third_party_hash != live_third_party {
        return Ok(FreshnessVerdict::ThirdPartyDrift);
    }
    if cached.static_hash != live_static {
        return Ok(FreshnessVerdict::StaticDrift);
    }
    Ok(FreshnessVerdict::Fresh)
}
```

Note on the `None` venv branch: `glob_manifests` returns an empty `Vec` for a non-existent path (it just yields no matches), so `compute_third_party_hash` returns the empty-input hash deterministically. Using `Path::new("/__nonexistent__")` is a deliberate way to get that deterministic empty result; if the implementation of `glob_manifests` ever errors instead of returning empty, the `.unwrap_or_else` falls through to an empty string and the comparison degrades gracefully.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p toolr-core --lib freshness::tests`
Expected: 7 passed.

- [ ] **Step 6: Run the rest of the workspace**

Run: `cargo test --workspace`
Expected: still green.

- [ ] **Step 7: Commit**

```bash
git add -u
git commit -m "feat(toolr-core): add freshness::compare for shared dispatch/tab logic"
```

---

### Task 7: Refactor `complete::resolve_manifest_at_tab` to use `freshness::compare`

The existing implementation in `crates/toolr-core/src/complete/freshness.rs` hashes the tools dir inline and branches on equality with `cached.static_hash`. Switch it to call `freshness::compare`. Tab completion still does **not** persist and does **not** trigger a `_with_venv` rebuild on `ThirdPartyDrift` (cost guard — completion latency is sacred).

**Files:**

- Modify: `crates/toolr-core/src/complete/freshness.rs` (rewrite body of `resolve_manifest_at_tab`)
- Modify: `crates/toolr-core/src/complete/mod.rs` (update the doc comment in the module-level rustdoc that describes step 2)

**Steps:**

- [ ] **Step 1: Rewrite `resolve_manifest_at_tab`**

Replace the existing function with:

```rust
pub fn resolve_manifest_at_tab(cwd: &Path) -> Result<ResolvedManifest> {
    let project_root = discover_project_root(cwd)
        .with_context(|| format!("walking up from {} to find tools/", cwd.display()))?;
    let tools_dir = project_root.join("tools");
    let manifest_path = tools_dir.join(".toolr-manifest.json");
    let cached = load_manifest(&manifest_path).ok();

    // Tab completion never globs the venv — too costly on the hot path.
    // We feed `compare` `venv_dir = None`, which makes any third-party
    // drift indistinguishable from "no third-party plugins." That's
    // acceptable for completion: stale third-party suggestions on the
    // very next Tab press are fine; the next dispatch will rebuild.
    let verdict = crate::freshness::compare(cached.as_ref(), &tools_dir, None)?;

    if matches!(verdict, crate::freshness::FreshnessVerdict::Fresh) {
        if let Some(cached) = cached {
            return Ok(ResolvedManifest {
                manifest: cached,
                from_cache: true,
                project_root,
            });
        }
    }

    // StaticDrift (or ThirdPartyDrift, which tab completion handles like
    // StaticDrift): re-parse `tools/*.py` and preserve cached dynamic +
    // third-party entries.
    let mut fresh = build_static_manifest(&tools_dir)?;
    if let Some(cached) = cached {
        preserve_dynamic_and_third_party(&mut fresh, cached);
    }

    Ok(ResolvedManifest {
        manifest: fresh,
        from_cache: false,
        project_root,
    })
}

/// Carry forward `origin == Dynamic` and `origin == ThirdParty` entries
/// from the cache that don't collide with anything the fresh static
/// parser already produced.
fn preserve_dynamic_and_third_party(fresh: &mut Manifest, cached: Manifest) {
    for group in cached.groups {
        if !matches!(group.origin, Origin::Static)
            && !fresh.groups.iter().any(|g| g.name == group.name)
        {
            fresh.groups.push(group);
        }
    }
    for cmd in cached.commands {
        if !matches!(cmd.origin, Origin::Static)
            && !fresh
                .commands
                .iter()
                .any(|c| c.group == cmd.group && c.name == cmd.name)
        {
            fresh.commands.push(cmd);
        }
    }
    fresh.third_party_hash = cached.third_party_hash;
}
```

(Drop the unused `hash_tools_dir` import if it's no longer used directly in this file.)

- [ ] **Step 2: Update the module-level rustdoc**

In `crates/toolr-core/src/complete/mod.rs:7-13`, replace the description of step 2 with:

```rust
//! 2. [`resolve_manifest_at_tab`] — Tab-time freshness check that loads
//!    the cached manifest, delegates to [`crate::freshness::compare`],
//!    and either returns the cached manifest or a fresh one built by
//!    [`crate::parser::build_static_manifest`]. Tab completion never
//!    persists and never re-globs the tools venv.
```

- [ ] **Step 3: Run the tab-completion tests**

Run: `cargo test -p toolr-core --lib complete`
Expected: all green. The existing `complete::tests` cover the freshness paths via the public `resolve_manifest_at_tab` API and should be unaffected by the refactor.

- [ ] **Step 4: Run the full workspace**

Run: `cargo test --workspace`
Expected: green.

- [ ] **Step 5: Commit**

```bash
git add -u
git commit -m "refactor(complete): use freshness::compare in resolve_manifest_at_tab"
```

---

## Phase 3 — Wire dispatch-time freshness

### Task 8: Implement `ensure_manifest_fresh` in `bootstrap.rs`

**Files:**

- Modify: `crates/toolr/src/bootstrap.rs` (add new function + small helpers)

**Steps:**

- [ ] **Step 1: Add the function below `ensure_manifest_present_or_bootstrap`**

Append to `crates/toolr/src/bootstrap.rs`:

```rust
use std::path::PathBuf;

use toolr_core::freshness::{FreshnessVerdict, compare};
use toolr_core::hash::hash_tools_dir;
use toolr_core::manifest::{Manifest, Origin, load_manifest, save_manifest};
use toolr_core::parser::{build_static_manifest, build_static_manifest_with_venv, BuildError};

/// After the "manifest present" gate, verify the cached manifest is
/// still fresh against the live filesystem. Rebuild + persist when
/// stale; soft-fail with a warning when a rebuild errors.
pub(crate) fn ensure_manifest_fresh(
    cwd: &Path,
    argv: &[String],
) -> anyhow::Result<()> {
    let Ok(root) = discover_project_root(cwd) else {
        return Ok(());
    };
    let tools = root.join("tools");
    if !tools.join("pyproject.toml").is_file() {
        return Ok(());
    }
    if should_skip_auto_rebuild(argv) {
        return Ok(());
    }
    let manifest_path = tools.join(".toolr-manifest.json");
    let cached = load_manifest(&manifest_path).ok();

    // Resolve venv if available; freshness is venv-tolerant.
    let venv_dir: Option<PathBuf> = toolr_core::venv::resolve_venv_path(&root)
        .ok()
        .map(|r| r.venv_dir);

    let verdict = compare(cached.as_ref(), &tools, venv_dir.as_deref())?;

    match verdict {
        FreshnessVerdict::Fresh => Ok(()),
        v => match try_rebuild(v, &tools, venv_dir.as_deref(), cached.as_ref()) {
            Ok(fresh) => save_manifest(&manifest_path, &fresh)
                .with_context(|| format!("writing {}", manifest_path.display())),
            Err(e) => {
                warn_and_keep_cache(&e, cached.is_some());
                Ok(())
            }
        },
    }
}

fn try_rebuild(
    verdict: FreshnessVerdict,
    tools: &Path,
    venv: Option<&Path>,
    cached: Option<&Manifest>,
) -> Result<Manifest, BuildError> {
    let mut fresh = match verdict {
        FreshnessVerdict::StaticDrift => build_static_manifest(tools).map_err(BuildError::Build)?,
        FreshnessVerdict::ThirdPartyDrift => match venv {
            Some(v) => build_static_manifest_with_venv(tools, v)?,
            None => build_static_manifest(tools).map_err(BuildError::Build)?,
        },
        FreshnessVerdict::Fresh => unreachable!("Fresh handled by caller"),
    };
    if let Some(cached) = cached {
        preserve_origin_carryover(&mut fresh, cached, verdict);
    }
    // Stamp the fresh hashes.
    let live_static = hash_tools_dir(tools)
        .map_err(|e| BuildError::Build(anyhow::anyhow!("hashing tools: {e}")))?;
    fresh.static_hash = live_static;
    if matches!(verdict, FreshnessVerdict::StaticDrift) {
        // Static-only rebuild preserves cached third-party hash too,
        // because we didn't re-glob.
        if let Some(c) = cached {
            fresh.third_party_hash = c.third_party_hash.clone();
        }
    } else if let Some(v) = venv {
        fresh.third_party_hash = toolr_core::dynamic::compute_third_party_hash(v)
            .map_err(|e| BuildError::Build(anyhow::anyhow!("hashing third-party: {e}")))?;
    } else {
        fresh.third_party_hash = String::new();
    }
    Ok(fresh)
}

/// Copy non-static entries from `cached` into `fresh` when the fresh
/// rebuild has no entry with the same identity. On `StaticDrift` we
/// preserve both `Dynamic` and `ThirdParty` origins (we didn't re-glob).
/// On `ThirdPartyDrift` we only preserve `Dynamic` (third-party comes
/// from the fresh glob).
fn preserve_origin_carryover(fresh: &mut Manifest, cached: &Manifest, verdict: FreshnessVerdict) {
    let keep = |o: &Origin| match (verdict, o) {
        (FreshnessVerdict::StaticDrift, Origin::Dynamic | Origin::ThirdParty) => true,
        (FreshnessVerdict::ThirdPartyDrift, Origin::Dynamic) => true,
        _ => false,
    };
    for group in &cached.groups {
        if keep(&group.origin) && !fresh.groups.iter().any(|g| g.name == group.name) {
            fresh.groups.push(group.clone());
        }
    }
    for cmd in &cached.commands {
        if keep(&cmd.origin)
            && !fresh
                .commands
                .iter()
                .any(|c| c.group == cmd.group && c.name == cmd.name)
        {
            fresh.commands.push(cmd.clone());
        }
    }
}

fn warn_and_keep_cache(err: &BuildError, had_cache: bool) {
    eprintln!(
        "toolr: warning: tools manifest is stale and a fresh build failed; \
         falling back to cached manifest"
    );
    // BuildError::Display already truncates the cause into a single
    // line for `BuildError::Build`; for the list variants we use
    // `.to_string()` and take only the first line so dispatch isn't
    // drowned in output.
    let s = err.to_string();
    let first = s.lines().next().unwrap_or(&s);
    eprintln!("toolr: warning: cause: {first}");
    if !had_cache {
        eprintln!(
            "toolr: warning: no cached manifest available — `toolr <user-cmd>` \
             will likely fail until you fix the build error"
        );
    }
    eprintln!("toolr: warning: run `toolr project manifest rebuild` to see the full error");
}
```

Notes on imports / signatures:

- `save_manifest` already exists alongside `load_manifest` in `toolr_core::manifest`. If it doesn't, add a thin wrapper that serializes JSON and writes atomically (tmp file + rename).
- `Origin::ThirdParty` is an existing variant per `crates/toolr-core/src/manifest/model.rs` (the model defines `Static | Dynamic | ThirdParty`). Verify the variant names match before compiling; adjust the `match` arms if names differ.
- `BuildError` is re-exported from `crates/toolr-core/src/parser/build.rs:205`.
- [ ] **Step 2: Verify imports compile and existence of helpers**

Before moving on, confirm:

```bash
grep -n 'pub fn save_manifest' crates/toolr-core/src/manifest/io.rs crates/toolr-core/src/manifest/mod.rs 2>/dev/null
grep -n 'enum Origin' crates/toolr-core/src/manifest/model.rs
```

If `save_manifest` doesn't exist, add a small atomic-write helper alongside `load_manifest` (use `serde_json::to_writer_pretty` to a tempfile, then `rename`). Match the existing file layout (`manifest/io.rs` or `manifest/mod.rs` — wherever `load_manifest` lives).

- [ ] **Step 3: Compile**

Run: `cargo check -p toolr`
Expected: no errors.

- [ ] **Step 4: Run the bootstrap unit tests**

Run: `cargo test -p toolr --lib bootstrap`
Expected: existing tests pass. New behavior is covered by integration tests in later tasks.

- [ ] **Step 5: Commit**

```bash
git add -u
git commit -m "feat(toolr): add ensure_manifest_fresh bootstrap step"
```

---

### Task 9: Call `ensure_manifest_fresh` from `main.rs`

**Files:**

- Modify: `crates/toolr/src/main.rs:29-40`

**Steps:**

- [ ] **Step 1: Wire the call**

In `crates/toolr/src/main.rs`, change `run` from:

```rust
fn run() -> anyhow::Result<ExitCode> {
    let cwd = std::env::current_dir()?;
    let argv: Vec<String> = std::env::args().collect();
    maybe_emit_cache_hint_from_argv();
    bootstrap::ensure_manifest_present_or_bootstrap(&cwd, &argv)?;
    let manifest = load_or_empty(&cwd);
    let mut command = cli::build_command(&manifest);
    let matches = command.clone().get_matches();
    dispatch::dispatch(&matches, &manifest, &mut command)
}
```

to:

```rust
fn run() -> anyhow::Result<ExitCode> {
    let cwd = std::env::current_dir()?;
    let argv: Vec<String> = std::env::args().collect();
    maybe_emit_cache_hint_from_argv();
    bootstrap::ensure_manifest_present_or_bootstrap(&cwd, &argv)?;
    bootstrap::ensure_manifest_fresh(&cwd, &argv)?;
    let manifest = load_or_empty(&cwd);
    let mut command = cli::build_command(&manifest);
    let matches = command.clone().get_matches();
    dispatch::dispatch(&matches, &manifest, &mut command)
}
```

- [ ] **Step 2: Smoke-test manually**

```bash
cargo build -p toolr
# In a test project (or copy of dashtastic) — verify a newly added tools/example.py shows up:
cd /tmp/<scratch-project>
./target/debug/toolr --help | head -20
```

(Skip this step in CI; it's an interactive sanity check only.)

- [ ] **Step 3: Run the full workspace**

Run: `cargo test --workspace`
Expected: green. (Integration tests covering this wiring land in Tasks 9–12.)

- [ ] **Step 4: Commit**

```bash
git add -u
git commit -m "feat(toolr): refresh manifest before clap parses argv"
```

---

## Phase 4 — Integration tests

### Task 10: Dashtastic-style regression — added `tools/*.py` is detected on dispatch

**Files:**

- Create: `crates/toolr/tests/freshness_dispatch.rs`

**Steps:**

- [ ] **Step 1: Write the failing test**

Create `crates/toolr/tests/freshness_dispatch.rs`:

```rust
//! Integration tests for dispatch-time manifest freshness.

use std::fs;
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::TempDir;

const EXAMPLE_PY: &str = r#"
from toolr import Context, command_group

example = command_group("example", "Example commands")

@example.command
def hello(ctx: Context, name: str = "world") -> None:
    """Greet someone."""
    ctx.print(f"hello, {name}")
"#;

fn write_minimal_project(tmp: &std::path::Path) {
    let tools = tmp.join("tools");
    fs::create_dir_all(&tools).unwrap();
    fs::write(
        tools.join("pyproject.toml"),
        r#"
[project]
name = "tools"
version = "0.0.0"
"#,
    )
    .unwrap();
    // Seed an empty manifest so `ensure_manifest_present_or_bootstrap`
    // doesn't try to bootstrap via Python — we want to exercise the
    // freshness path, not the missing-manifest path.
    fs::write(
        tools.join(".toolr-manifest.json"),
        r#"{
            "schema_version": 1,
            "static_hash": "stale",
            "third_party_hash": "",
            "groups": [],
            "commands": []
        }"#,
    )
    .unwrap();
}

#[test]
fn new_tools_file_appears_in_help_without_explicit_rebuild() {
    let tmp = TempDir::new().unwrap();
    write_minimal_project(tmp.path());

    // Drop a new `example.py` in tools/ after the manifest was seeded.
    fs::write(tmp.path().join("tools").join("example.py"), EXAMPLE_PY).unwrap();

    let output = Command::cargo_bin("toolr")
        .unwrap()
        .arg("--help")
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "toolr --help failed: {output:?}");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("example"),
        "expected `example` in --help, got:\n{stdout}"
    );

    // Manifest on disk should have been rewritten to include the group.
    let manifest = fs::read_to_string(tmp.path().join("tools").join(".toolr-manifest.json")).unwrap();
    assert!(
        manifest.contains(r#""name": "example""#) || manifest.contains(r#""name":"example""#),
        "manifest was not persisted with the example group:\n{manifest}"
    );
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p toolr --test freshness_dispatch new_tools_file_appears_in_help_without_explicit_rebuild`
Expected: PASS. (The wiring from Task 9 should make this work end-to-end.)

If it fails because the project root discovery can't find the tools/ dir, double-check that the test sets cwd correctly and that `discover_project_root` walks up from cwd — write a small `dbg!` if needed.

- [ ] **Step 3: Commit**

```bash
git add -u
git commit -m "test(toolr): regression — added tools/*.py is detected on dispatch"
```

---

### Task 11: Soft-fail test — broken `tools/*.py` warns and falls back

**Files:**

- Modify: `crates/toolr/tests/freshness_dispatch.rs` (add test)

**Steps:**

- [ ] **Step 1: Add the test**

Append to `crates/toolr/tests/freshness_dispatch.rs`:

```rust
#[test]
fn syntax_error_in_tools_warns_and_serves_cached() {
    let tmp = TempDir::new().unwrap();
    write_minimal_project(tmp.path());

    // A pre-cached good manifest with one group so we can assert the
    // cached manifest is preserved.
    fs::write(
        tmp.path().join("tools").join(".toolr-manifest.json"),
        r#"{
            "schema_version": 1,
            "static_hash": "stale",
            "third_party_hash": "",
            "groups": [
                {"name": "good", "title": "Good", "description": "", "parent": null, "origin": "static"}
            ],
            "commands": []
        }"#,
    )
    .unwrap();

    // Now write a syntactically broken Python file so the static rebuild fails.
    fs::write(
        tmp.path().join("tools").join("broken.py"),
        "def not closed(",
    )
    .unwrap();

    let output = Command::cargo_bin("toolr")
        .unwrap()
        .arg("--help")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    // toolr --help itself must still succeed — we're soft-failing.
    assert!(output.status.success(), "toolr --help failed: {output:?}");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("tools manifest is stale and a fresh build failed"),
        "expected soft-fail warning in stderr; got:\n{stderr}"
    );
    assert!(
        stderr.contains("broken.py"),
        "expected the offending filename in the warning; got:\n{stderr}"
    );
    assert!(
        stderr.contains("toolr project manifest rebuild"),
        "expected pointer to explicit rebuild command; got:\n{stderr}"
    );

    // Cached `good` group must still be visible because we fell back.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("good"),
        "expected cached group in --help; got:\n{stdout}"
    );
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p toolr --test freshness_dispatch syntax_error_in_tools_warns_and_serves_cached`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add -u
git commit -m "test(toolr): syntax error in tools/ soft-fails with cached fallback"
```

---

### Task 12: Skip-list bypass — `__complete`, `self …`, `project …`, `init`, `--version` don't rebuild

**Files:**

- Modify: `crates/toolr/tests/freshness_dispatch.rs` (add test)

**Steps:**

- [ ] **Step 1: Add the test**

Append:

```rust
#[test]
fn skip_list_argv_does_not_trigger_freshness() {
    let tmp = TempDir::new().unwrap();
    write_minimal_project(tmp.path());

    // A poisonous tools/*.py would break a rebuild — but a skipped argv
    // must never call into freshness, so this should still succeed.
    fs::write(
        tmp.path().join("tools").join("broken.py"),
        "def not closed(",
    )
    .unwrap();

    for argv in [
        vec!["--version"],
        vec!["self", "cache", "list"],
        vec!["project", "manifest", "--help"],
    ] {
        let output = Command::cargo_bin("toolr")
            .unwrap()
            .args(&argv)
            .current_dir(tmp.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "argv {argv:?} should bypass freshness, got: {output:?}"
        );
        // The soft-fail warning must NOT appear; freshness was bypassed entirely.
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("tools manifest is stale"),
            "unexpected freshness warning for argv {argv:?}:\n{stderr}"
        );
    }
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p toolr --test freshness_dispatch skip_list_argv_does_not_trigger_freshness`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add -u
git commit -m "test(toolr): bypass argv (self/project/--version) skips freshness"
```

---

### Task 13: Tab-completion stays in-memory — no `.toolr-manifest.json` write

**Files:**

- Modify: `crates/toolr/tests/freshness_dispatch.rs` (add test)

**Steps:**

- [ ] **Step 1: Add the test**

Append:

```rust
#[test]
fn tab_completion_does_not_persist_manifest() {
    let tmp = TempDir::new().unwrap();
    write_minimal_project(tmp.path());
    // Force StaticDrift so we know the freshness code path would rebuild
    // if not bypassed.
    fs::write(tmp.path().join("tools").join("a.py"), "x = 1\n").unwrap();

    let manifest_path = tmp.path().join("tools").join(".toolr-manifest.json");
    let before = fs::read_to_string(&manifest_path).unwrap();
    let mtime_before = fs::metadata(&manifest_path).unwrap().modified().unwrap();

    // Trigger a completion call.
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .args(["__complete", tmp.path().to_str().unwrap(), "toolr", ""])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "__complete failed: {output:?}");

    // Manifest file must not have been rewritten.
    let after = fs::read_to_string(&manifest_path).unwrap();
    let mtime_after = fs::metadata(&manifest_path).unwrap().modified().unwrap();
    assert_eq!(before, after, "tab completion rewrote the manifest contents");
    assert_eq!(mtime_before, mtime_after, "tab completion touched mtime");
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p toolr --test freshness_dispatch tab_completion_does_not_persist_manifest`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add -u
git commit -m "test(toolr): tab completion never persists the manifest"
```

---

## Phase 5 — Documentation

### Task 14: UNRELEASED.md entry

**Files:**

- Modify: `UNRELEASED.md`

**Steps:**

- [ ] **Step 1: Append the breaking-change narrative**

Append to `UNRELEASED.md` (before any trailing whitespace; if the file is empty it's fine to just write the content):

````markdown

### Breaking — entry-point plugins removed

The `toolr.commands` entry-point mechanism for registering third-party
plugins is removed. Plugin authors must instead ship a static
`toolr-manifest.json` at the root of their installed Python package.
toolr's dispatch path is now pure Rust and never spawns Python just to
discover commands.

Migrating a plugin:

1. From inside the plugin's repo, run `toolr self build-manifest <pkg>`
   (replace `<pkg>` with the dotted package name). This writes a
   `toolr-manifest.json` next to your package's `__init__.py`.
2. Include the file in your built wheel. For hatchling, add this to
   `pyproject.toml`:

   ```toml
   [tool.hatch.build.targets.wheel]
   include = ["src/<pkg>/toolr-manifest.json"]
   ```

   For setuptools, add `include src/<pkg>/toolr-manifest.json` to
   `MANIFEST.in`.
3. Wire `toolr self build-manifest <pkg> --check` into CI and as a
   pre-commit hook. The `--check` flag exits non-zero when the
   committed `toolr-manifest.json` no longer matches what would be
   generated from current sources.
4. Delete the now-inert `[project.entry-points.'toolr.commands']`
   section from your plugin's `pyproject.toml`.

If you don't ship the file, your plugin's commands will not appear in
`toolr --help` or `toolr <group> --help`.

### Improved — dispatch detects stale manifests automatically

Adding, removing, or editing `tools/*.py` is now reflected on the very
next `toolr <user-cmd>` or `toolr --help` invocation — no
`toolr project manifest rebuild` needed. Installing or upgrading a
third-party plugin that ships its own `toolr-manifest.json` is
similarly picked up automatically. The check is pure Rust and adds
single-digit milliseconds on a warm cache. When a rebuild fails (for
example a syntax error in `tools/foo.py`), toolr serves the cached
manifest with a warning identifying the offending file rather than
blocking dispatch.

````

- [ ] **Step 2: Run docs linters**

```bash
cargo build -p toolr 2>/dev/null  # warm build cache
pre-commit run --files UNRELEASED.md
```

Expected: rumdl and other markdown checks pass (or auto-fix and re-stage).

- [ ] **Step 3: Commit**

```bash
git add UNRELEASED.md
git commit -m "docs(unreleased): note entry-point removal and dispatch freshness"
```

---

### Task 15: Plugin-author guide in docs/

**Files:**

- Modify: `docs/third-party.md` (or `docs/writing-commands/third-party-plugins.md` if the existing file is dedicated to user docs)

**Steps:**

- [ ] **Step 1: Check the existing docs layout**

```bash
cat docs/third-party.md
ls docs/writing-commands/
```

Decide: if `docs/third-party.md` is short and end-user-focused, add a new `docs/writing-commands/third-party-plugins.md` and link from `docs/third-party.md`. If `docs/third-party.md` is the plugin-author home, expand it in place.

- [ ] **Step 2: Write the plugin-author guide**

The document should contain these sections — adapt the path to whichever location Step 1 picked:

````markdown
# Authoring a third-party toolr plugin

Third-party packages add commands to a project's `toolr` CLI by
shipping a static `toolr-manifest.json` at the root of their installed
Python package. toolr's CLI is a Rust binary that boots in under 50 ms;
to keep that budget, command discovery globs these manifest files at
dispatch time and never spawns Python just to learn the shape of the
command tree.

## Generate the manifest

From inside your plugin's source tree, run:

```bash
toolr self build-manifest your_package
```

This walks the `command_group(...)` and `@group.command` decorators in
`your_package`, then writes `your_package/toolr-manifest.json` next to
the package's `__init__.py`. The file is plain JSON — commit it to
your repo.

## Ship it in your wheel

Make sure your build backend copies the file into the wheel.

Hatchling:

```toml
[tool.hatch.build.targets.wheel]
include = ["src/your_package/toolr-manifest.json"]
```

setuptools (`MANIFEST.in`):

```text
include src/your_package/toolr-manifest.json
```

Verify after `python -m build` that the wheel contains the file:

```bash
unzip -l dist/your_package-*.whl | grep toolr-manifest
```

## Keep it in sync

Add the `--check` flag to CI and pre-commit so the committed manifest
never drifts from the source code:

```bash
toolr self build-manifest your_package --check
```

Exits non-zero when the regenerated manifest differs from the file on
disk. Wire it as a pre-commit hook:

```yaml
- repo: local
  hooks:
    - id: toolr-manifest-check
      name: toolr-manifest-check
      entry: toolr self build-manifest your_package --check
      language: system
      pass_filenames: false
```

## How toolr finds your plugin

At dispatch time, toolr globs
`<tools-venv>/lib/python*/site-packages/*/toolr-manifest.json`, hashes
the result, and merges the discovered groups and commands into the
project manifest. If you uninstall, upgrade, or modify a plugin, the
hash changes and toolr rebuilds the project manifest on the very next
invocation. No manual step is required.

## Migration from entry-point plugins

In earlier toolr versions, plugins registered via
`[project.entry-points.'toolr.commands']` and toolr's CLI spawned
Python on every help / dispatch to enumerate them. That mechanism is
removed. To migrate:

1. Run `toolr self build-manifest your_package` once. Commit the
   resulting `toolr-manifest.json`.
2. Ensure your build backend includes the file in the wheel (see
   "Ship it" above).
3. Delete the `[project.entry-points.'toolr.commands']` section from
   `pyproject.toml`.
4. Publish a new release of your plugin.
````

- [ ] **Step 3: Cross-link from `docs/third-party.md`**

If you added a new file in Step 1, ensure `docs/third-party.md` (the user-side index) has a "Plugin authors → see `[Authoring third-party plugins](writing-commands/third-party-plugins.md)`" pointer near the top.

- [ ] **Step 4: Lint**

```bash
pre-commit run --files docs/third-party.md docs/writing-commands/third-party-plugins.md 2>/dev/null
```

- [ ] **Step 5: Commit**

```bash
git add docs/
git commit -m "docs: plugin-author guide for shipping toolr-manifest.json"
```

---

## Final verification

After the last task lands, run the full sweep:

```bash
cargo test --workspace
pre-commit run --all-files
```

Then verify the dashtastic-style scenario manually one more time:

```bash
cd /Users/pedro.algarvio/projects/paddle/dashtastic
ls tools/example.py             # make sure the example file is present
toolr --help | grep example     # should appear without `toolr project manifest rebuild`
```

If both spots show `example`, the end-to-end fix is complete.
