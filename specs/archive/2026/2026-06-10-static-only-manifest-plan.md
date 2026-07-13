# Static-only manifest Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for
> tracking.

**Goal:** Make toolr build its command manifest exclusively by static analysis: a Rust AST parse of
`tools/*.py` plus an execution-free glob of installed third-party `toolr-manifest.json` fragments. No
repository Python runs until the user explicitly dispatches a command — and only ever through a
provenance-verified interpreter.

**Architecture:** Delete the dynamic introspection layer (`_introspect.py` +
`dynamic/{runner,payload,merge}.rs` + the Python-spawning bootstrap path). Bootstrap and freshness rebuilds
become pure-Rust static builds. Add interpreter-provenance verification at dispatch time, backed by the
existing out-of-repo `cache/meta.rs` sidecar. Treat `.toolr-manifest.json` as a `static_hash`-verified cache
that rebuilds when `tools/` or the venv changes.

**Tech Stack:** Rust (toolr-core, toolr), pyo3/Python (toolr-py), `cargo test`, `pytest`, `assert_cmd`
integration tests, `cargo xtask build-skill-refs`.

**Design:** `specs/2026-06-10-static-only-manifest-design.md`. Read it before starting.

**Conventions (from CLAUDE.md):**

- Conventional Commits (`feat(...)`, `fix(...)`, `refactor(...)`, `docs(...)`, `test(...)`).
- No `Co-Authored-By` footer.
- Queue release notes in `UNRELEASED.md`; never hand-edit `CHANGELOG.md`.
- Regenerate skill refs with `cargo xtask build-skill-refs` after public-surface changes; `--check` gates CI.
- Run the full umbrella `mise run test` for Rust/Python changes. Poll long `cargo test --workspace` runs every
  30–60s; don't fire-and-forget.
- Bump `RUNNER_SCHEMA_VERSION` and `SCHEMA_VERSION` together if the runner JSON spec changes (this plan should
  NOT need it — runner spec is untouched).

**Working location:** an isolated git worktree on branch `static-only-manifest` (created via the worktree
skill at execution time).

---

## File map

**Delete:**

- `crates/toolr-core/src/dynamic/runner.rs` — spawns `python -m toolr._introspect`.
- `crates/toolr-core/src/dynamic/payload.rs` — dynamic payload type + `PAYLOAD_SCHEMA_VERSION`.
- `crates/toolr-core/src/dynamic/merge.rs` — `merge_dynamic`.
- `crates/toolr-py/python/toolr/_introspect.py` — the introspection helper.
- `tests/introspect/` — its pytest suite (`__init__.py`, `test_introspect_empty.py`,
  `test_introspect_tools_walk.py`, `test_introspect_unit.py`).

**Modify:**

- `crates/toolr-core/src/dynamic/mod.rs` — drop `runner`/`payload`/`merge` modules +
  re-exports; keep `hash`, `rebuild`.
- `crates/toolr-core/src/dynamic/rebuild.rs` — `rebuild_manifest_full` becomes static-only (no `python` arg,
  no `run_introspect`, no `merge_dynamic`).
- `crates/toolr-core/src/manifest/model.rs:239-249` — remove `Origin::Dynamic`; keep serde lenient for legacy files.
- `crates/toolr-core/src/freshness/compare.rs` — doc comment mentions "dynamic entries"; update wording (logic
  already venv-driven).
- `crates/toolr/src/bootstrap.rs` — `ensure_manifest_present_or_bootstrap` builds statically (no venv/Python);
  simplify `should_skip_auto_rebuild`; `carry_forward_cached_entries` drops `Dynamic` handling.
- `crates/toolr/src/dispatch.rs:243-310` — add interpreter-provenance verification before `spawn_runner`.
- `crates/toolr-core/src/cache/meta.rs` — add interpreter-provenance fields; bump `SCHEMA_VERSION` to 2; lenient load.
- `crates/toolr-core/src/venv/sync.rs` (and/or `crates/toolr-core/src/project.rs`) — write provenance after a
  successful sync; rebuild the manifest as the final sync step.
- `crates/toolr-core/src/venv/mod.rs` — export the new provenance module.
- `skills/toolr-command-authoring/SKILL.md` + `docs/` — document the static-only contract.
- `UNRELEASED.md` — release notes.

**Create:**

- `crates/toolr-core/src/venv/provenance.rs` — interpreter trust check.
- `crates/toolr/tests/untrusted_repo.rs` — security regression integration tests.

---

## Phase 0 — Security regression tests (write the proof first)

These define "done." They must FAIL on the current code (which executes Python on `--help`) and PASS at the end.

### Task 0.1: Regression test — `--help` must not execute a committed in-tree interpreter

**Files:**

- Create: `crates/toolr/tests/untrusted_repo.rs`

- [ ] **Step 1: Write the failing test**

Use the built dev binary via `assert_cmd`. The fake interpreter writes a sentinel file if executed; the test
asserts the sentinel never appears.

```rust
//! Untrusted-repository regression tests for SEC-01.
//! A repo must not be able to run code via toolr's read-only surfaces.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use assert_cmd::Command;
use tempfile::TempDir;

/// Build a malicious repo: in-tree venv-location, a committed fake
/// `tools/.venv/bin/python` that drops a sentinel when executed, and NO
/// `.toolr-manifest.json`.
fn malicious_repo(sentinel: &std::path::Path) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir_all(tools.join(".venv").join("bin")).unwrap();
    fs::write(
        tools.join("pyproject.toml"),
        "[project]\nname=\"evil\"\nversion=\"0\"\n\n[tool.toolr]\nvenv-location = \"in-tree\"\n",
    )
    .unwrap();
    fs::write(tools.join("hello.py"), "\"\"\"Hi.\"\"\"\n").unwrap();
    let py = tools.join(".venv").join("bin").join("python");
    fs::write(&py, format!("#!/bin/sh\necho pwned > {}\n", sentinel.display())).unwrap();
    fs::set_permissions(&py, fs::Permissions::from_mode(0o755)).unwrap();
    tmp
}

#[test]
#[cfg(unix)]
fn help_does_not_execute_committed_interpreter() {
    let out = TempDir::new().unwrap();
    let sentinel = out.path().join("sentinel");
    let repo = malicious_repo(&sentinel);

    Command::cargo_bin("toolr")
        .unwrap()
        .arg("--help")
        .current_dir(repo.path())
        .assert()
        .success();

    assert!(!sentinel.exists(), "toolr --help executed the committed interpreter");
}

#[test]
#[cfg(unix)]
fn bare_invocation_does_not_execute_committed_interpreter() {
    let out = TempDir::new().unwrap();
    let sentinel = out.path().join("sentinel");
    let repo = malicious_repo(&sentinel);

    // Bare `toolr` exits non-zero (no command) but must not run the interpreter.
    let _ = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(repo.path())
        .assert();

    assert!(!sentinel.exists(), "bare toolr executed the committed interpreter");
}
```

- [ ] **Step 2: Run to verify it fails (RED)**

Run: `cargo test -p toolr --test untrusted_repo -- --nocapture`
Expected: `help_does_not_execute_committed_interpreter` FAILS — the sentinel exists, because today bootstrap
spawns `tools/.venv/bin/python`.

- [ ] **Step 3: Commit the red tests**

```bash
git add crates/toolr/tests/untrusted_repo.rs
git commit -m "test(security): failing regression for SEC-01 implicit code execution"
```

> Do not make these pass yet — they go green after Phase 1 (no Python on `--help`) and Phase 2 (interpreter
> provenance). Leave them failing; later tasks reference them.

---

## Phase 1 — Remove the dynamic execution layer

### Task 1.1: Bootstrap builds the manifest statically (no Python, no venv)

**Files:**

- Modify: `crates/toolr/src/bootstrap.rs`

- [ ] **Step 1: Rewrite `ensure_manifest_present_or_bootstrap` to build statically**

Replace the venv-resolve + `rebuild_manifest_full(&root, &resolved.python, &resolved.venv_dir)` body (current
`bootstrap.rs:44-54`) so it never resolves a venv interpreter and never spawns Python. Build first-party
statically, and third-party only if a venv already exists (execution-free glob).

```rust
// in ensure_manifest_present_or_bootstrap, replacing the resolve+rebuild block:
let manifest_path = tools.join(".toolr-manifest.json");
// First-party is always available (pure AST). Add third-party only when a
// venv already exists — globbing site-packages JSON executes nothing.
let venv_dir = resolve_venv_path(&root).ok().map(|r| r.venv_dir);
let manifest = match venv_dir.as_deref() {
    Some(v) if v.join("pyvenv.cfg").is_file() => build_static_manifest_with_venv(&tools, v)
        .map_err(anyhow::Error::from)?,
    _ => build_static_manifest(&tools)?,
};
write_manifest(&manifest_path, &manifest)
    .with_context(|| format!("writing {}", manifest_path.display()))?;
Ok(())
```

Remove the now-unused `use toolr_core::dynamic::rebuild_manifest_full;`. Keep
`build_static_manifest`/`build_static_manifest_with_venv`/`write_manifest`/`resolve_venv_path` imports.

- [ ] **Step 2: Simplify `should_skip_auto_rebuild`**

Static building is cheap and safe, so the help/version distinction is no longer about safety. Keep skipping
the built-ins that manage state themselves (`__complete`, `project`, `self`, `init`) but it is now fine for
`--help`/bare to take the static path. Update the doc comment to say the skip is a latency optimization, not a
safety gate. Update the two tests `fires_for_long_help_flag`/`fires_for_bare_toolr` to reflect intended
behavior: they may now skip OR run the static build — assert whichever the new code does, and add a comment
that neither path executes Python.

- [ ] **Step 3: `carry_forward_cached_entries` drops `Dynamic`**

In `carry_forward_cached_entries` (`bootstrap.rs:164-191`), change the `keep` closure so it no longer
references `Origin::Dynamic` (that variant is removed in Task 1.4). On `StaticDrift`, carry forward only
`Origin::ThirdParty`; on `ThirdPartyDrift`, carry forward nothing (third-party comes from the fresh glob).
This is the SEC-03 fix.

```rust
let keep = |o: &Origin| {
    matches!(
        (verdict, o),
        (FreshnessVerdict::StaticDrift, Origin::ThirdParty)
    )
};
```

- [ ] **Step 4: Build (will fail until Origin::Dynamic removed — that's Task 1.4; iterate within the phase)**

Run: `cargo build -p toolr 2>&1 | tail -20`
Expected: compiles after Tasks 1.2–1.4 land; if you do them in order, defer this build to the end of Task 1.4.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr/src/bootstrap.rs
git commit -m "refactor(bootstrap): build manifest statically, never spawn Python"
```

### Task 1.2: `rebuild_manifest_full` becomes static-only

**Files:**

- Modify: `crates/toolr-core/src/dynamic/rebuild.rs`

- [ ] **Step 1: Drop the Python introspection from the rebuild**

Rewrite `rebuild_manifest_full` to take `(project_root, venv_root)` only (no `python`), build the static
manifest with the venv glob, stamp `third_party_hash`, and write. Remove `run_introspect`/`merge_dynamic`
usage.

```rust
pub fn rebuild_manifest_full(project_root: &Path, venv_root: &Path) -> Result<RebuildOutcome> {
    let tools = project_root.join("tools");
    let mut manifest = build_static_manifest_with_venv(&tools, venv_root)
        .with_context(|| "building static manifest (incl. third-party glob-merge)")?;
    manifest.third_party_hash = compute_third_party_hash(venv_root)?;
    let manifest_path = tools.join(".toolr-manifest.json");
    write_manifest(&manifest_path, &manifest)?;
    Ok(RebuildOutcome {
        manifest_path,
        group_count: manifest.groups.len(),
        command_count: manifest.commands.len(),
        warnings: Vec::new(),
    })
}
```

Remove `use super::{merge::merge_dynamic, runner::run_introspect};`. Update the existing test
`full_rebuild_writes_combined_manifest` to call `rebuild_manifest_full(project, &venv)` (drop the fake-python
arg) and assert the `ci` group from `tools/ci.py` is present (it no longer asserts a dynamic `legacy` group).

- [ ] **Step 2: Update callers of `rebuild_manifest_full`**

Find them: `grep -rn 'rebuild_manifest_full' crates/`. Update each call site (notably `project manifest
rebuild` in the dispatch/CLI path) to drop the `python` argument.

- [ ] **Step 3: Commit (defer build to Task 1.4)**

```bash
git add crates/toolr-core/src/dynamic/rebuild.rs
git commit -m "refactor(dynamic): make rebuild_manifest_full static-only"
```

### Task 1.3: Delete the dynamic execution modules and the Python helper

**Files:**

- Delete: `crates/toolr-core/src/dynamic/{runner,payload,merge}.rs`
- Delete: `crates/toolr-py/python/toolr/_introspect.py`
- Delete: `tests/introspect/` (whole directory)
- Modify: `crates/toolr-core/src/dynamic/mod.rs`
- [ ] **Step 1: Remove the files**

```bash
git rm crates/toolr-core/src/dynamic/runner.rs \
       crates/toolr-core/src/dynamic/payload.rs \
       crates/toolr-core/src/dynamic/merge.rs \
       crates/toolr-py/python/toolr/_introspect.py
git rm -r tests/introspect
```

- [ ] **Step 2: Update `dynamic/mod.rs`**

Keep only `hash` and `rebuild`. Remove `pub mod runner; pub mod payload; pub mod merge;` and their re-exports
(`merge_dynamic`, `DynamicPayload`, `PAYLOAD_SCHEMA_VERSION`, `run_introspect`, `IntrospectError`). Resulting
file:

```rust
//! Manifest build helpers: static third-party glob-merge + hashing.
//! (Historically also hosted a Python introspection layer, now removed —
//! toolr never executes repository code to build the manifest.)

pub mod hash;
pub mod rebuild;

pub use hash::{compute_third_party_hash, empty_third_party_hash};
pub use rebuild::{RebuildOutcome, rebuild_manifest_full};

#[cfg(test)]
mod tests;
```

- [ ] **Step 3: Check `dynamic/tests.rs`** for references to the deleted symbols (`merge_dynamic`, payload);
      delete those test fns. Run `grep -n 'merge_dynamic\|DynamicPayload\|run_introspect\|PAYLOAD_SCHEMA'
      crates/toolr-core/src/dynamic/tests.rs`.

- [ ] **Step 4: Commit (defer build to Task 1.4)**

```bash
git add -A
git commit -m "refactor(dynamic): delete Python introspection layer (_introspect, runner, payload, merge)"
```

### Task 1.4: Remove `Origin::Dynamic`

**Files:**

- Modify: `crates/toolr-core/src/manifest/model.rs:239-249`

- [ ] **Step 1: Remove the variant**

Delete the `Dynamic,` variant and its doc comment from the `Origin` enum. Because `Origin` derives
`Deserialize` with `#[serde(rename_all = "snake_case")]`, a legacy manifest containing `"origin":"dynamic"`
would now fail to deserialize. Make load lenient: add `#[serde(other)]` fallback OR a custom default. Simplest
robust approach — add a catch-all that maps unknown origins to `Static` is wrong (would wrongly trust);
instead, have the manifest loader treat a deserialize failure as "absent manifest → rebuild." Confirm
`manifest/io.rs` already returns `Err` on bad JSON and that `bootstrap`/freshness treat a load error as
"rebuild from source" (it does: `load_manifest(...).ok()`), so a legacy dynamic entry simply triggers a clean
static rebuild. Add a test in `manifest/tests.rs`:

```rust
#[test]
fn legacy_dynamic_origin_is_not_loadable_and_triggers_rebuild() {
    // A manifest written by an older toolr with an entry origin "dynamic"
    // must not deserialize into the new enum; callers treat that as absent.
    let json = r#"{"schema_version":1,"static_hash":"x","third_party_hash":"",
        "groups":[],"commands":[{"name":"c","group":"g","module":"m","function":"f",
        "summary":"","description":"","arguments":[],"imports":[],"origin":"dynamic",
        "dispatched_from":null,"is_dispatcher":false}]}"#;
    let parsed: Result<crate::manifest::Manifest, _> = serde_json::from_str(json);
    assert!(parsed.is_err());
}
```

- [ ] **Step 2: Fix all match arms / constructors referencing `Origin::Dynamic`**

Run: `grep -rn 'Origin::Dynamic' crates/`. Remove every arm. Likely sites: `freshness/compare.rs` doc comment
(text only), `bootstrap.rs` (done in 1.1), `manifest/tests.rs`, `dynamic/merge.rs` (deleted). Also remove the
`"dynamic"` origin literal from any remaining Python (`_runner.py`, `utils/command.py`) — `grep -rn
'"dynamic"\|dynamic' crates/toolr-py/python/toolr | grep -i origin`.

- [ ] **Step 3: Regenerate skill refs (the `Origin` enum is a `// region:` skill-ref source)**

Run: `cargo xtask build-skill-refs`
Then: `git diff --stat skills/` to confirm the `SkillRefOrigin` region updated.

- [ ] **Step 4: Build the workspace**

Run: `cargo build --workspace 2>&1 | tail -20`
Expected: clean build. Fix any remaining `Origin::Dynamic` references it flags.

- [ ] **Step 5: Run Rust tests for the touched crates**

Run: `cargo test -p toolr-core 2>&1 | tail -20`
Expected: PASS (freshness, manifest, dynamic tests green).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor(manifest): remove Origin::Dynamic; static rebuild covers legacy manifests"
```

### Task 1.5: Confirm Phase 0 help test now goes green

- [ ] **Step 1: Run the regression test**

Run: `cargo test -p toolr --test untrusted_repo help_does_not_execute_committed_interpreter -- --nocapture`
Expected: PASS — `--help` now builds statically and spawns no Python. (The `bare_invocation` test should also pass.)

- [ ] **Step 2: Add a fresh-clone help test**

Append to `crates/toolr/tests/untrusted_repo.rs`:

```rust
#[test]
fn help_works_with_no_venv_and_shows_first_party_commands() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    std::fs::write(
        tools.join("pyproject.toml"),
        "[project]\nname=\"demo\"\nversion=\"0\"\n",
    )
    .unwrap();
    std::fs::write(
        tools.join("greet.py"),
        "\"\"\"Greetings.\"\"\"\nfrom toolr import command_group\ngroup = command_group(\"greet\", \"Greetings\")\n@group.command\ndef hi(ctx):\n    \"\"\"Say hi.\"\"\"\n",
    )
    .unwrap();

    Command::cargo_bin("toolr")
        .unwrap()
        .arg("--help")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("greet"));
}
```

Add `predicates` to `[dev-dependencies]` in `crates/toolr/Cargo.toml` if not already present (`grep predicates
crates/toolr/Cargo.toml`).

- [ ] **Step 3: Run & commit**

Run: `cargo test -p toolr --test untrusted_repo`
Expected: PASS.

```bash
git add crates/toolr/tests/untrusted_repo.rs crates/toolr/Cargo.toml
git commit -m "test(security): --help builds statically with no venv (SEC-01 green)"
```

---

## Phase 2 — Interpreter provenance

### Task 2.1: Extend `cache::meta::Meta` with interpreter provenance

**Files:**

- Modify: `crates/toolr-core/src/cache/meta.rs`

- [ ] **Step 1: Add fields + bump schema**

Bump `SCHEMA_VERSION` to `2`. Add two optional fields so older (v1) sidecars still load:

```rust
pub const SCHEMA_VERSION: u32 = 2;

// add to struct Meta:
    /// Canonical, symlink-resolved path of the interpreter toolr provisioned
    /// for this repo. None for v1 sidecars (pre-provenance).
    #[serde(default)]
    pub interpreter_path: Option<PathBuf>,
    /// blake3 hex of the interpreter file's bytes at provision time.
    #[serde(default)]
    pub interpreter_hash: Option<String>,
```

Update `Meta::new` to set both to `None`; add a builder `with_interpreter(self, path: PathBuf, hash: String)
-> Self`. Keep `load` rejecting versions `> SCHEMA_VERSION` and accepting v1 (the `#[serde(default)]` covers
the missing fields).

- [ ] **Step 2: Test v1 → v2 lenient load**

Add to `meta.rs` tests:

```rust
#[test]
fn loads_v1_sidecar_without_provenance_fields() {
    let tmp = TempDir::new().unwrap();
    let v1 = r#"{"schema_version":1,"repo_path":"/r","toolr_version":"0","python_version":"3.13","created_at":"2026-01-01T00:00:00Z","last_used_at":"2026-01-01T00:00:00Z"}"#;
    std::fs::write(tmp.path().join("meta.json"), v1).unwrap();
    let m = Meta::load(tmp.path()).unwrap();
    assert!(m.interpreter_path.is_none());
}
```

- [ ] **Step 3: Run & commit**

Run: `cargo test -p toolr-core cache::meta 2>&1 | tail -10`

```bash
git add crates/toolr-core/src/cache/meta.rs
git commit -m "feat(cache): record interpreter provenance in meta.json (schema v2)"
```

### Task 2.2: Provenance verification module

**Files:**

- Create: `crates/toolr-core/src/venv/provenance.rs`
- Modify: `crates/toolr-core/src/venv/mod.rs` (add `pub mod provenance;`)
- [ ] **Step 1: Write the test first**

Create `provenance.rs` with this test module describing the contract:

```rust
//! Verify a resolved interpreter is one toolr is allowed to execute.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::cache::meta::Meta;
use crate::hash::hash_file; // see Step 3 — add if missing
use crate::uv::toolr_cache_dir;

/// Why an interpreter was rejected.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProvenanceError {
    #[error("refusing to run {0}: it lives inside the repository and was not provisioned by toolr — run `toolr project venv sync`")]
    UntrustedInRepo(PathBuf),
}

/// Returns Ok(()) if `interpreter` is safe to execute for `repo_root`.
///
/// Trusted when EITHER:
/// - the canonical interpreter path is under toolr's cache dir, OR
/// - it is inside the repo tree but matches the provenance record in the
///   out-of-repo `meta.json` for this repo (path + content hash).
pub fn verify_interpreter(
    interpreter: &Path,
    repo_root: &Path,
    cache_dir_for_repo: Option<&Path>,
) -> Result<(), ProvenanceError> {
    let canon = interpreter.canonicalize().unwrap_or_else(|_| interpreter.to_path_buf());

    // 1. Under toolr's cache dir → trusted (repo can't write there).
    if let Some(cache_root) = toolr_cache_dir() {
        if canon.starts_with(&cache_root) {
            return Ok(());
        }
    }

    // 2. Inside the repo tree → require a matching provenance record.
    let repo_canon = repo_root.canonicalize().unwrap_or_else(|_| repo_root.to_path_buf());
    if canon.starts_with(&repo_canon) {
        if let Some(cache_dir) = cache_dir_for_repo {
            if let Ok(meta) = Meta::load(cache_dir) {
                if meta.interpreter_path.as_deref() == Some(canon.as_path())
                    && meta.interpreter_hash.as_deref()
                        == hash_file(&canon).ok().as_deref()
                {
                    return Ok(());
                }
            }
        }
        return Err(ProvenanceError::UntrustedInRepo(canon));
    }

    // 3. Outside repo, outside cache (e.g. system python for non-venv repos) → allowed.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn write_exec(p: &Path, body: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn rejects_unrecorded_in_repo_interpreter() {
        let repo = TempDir::new().unwrap();
        let py = repo.path().join("tools/.venv/bin/python");
        write_exec(&py, "#!/bin/sh\n");
        let cache = TempDir::new().unwrap(); // empty: no meta.json
        let err = verify_interpreter(&py, repo.path(), Some(cache.path())).unwrap_err();
        assert!(matches!(err, ProvenanceError::UntrustedInRepo(_)));
    }

    #[test]
    #[cfg(unix)]
    fn accepts_recorded_in_repo_interpreter() {
        let repo = TempDir::new().unwrap();
        let py = repo.path().join("tools/.venv/bin/python");
        write_exec(&py, "#!/bin/sh\necho hi\n");
        let canon = py.canonicalize().unwrap();
        let cache = TempDir::new().unwrap();
        Meta::new(repo.path(), "0", "3.13")
            .with_interpreter(canon.clone(), crate::hash::hash_file(&canon).unwrap())
            .write(cache.path())
            .unwrap();
        assert!(verify_interpreter(&py, repo.path(), Some(cache.path())).is_ok());
    }
}
```

- [ ] **Step 2: Add `pub mod provenance;` to `crates/toolr-core/src/venv/mod.rs`** and re-export
      `verify_interpreter`, `ProvenanceError` if the crate's style re-exports (check neighbors).

- [ ] **Step 3: Ensure `hash_file` exists**

Check: `grep -n 'pub fn hash_file' crates/toolr-core/src/hash.rs`. If absent, add a small helper that
blake3-hashes a file's bytes and returns hex (mirror the existing hashing style in `hash.rs`), with a unit
test.

- [ ] **Step 4: Run & commit**

Run: `cargo test -p toolr-core venv::provenance 2>&1 | tail -15`
Expected: PASS.

```bash
git add crates/toolr-core/src/venv/provenance.rs crates/toolr-core/src/venv/mod.rs crates/toolr-core/src/hash.rs
git commit -m "feat(venv): interpreter provenance verification"
```

### Task 2.3: Write provenance on sync; rebuild manifest as sync's final step

**Files:**

- Modify: `crates/toolr-core/src/venv/sync.rs` and/or `crates/toolr-core/src/project.rs` (whichever owns
  `ensure_venv_ready` / the post-sync step — confirm with `grep -rn 'ensure_venv_ready\|fn sync'
  crates/toolr-core/src/venv crates/toolr-core/src/project.rs`).

- [ ] **Step 1: After a successful sync, record provenance**

Where the venv is confirmed ready (interpreter exists + `validate_venv` passes), compute the repo-key
(`compute_repo_key(repo_root, &python_version)`), build the cache dir path
(`toolr_cache_dir()?.join(&repo_key)`), and write/refresh `meta.json` with
`.with_interpreter(canonical_python, hash_file(&canonical_python)?)`. For cache venvs the meta dir is the
venv's parent (existing pattern in dispatch.rs:265); for in-tree venvs use the computed
`toolr_cache_dir()/<repo-key>` dir (the venv stays in `tools/.venv`).

- [ ] **Step 2: Rebuild the manifest as the final sync step**

After provenance is written, call `rebuild_manifest_full(repo_root, &venv_dir)` so third-party plugin commands
appear immediately post-sync (design §3).

- [ ] **Step 3: Test**

Add a `venv` test (or extend `project.rs` tests) that, after a simulated sync into a temp repo with a fake but
recorded interpreter, `Meta::load(cache_dir).interpreter_path` is `Some` and `.toolr-manifest.json` exists.
Where a real `uv` sync is impractical in a unit test, factor the "record provenance + rebuild" into a pure
function `finalize_sync(repo_root, venv_dir, python, python_version)` and test that directly.

- [ ] **Step 4: Run & commit**

Run: `cargo test -p toolr-core 2>&1 | tail -15`

```bash
git add -A
git commit -m "feat(venv): record interpreter provenance and rebuild manifest on sync"
```

### Task 2.4: Enforce provenance at dispatch

**Files:**

- Modify: `crates/toolr/src/dispatch.rs` (around the interpreter-existence check at `dispatch.rs:296-308`,
  before `spawn_runner` at `:309`).

- [ ] **Step 1: Insert the provenance gate**

After the `!python.is_file()` bail and before `spawn_runner`, when `venv_dir` is `Some`, verify provenance.
Compute the cache dir for the repo (cache venv: `venv.parent()`; in-tree:
`toolr_cache_dir()?.join(compute_repo_key(&repo_root, py_ver)?)`).

```rust
if let Some(venv) = &venv_dir {
    let py_ver = python_version.as_deref().unwrap_or("");
    let cache_dir = cache_dir_for_repo(venv, &repo_root, py_ver); // helper: cache-parent or toolr_cache_dir/<key>
    if let Err(e) =
        toolr_core::venv::provenance::verify_interpreter(&python, &repo_root, cache_dir.as_deref())
    {
        anyhow::bail!("toolr: {e}");
    }
}
```

Add the small `cache_dir_for_repo` helper in dispatch.rs (or export one from `toolr_core::venv`). Keep the
existing `touch_or_backfill` block — but note it backfills a v1-style meta; ensure backfill does not clobber
provenance fields (use the meta's update path, not a fresh `Meta::new`, when one already exists).

- [ ] **Step 2: Make the malicious-repo dispatch test green**

Add to `crates/toolr/tests/untrusted_repo.rs`: a repo with a committed in-tree `tools/.venv/bin/python` (no
cache provenance record) and a static command; `toolr <cmd>` must exit non-zero with a message containing "not
provisioned by toolr" and must not create the sentinel.

```rust
#[test]
#[cfg(unix)]
fn dispatch_refuses_committed_interpreter() {
    let out = TempDir::new().unwrap();
    let sentinel = out.path().join("sentinel");
    let repo = malicious_repo(&sentinel);
    // give it a statically-parseable command so dispatch is attempted
    std::fs::write(
        repo.path().join("tools").join("hello.py"),
        "\"\"\"Hi.\"\"\"\nfrom toolr import command_group\ngroup = command_group(\"hello\", \"Hi\")\n@group.command\ndef world(ctx):\n    \"\"\"World.\"\"\"\n",
    ).unwrap();

    Command::cargo_bin("toolr").unwrap()
        .args(["hello", "world"])
        .current_dir(repo.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("not provisioned by toolr"));
    assert!(!sentinel.exists());
}
```

- [ ] **Step 3: Run & commit**

Run: `cargo test -p toolr --test untrusted_repo`
Expected: all PASS.

```bash
git add crates/toolr/src/dispatch.rs crates/toolr/tests/untrusted_repo.rs
git commit -m "feat(dispatch): refuse to execute non-provisioned in-repo interpreters (SEC-01)"
```

---

## Phase 3 — Manifest-as-cache: venv-appeared rebuild

### Task 3.1: Rebuild when a venv appears (third-party axis)

**Files:**

- Modify: `crates/toolr/src/bootstrap.rs` (`ensure_manifest_fresh`) — confirm it passes `venv_dir` into
  `compare` so an absent→present venv is detected as `ThirdPartyDrift`.

- [ ] **Step 1: Verify/adjust the freshness call**

`ensure_manifest_fresh` already resolves `venv_dir` and calls `compare(cached, &tools, venv_dir.as_deref())`.
Confirm that when the manifest was first written with `third_party_hash == empty` (no venv) and a venv now
exists, `compare` returns `ThirdPartyDrift` (it does: `compute_third_party_hash(v)` differs from empty). No
code change may be needed — prove it with a test.

- [ ] **Step 2: Integration test**

Add to `crates/toolr/tests/untrusted_repo.rs` (or a new `manifest_freshness.rs`): build a repo, run `toolr
--help` (writes a venv-less manifest, empty third-party hash), then create a fake venv dir with a
`site-packages/<pkg>/toolr-manifest.json` declaring a third-party command, then run `toolr --help` again and
assert the new command appears. (Use a minimal valid third-party fragment; copy the shape from
`examples/plugin-package/src/toolr_example_plugin/toolr-manifest.json`.)

- [ ] **Step 3: Run & commit**

```bash
git add crates/toolr/tests/*.rs
git commit -m "test(freshness): manifest rebuilds when a venv appears"
```

---

## Phase 4 — Document the static-only contract & release notes

### Task 4.1: Document the contract

**Files:**

- Modify: `skills/toolr-command-authoring/SKILL.md` (and any `docs/` authoring page).

- [ ] **Step 1: State the contract**

Add a short, explicit section: *toolr discovers commands only by static analysis. Declare `command_group(...)`
at module top level and apply `@group.command` to module-level functions. Commands registered dynamically (in
loops, conditionals, or factory functions) are not discovered and will not appear in `--help`, completion, or
dispatch.*

- [ ] **Step 2: Regenerate skill refs**

Run: `cargo xtask build-skill-refs`
Then commit any regenerated `skills/**/references/*.md`.

- [ ] **Step 3: Commit**

```bash
git add skills/ docs/
git commit -m "docs(authoring): document the static-only command-discovery contract"
```

### Task 4.2: Release notes

**Files:**

- Modify: `UNRELEASED.md`

- [ ] **Step 1: Add entries** (do NOT edit `CHANGELOG.md`)

```markdown
### Security

- toolr no longer executes repository Python to build its command manifest.
  `toolr --help`, completion, and first-run are now fully static (AST parse +
  execution-free third-party glob). Repository code runs only on explicit
  command dispatch, through a provenance-verified interpreter. A committed
  `tools/.venv` is refused unless toolr provisioned it (`toolr project venv sync`).

### Removed

- The dynamic introspection layer (`toolr._introspect`) is gone. Commands
  registered dynamically (not via top-level `command_group(...)` + module-level
  `@group.command`) are no longer discovered. Third-party plugins via shipped
  `toolr-manifest.json` are unaffected.
```

- [ ] **Step 2: Commit**

```bash
git add UNRELEASED.md
git commit -m "docs(unreleased): note static-only manifest and provenance"
```

---

## Phase 5 — Full verification & audit status update

### Task 5.1: Umbrella test run

- [ ] **Step 1: Run the full suite** (poll long runs every 30–60s)

Run: `mise run test`
Expected: skill-refs drift gate passes, `cargo test --workspace` green, `pytest` green.

- [ ] **Step 2: Targeted security re-check**

Run: `cargo test -p toolr --test untrusted_repo`
Expected: all PASS.

- [ ] **Step 3: Grep for leftovers**

Run: `grep -rn 'Origin::Dynamic\|merge_dynamic\|run_introspect\|_introspect\|PAYLOAD_SCHEMA_VERSION' crates/ tests/`
Expected: no hits outside historical CHANGELOG/spec text.

### Task 5.2: Update audit tracking

**Files:**

- Modify: `audit/2026-06-10/README.md`, `audit/2026-06-10/security/SEC-01-*.md`, `SEC-03-*.md` (these are
  local/untracked — edit them in the main working tree, not committed).

- [ ] **Step 1:** Flip SEC-01 status to `Done: <this branch / PR>` and SEC-03 to `Done (subsumed): <same>` in
      both the README table and the finding files. (Leave them untracked per the "keep findings local"
      decision.)

> Done when: `mise run test` is green, all `untrusted_repo` tests pass, the leftover-grep is clean, and skill
> refs are regenerated and committed.

---

## Self-review notes (author)

- **Spec coverage:** §1 (Phase 1), §2 provenance (Phase 2), §3 manifest-as-cache + venv-appeared + sync
  rebuild (Tasks 2.3 Step 2 + Phase 3), §4 documented contract (Task 4.1). SEC-03 subsumption: Task 1.1 Step 3
  and Task 1.4. ✓
- **Schema bumps:** only `cache::meta::SCHEMA_VERSION` (1→2, lenient). Runner spec untouched → no
  `RUNNER_SCHEMA_VERSION`/`SCHEMA_VERSION` (Python) bump. ✓
- **Build ordering:** Tasks 1.1–1.4 are interdependent (removing `Origin::Dynamic` is what makes 1.1/1.3
  compile); build/test gate is at Task 1.4 Step 4–5, not after each. Flagged inline.
- **Open verification for implementer:** exact location of `ensure_venv_ready`/sync finalize (Task 2.3) and
  `project manifest rebuild` caller (Task 1.2 Step 2) — resolve via the given greps before editing.

```text
