<!-- rumdl-disable MD046 MD076 -->

# Plan 8: Cache Management

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Lint:** Plan docs nest fenced code inside list items for step-by-step
> structure. The `<!-- rumdl-disable MD046 MD076 -->` directive above turns
> off the code-block-style and list-item-spacing rules for this file only.

**Goal:** Make the toolr cache observable and prunable. Each cached venv gets
a `meta.json` sidecar at the cache root carrying repo path, toolr version,
python version, and timestamps. Every toolr invocation touches `last_used_at`.
A `toolr self cache` command group lets users list, prune, or nuke the cache.
A passive size-hint on each invocation nudges users when the cache grows past
sensible thresholds.

**Architecture:** Plan 3 introduces the cache layout
`$XDG_CACHE_HOME/toolr/<repo-key>/venv/`. Plan 8 adds a peer file at
`$XDG_CACHE_HOME/toolr/<repo-key>/meta.json` and a new
`_rust_utils::cache` module that owns read/write of those sidecars, cache
enumeration, size accounting, pruning, and the passive-hint logic. The binary
wires a `toolr self cache { list | prune [--all] }` subcommand tree into clap,
and every command path that ends in an execute (Plan 2) touches `last_used_at`
once. The passive hint is computed in `_rust_utils::cache::size_hint` and
printed by the binary on its way to dispatch, before any subprocess spawns.

**Tech Stack:** Rust 2021, clap (`derive`, `string`), serde + serde_json,
chrono (RFC 3339 timestamps), filetime (mtime touch), humansize (formatting
sizes for the user), anyhow + thiserror (error plumbing), tempfile +
assert_cmd (tests). `chrono` and `humansize` are new dependencies introduced
in this plan; `filetime` may already be transitive but is added explicitly.

**Reading order in this plan:** Tasks build on each other. Don't skip ahead;
later tasks reference types defined in earlier ones. Plan 3 must be merged
before Plan 8 starts — the cache directory layout and `venv_path()` helper
live there.

---

## Task 1: Cache metadata data model

Create the `_rust_utils::cache::Meta` struct that maps onto the on-disk
`meta.json` sidecar. Round-trip through serde_json.

**Files:**

- Modify: `Cargo.toml`
- Modify: `src/lib.rs`
- Create: `src/cache/mod.rs`
- Create: `src/cache/meta.rs`
- Create: `src/cache/tests.rs`
- [x] **Step 1.1: Add `chrono` to `[dependencies]` in `Cargo.toml`**

    ```toml
    chrono = { version = "0.4", default-features = false, features = ["clock", "serde", "std"] }
    ```

    Rationale: `clock` gives us `Utc::now()`, `serde` gives us
    `#[serde(with = ...)]`-free RFC 3339 (de)serialization out of the box,
    `std` is needed for our targets, and disabling defaults keeps the
    Windows time-crate transitive tree predictable.

- [x] **Step 1.2: Expose a `cache` module from `src/lib.rs`**

    Add alongside the other `pub mod` lines:

    ```rust
    pub mod cache;
    ```

- [x] **Step 1.3: Create `src/cache/mod.rs`**

    ```rust
    //! Toolr venv cache: per-venv metadata sidecar, enumeration, pruning,
    //! and passive size hints.

    pub mod meta;

    pub use meta::{Meta, MetaError, SCHEMA_VERSION};

    #[cfg(test)]
    mod tests;
    ```

- [x] **Step 1.4: Create `src/cache/meta.rs`**

    ```rust
    //! Per-cache-entry `meta.json` sidecar.
    //!
    //! Layout (alongside the venv that Plan 3 manages):
    //!
    //! ```text
    //! $XDG_CACHE_HOME/toolr/<repo-key>/
    //!     venv/         (managed by Plan 3)
    //!     meta.json     (this module)
    //! ```

    use std::fs;
    use std::path::{Path, PathBuf};

    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Serialize};
    use thiserror::Error;

    /// Current sidecar schema version. Bump on breaking format changes;
    /// `Meta::load` rejects newer versions and silently upgrades older ones
    /// in-process if migrations are added.
    pub const SCHEMA_VERSION: u32 = 1;

    /// Filename used for the sidecar inside the per-repo cache directory.
    pub const FILE_NAME: &str = "meta.json";

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Meta {
        /// Schema version. Defaults to 1 for files written by older toolr.
        #[serde(default = "default_schema_version")]
        pub schema_version: u32,
        /// Absolute, symlink-resolved repo path that owns this venv.
        pub repo_path: PathBuf,
        /// Toolr binary version that created this entry.
        pub toolr_version: String,
        /// Concrete Python version used (e.g. "3.13.1").
        pub python_version: String,
        /// When this cache entry was first materialised.
        pub created_at: DateTime<Utc>,
        /// Updated on every toolr invocation against this cache entry.
        pub last_used_at: DateTime<Utc>,
    }

    fn default_schema_version() -> u32 {
        1
    }

    #[derive(Debug, Error)]
    pub enum MetaError {
        #[error("I/O error: {0}")]
        Io(#[from] std::io::Error),
        #[error("JSON error: {0}")]
        Json(#[from] serde_json::Error),
        #[error("unknown meta schema_version {0}; this toolr supports up to {}", SCHEMA_VERSION)]
        UnknownSchemaVersion(u32),
    }

    impl Meta {
        /// Build a fresh `Meta`. `created_at` and `last_used_at` are set to
        /// the same instant.
        pub fn new(
            repo_path: impl Into<PathBuf>,
            toolr_version: impl Into<String>,
            python_version: impl Into<String>,
        ) -> Self {
            let now = Utc::now();
            Self {
                schema_version: SCHEMA_VERSION,
                repo_path: repo_path.into(),
                toolr_version: toolr_version.into(),
                python_version: python_version.into(),
                created_at: now,
                last_used_at: now,
            }
        }

        /// Return the directory portion of a cache entry given the full path
        /// to either the `meta.json` file or the `venv/` subdir.
        pub fn cache_dir_of(path: &Path) -> Option<&Path> {
            if path.file_name().map(|n| n == FILE_NAME).unwrap_or(false) {
                path.parent()
            } else if path.file_name().map(|n| n == "venv").unwrap_or(false) {
                path.parent()
            } else {
                Some(path)
            }
        }

        /// Path of the sidecar file inside `cache_dir`.
        pub fn path_in(cache_dir: &Path) -> PathBuf {
            cache_dir.join(FILE_NAME)
        }

        /// Load `meta.json` from `cache_dir`.
        pub fn load(cache_dir: &Path) -> Result<Self, MetaError> {
            let path = Self::path_in(cache_dir);
            let bytes = fs::read(&path)?;
            let raw: serde_json::Value = serde_json::from_slice(&bytes)?;
            let version = raw
                .get("schema_version")
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as u32;
            if version > SCHEMA_VERSION {
                return Err(MetaError::UnknownSchemaVersion(version));
            }
            let meta: Meta = serde_json::from_value(raw)?;
            Ok(meta)
        }

        /// Atomically write `meta.json` into `cache_dir`. The directory is
        /// created if missing.
        pub fn write(&self, cache_dir: &Path) -> Result<(), MetaError> {
            fs::create_dir_all(cache_dir)?;
            let final_path = Self::path_in(cache_dir);
            let tmp_path = cache_dir.join(".meta.json.tmp");
            let bytes = serde_json::to_vec_pretty(self)?;
            fs::write(&tmp_path, bytes)?;
            fs::rename(&tmp_path, &final_path)?;
            Ok(())
        }
    }
    ```

- [x] **Step 1.5: Add round-trip tests in `src/cache/tests.rs`**

    ```rust
    use super::meta::{Meta, MetaError, SCHEMA_VERSION};
    use chrono::{TimeZone, Utc};
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn sample_meta() -> Meta {
        Meta {
            schema_version: SCHEMA_VERSION,
            repo_path: PathBuf::from("/home/u/repo"),
            toolr_version: "1.0.0".into(),
            python_version: "3.13.1".into(),
            created_at: Utc.with_ymd_and_hms(2026, 5, 11, 12, 0, 0).unwrap(),
            last_used_at: Utc.with_ymd_and_hms(2026, 5, 11, 12, 34, 56).unwrap(),
        }
    }

    #[test]
    fn meta_round_trips_through_json() {
        let m = sample_meta();
        let s = serde_json::to_string_pretty(&m).expect("serialize");
        let back: Meta = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(m, back);
    }

    #[test]
    fn meta_write_then_load_round_trips() {
        let tmp = TempDir::new().unwrap();
        let m = sample_meta();
        m.write(tmp.path()).expect("write");
        let loaded = Meta::load(tmp.path()).expect("load");
        assert_eq!(m, loaded);
    }

    #[test]
    fn meta_load_rejects_unknown_schema_version() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("meta.json"),
            r#"{
              "schema_version": 999,
              "repo_path": "/x",
              "toolr_version": "1.0.0",
              "python_version": "3.13.1",
              "created_at": "2026-05-11T12:00:00Z",
              "last_used_at": "2026-05-11T12:00:00Z"
            }"#,
        )
        .unwrap();
        let err = Meta::load(tmp.path()).expect_err("should reject");
        assert!(matches!(err, MetaError::UnknownSchemaVersion(999)));
    }

    #[test]
    fn meta_load_missing_returns_io_error() {
        let tmp = TempDir::new().unwrap();
        let err = Meta::load(tmp.path()).expect_err("should be missing");
        assert!(matches!(err, MetaError::Io(_)));
    }

    #[test]
    fn meta_new_sets_created_and_last_used_equal() {
        let m = Meta::new("/x", "1.0.0", "3.13.1");
        assert_eq!(m.created_at, m.last_used_at);
        assert_eq!(m.schema_version, SCHEMA_VERSION);
    }
    ```

- [x] **Step 1.6: Run tests, expect PASS**

    ```bash
    cargo test --lib cache::
    ```

    Expected: 5 tests passing.

- [x] **Step 1.7: Commit**

    ```bash
    git add Cargo.toml src/lib.rs src/cache/
    git commit -m "feat(cache): Add Meta sidecar data model with serde round-trip tests"
    ```

---

## Task 2: Write `meta.json` on venv creation

Wire the venv-creation path (added by Plan 3) to drop a `meta.json` alongside
the new venv. This task lives in the cache module, not in the venv module, so
the rest of Plan 8 has one place to look.

Plan 3 is assumed to expose `_rust_utils::venv::ensure_venv(...) -> Result<VenvInfo>`
where `VenvInfo` carries `repo_path`, `cache_dir` (i.e. the parent of
`venv/`), and `python_version`. If the actual symbol names differ, adjust at
the call site only; the public API in this plan stays the same.

**Files:**

- Modify: `src/cache/mod.rs`
- Create: `src/cache/init.rs`
- Modify: `src/cache/tests.rs`
- [x] **Step 2.1: Write the failing test in `src/cache/tests.rs`**

    Append:

    ```rust
    use super::init::write_meta_for_new_venv;

    #[test]
    fn write_meta_for_new_venv_creates_sidecar() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("repo-key");
        std::fs::create_dir_all(cache_dir.join("venv")).unwrap();

        let meta = write_meta_for_new_venv(
            &cache_dir,
            "/abs/repo".as_ref(),
            "1.2.3",
            "3.13.1",
        )
        .expect("write meta");

        let loaded = Meta::load(&cache_dir).expect("load meta");
        assert_eq!(meta, loaded);
        assert_eq!(loaded.repo_path, std::path::PathBuf::from("/abs/repo"));
        assert_eq!(loaded.toolr_version, "1.2.3");
        assert_eq!(loaded.python_version, "3.13.1");
        assert_eq!(loaded.created_at, loaded.last_used_at);
    }

    #[test]
    fn write_meta_for_new_venv_overwrites_existing() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("repo-key");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let first = write_meta_for_new_venv(
            &cache_dir,
            "/abs/repo".as_ref(),
            "1.0.0",
            "3.12.0",
        )
        .unwrap();
        let second = write_meta_for_new_venv(
            &cache_dir,
            "/abs/repo".as_ref(),
            "1.0.0",
            "3.13.0",
        )
        .unwrap();
        assert_ne!(first.python_version, second.python_version);
        let loaded = Meta::load(&cache_dir).expect("load");
        assert_eq!(loaded.python_version, "3.13.0");
    }
    ```

- [x] **Step 2.2: Run and verify the tests FAIL**

    ```bash
    cargo test --lib cache::tests::write_meta_for_new_venv
    ```

    Expected: compile error (unresolved import `super::init`).

- [x] **Step 2.3: Create `src/cache/init.rs`**

    ```rust
    //! Hook called by the venv-creation path (Plan 3) to drop a `meta.json`
    //! sidecar next to the freshly-built venv.

    use std::path::Path;

    use super::meta::{Meta, MetaError};

    /// Write a fresh `meta.json` into `cache_dir`. Replaces any existing
    /// sidecar — venv recreation is the only reason this entry point is
    /// hit twice for the same `cache_dir`, and the new venv invalidates the
    /// old metadata.
    pub fn write_meta_for_new_venv(
        cache_dir: &Path,
        repo_path: &Path,
        toolr_version: &str,
        python_version: &str,
    ) -> Result<Meta, MetaError> {
        let meta = Meta::new(repo_path.to_path_buf(), toolr_version, python_version);
        meta.write(cache_dir)?;
        Ok(meta)
    }
    ```

- [x] **Step 2.4: Re-export the new entry point**

    Update `src/cache/mod.rs`:

    ```rust
    pub mod init;
    pub mod meta;

    pub use init::write_meta_for_new_venv;
    pub use meta::{Meta, MetaError, SCHEMA_VERSION};

    #[cfg(test)]
    mod tests;
    ```

- [x] **Step 2.5: Run tests, expect PASS**

    ```bash
    cargo test --lib cache::
    ```

    Expected: 7 tests passing.

- [x] **Step 2.6: Wire the hook into Plan 3's venv creation path**

    In whichever Plan 3 module calls `uv venv` / `uv sync` to materialise a
    new venv (likely `src/venv/ensure.rs` or similar), immediately after the
    venv is reported created successfully, call:

    ```rust
    use _rust_utils::cache::write_meta_for_new_venv;

    let _ = write_meta_for_new_venv(
        &cache_dir,
        &repo_path,
        env!("CARGO_PKG_VERSION"),
        &resolved_python_version,
    )
    .map_err(|e| {
        // Non-fatal: tools work without a sidecar; just warn.
        eprintln!("toolr: warning: failed to write cache meta.json: {e}");
    });
    ```

    Use `let _ = ... .map_err(...)` rather than `?` so a meta-write failure
    never blocks the user's command. If Plan 3 lives behind a feature gate or
    has not yet introduced the relevant function, leave this step
    documentation-only and surface the integration as an Open Question.

- [x] **Step 2.7: Commit**

    ```bash
    git add src/cache/
    git commit -m "feat(cache): Drop meta.json sidecar on venv creation"
    ```

---

## Task 3: Touch `last_used_at` on every invocation

Every toolr invocation that resolves to a cached venv updates
`last_used_at`. To keep this cheap and crash-tolerant, do it as a single JSON
re-write of the existing sidecar (not an mtime-only touch). The spec calls
this "single mtime touch" — we interpret it as "single, cheap I/O round-trip
per invocation", which is what `serde_json::from_slice` + `to_vec_pretty` +
`fs::write` gives us (sub-millisecond on any modern disk for a < 1 KiB
file). A pure mtime touch on the file would not update `last_used_at` in the
JSON, and the JSON value is what `list`/`prune` read.

**Files:**

- Create: `src/cache/touch.rs`
- Modify: `src/cache/mod.rs`
- Modify: `src/cache/tests.rs`
- [ ] **Step 3.1: Write the failing test**

    Append to `src/cache/tests.rs`:

    ```rust
    use super::touch::touch_last_used;

    #[test]
    fn touch_last_used_updates_only_last_used_at() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("repo-key");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let original = write_meta_for_new_venv(
            &cache_dir,
            "/abs/repo".as_ref(),
            "1.0.0",
            "3.13.1",
        )
        .unwrap();

        // Sleep just long enough that the timestamp must differ.
        std::thread::sleep(std::time::Duration::from_millis(20));

        touch_last_used(&cache_dir).expect("touch");
        let after = Meta::load(&cache_dir).expect("load");

        assert_eq!(after.created_at, original.created_at);
        assert_eq!(after.repo_path, original.repo_path);
        assert_eq!(after.toolr_version, original.toolr_version);
        assert_eq!(after.python_version, original.python_version);
        assert!(after.last_used_at > original.last_used_at);
    }

    #[test]
    fn touch_last_used_is_a_noop_when_sidecar_is_missing() {
        let tmp = TempDir::new().unwrap();
        // No meta.json present in cache_dir.
        let result = touch_last_used(tmp.path());
        // Missing sidecar is benign; the function must not error.
        assert!(result.is_ok());
    }
    ```

- [ ] **Step 3.2: Run and verify the tests FAIL**

    ```bash
    cargo test --lib cache::tests::touch_last_used
    ```

    Expected: compile error (unresolved import `super::touch`).

- [ ] **Step 3.3: Create `src/cache/touch.rs`**

    ```rust
    //! Update `last_used_at` on every toolr invocation.

    use std::path::Path;

    use chrono::Utc;

    use super::meta::{Meta, MetaError};

    /// Re-write `meta.json` with a fresh `last_used_at`. Missing sidecars
    /// are silently ignored — older cache entries that predate Plan 8 are
    /// allowed to exist without metadata.
    pub fn touch_last_used(cache_dir: &Path) -> Result<(), MetaError> {
        let mut meta = match Meta::load(cache_dir) {
            Ok(m) => m,
            Err(MetaError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        meta.last_used_at = Utc::now();
        meta.write(cache_dir)?;
        Ok(())
    }
    ```

- [ ] **Step 3.4: Re-export and run tests**

    Update `src/cache/mod.rs`:

    ```rust
    pub mod init;
    pub mod meta;
    pub mod touch;

    pub use init::write_meta_for_new_venv;
    pub use meta::{Meta, MetaError, SCHEMA_VERSION};
    pub use touch::touch_last_used;

    #[cfg(test)]
    mod tests;
    ```

    Then:

    ```bash
    cargo test --lib cache::
    ```

    Expected: 9 tests passing.

- [ ] **Step 3.5: Wire the touch into the per-invocation hot path**

    In the binary's dispatch path (the entry point that runs after CLI
    parsing but before subprocess spawn — typically `src/bin/toolr/dispatch.rs`
    after Plan 2 lands), call `touch_last_used(&cache_dir)` once we know
    which cache entry will be used. Skip the call for built-in subcommands
    that never touch a venv (`toolr --version`, `toolr --help`,
    `toolr self ...`, `toolr __complete`).

    Pseudocode for the dispatch site:

    ```rust
    use _rust_utils::cache::touch_last_used;

    if invocation.requires_venv() {
        let cache_dir = venv_info.cache_dir.clone();
        if let Err(e) = touch_last_used(&cache_dir) {
            // Non-fatal: log and continue.
            eprintln!("toolr: warning: failed to touch cache meta.json: {e}");
        }
    }
    ```

    Keep this off the hot path of cold-fast `--help` / completion.

- [ ] **Step 3.6: Commit**

    ```bash
    git add src/cache/
    git commit -m "feat(cache): Touch last_used_at on every invocation"
    ```

---

## Task 4: Enumerate all cached venvs

Walk `$XDG_CACHE_HOME/toolr/` and surface every `meta.json` plus a few
derived facts (size, orphan status). The enumeration is the foundation for
`list`, `prune`, and the size-hint check.

**Files:**

- Modify: `Cargo.toml`
- Create: `src/cache/enumerate.rs`
- Modify: `src/cache/mod.rs`
- Modify: `src/cache/tests.rs`
- [ ] **Step 4.1: Add `humansize` to `[dependencies]` in `Cargo.toml`**

    ```toml
    humansize = "2"
    ```

    Used by Task 5 (`list` formatting) and Task 8 (size-hint message).
    Pulling it in here keeps the dependency change adjacent to the
    enumeration code that produces the raw byte counts.

- [ ] **Step 4.2: Write the failing tests**

    Append to `src/cache/tests.rs`:

    ```rust
    use super::enumerate::{enumerate_caches, CachedVenv};

    fn make_entry(
        root: &std::path::Path,
        key: &str,
        repo_path: &str,
        last_used: chrono::DateTime<Utc>,
        venv_byte_count: usize,
    ) {
        let cache_dir = root.join(key);
        std::fs::create_dir_all(cache_dir.join("venv")).unwrap();
        // Drop a file in venv/ to give the entry a measurable size.
        std::fs::write(cache_dir.join("venv/blob.bin"), vec![0u8; venv_byte_count]).unwrap();
        let m = Meta {
            schema_version: SCHEMA_VERSION,
            repo_path: std::path::PathBuf::from(repo_path),
            toolr_version: "1.0.0".into(),
            python_version: "3.13.1".into(),
            created_at: last_used,
            last_used_at: last_used,
        };
        m.write(&cache_dir).unwrap();
    }

    #[test]
    fn enumerate_caches_returns_empty_when_root_missing() {
        let tmp = TempDir::new().unwrap();
        let caches = enumerate_caches(&tmp.path().join("no-such-dir")).expect("ok");
        assert!(caches.is_empty());
    }

    #[test]
    fn enumerate_caches_finds_all_meta_sidecars() {
        let tmp = TempDir::new().unwrap();
        let when = Utc.with_ymd_and_hms(2026, 5, 11, 12, 0, 0).unwrap();
        make_entry(tmp.path(), "key-a", "/repo/a", when, 1024);
        make_entry(tmp.path(), "key-b", "/repo/b", when, 2048);

        let mut caches = enumerate_caches(tmp.path()).expect("ok");
        caches.sort_by(|a, b| a.repo_key.cmp(&b.repo_key));
        assert_eq!(caches.len(), 2);
        assert_eq!(caches[0].repo_key, "key-a");
        assert_eq!(caches[1].repo_key, "key-b");
        assert!(caches[0].size_bytes >= 1024);
        assert!(caches[1].size_bytes >= 2048);
        assert!(!caches[0].is_orphan);
        assert!(!caches[1].is_orphan); // /repo/a and /repo/b don't exist on
                                       // disk — but Task 6 owns orphan
                                       // detection. Enumeration only
                                       // populates the field; default false.
    }

    #[test]
    fn enumerate_caches_skips_directories_without_meta() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("partial/venv")).unwrap();
        // No meta.json — enumerate must ignore this entry.
        let caches = enumerate_caches(tmp.path()).expect("ok");
        assert!(caches.is_empty());
    }
    ```

- [ ] **Step 4.3: Run tests, expect FAIL**

    ```bash
    cargo test --lib cache::tests::enumerate
    ```

    Expected: compile error (unresolved import `super::enumerate`).

- [ ] **Step 4.4: Create `src/cache/enumerate.rs`**

    ```rust
    //! Walk the cache root and collect one record per entry.

    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result};
    use walkdir::WalkDir;

    use super::meta::{Meta, FILE_NAME};

    /// One cache entry plus derived facts.
    #[derive(Debug, Clone)]
    pub struct CachedVenv {
        /// Subdirectory name under the cache root (the `<repo-key>`).
        pub repo_key: String,
        /// Absolute path to the per-entry cache directory.
        pub cache_dir: PathBuf,
        /// Parsed sidecar.
        pub meta: Meta,
        /// Disk usage of the entire `cache_dir` subtree (`venv/` plus
        /// `meta.json` plus anything else inside).
        pub size_bytes: u64,
        /// True iff `meta.repo_path` does not exist as a directory anymore.
        /// Populated in Task 6 (`prune`'s detection pass); enumeration
        /// defaults to `false`.
        pub is_orphan: bool,
    }

    /// Sum every regular-file size under `dir`. Returns 0 if `dir` does not
    /// exist. Symlinks are not followed.
    pub fn dir_size_bytes(dir: &Path) -> Result<u64> {
        if !dir.exists() {
            return Ok(0);
        }
        let mut total: u64 = 0;
        for entry in WalkDir::new(dir).follow_links(false) {
            let entry = entry.with_context(|| format!("walking {}", dir.display()))?;
            if entry.file_type().is_file() {
                match entry.metadata() {
                    Ok(md) => total = total.saturating_add(md.len()),
                    Err(_) => continue,
                }
            }
        }
        Ok(total)
    }

    /// Enumerate every cache entry directly under `cache_root`. Missing
    /// `cache_root` returns an empty vector. Entries without a `meta.json`
    /// are silently skipped — they predate Plan 8 or were partially
    /// created.
    pub fn enumerate_caches(cache_root: &Path) -> Result<Vec<CachedVenv>> {
        let mut out = Vec::new();
        let read = match std::fs::read_dir(cache_root) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => {
                return Err(e).with_context(|| {
                    format!("reading cache root {}", cache_root.display())
                })
            }
        };

        for entry in read {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let cache_dir = entry.path();
            let sidecar = cache_dir.join(FILE_NAME);
            if !sidecar.is_file() {
                continue;
            }
            let meta = match Meta::load(&cache_dir) {
                Ok(m) => m,
                Err(_) => continue, // malformed sidecars are ignored
            };
            let size_bytes = dir_size_bytes(&cache_dir)?;
            let repo_key = entry
                .file_name()
                .to_string_lossy()
                .into_owned();
            out.push(CachedVenv {
                repo_key,
                cache_dir,
                meta,
                size_bytes,
                is_orphan: false,
            });
        }
        Ok(out)
    }
    ```

- [ ] **Step 4.5: Re-export the new types**

    Update `src/cache/mod.rs`:

    ```rust
    pub mod enumerate;
    pub mod init;
    pub mod meta;
    pub mod touch;

    pub use enumerate::{dir_size_bytes, enumerate_caches, CachedVenv};
    pub use init::write_meta_for_new_venv;
    pub use meta::{Meta, MetaError, SCHEMA_VERSION};
    pub use touch::touch_last_used;

    #[cfg(test)]
    mod tests;
    ```

- [ ] **Step 4.6: Run tests, expect PASS**

    ```bash
    cargo test --lib cache::
    ```

    Expected: 12 tests passing.

- [ ] **Step 4.7: Commit**

    ```bash
    git add Cargo.toml src/cache/
    git commit -m "feat(cache): Enumerate cache entries with size accounting"
    ```

---

## Task 5: `toolr self cache list`

Wire the CLI for `toolr self cache list`. Tabular output: repo, size, last
use. Use a small in-crate formatter rather than pulling in a fully-featured
table crate; the output is three columns.

The `toolr self ...` namespace is reserved for binary-state operations (per
00-design). If Plan 4 or another upstream plan has already wired
`toolr self <subcommand>`, this task only adds the `cache` arm. Otherwise the
parent group is introduced here.

**Files:**

- Create: `src/bin/toolr/self_cmd/mod.rs`
- Create: `src/bin/toolr/self_cmd/cache.rs`
- Modify: `src/bin/toolr/cli.rs`
- Modify: `src/bin/toolr/main.rs`
- [ ] **Step 5.1: Introduce or extend the `self` subcommand tree in `src/bin/toolr/cli.rs`**

    If the `Self` arm does not yet exist, add it. If it exists (Plan 4 added
    `toolr self completion`), add `Cache` to its sibling enum. The pattern:

    ```rust
    use clap::{Args, Subcommand};

    #[derive(Subcommand, Debug)]
    pub enum TopLevel {
        /// Operations on toolr's own state.
        #[command(name = "self")]
        Self_(SelfArgs),
        // ... other top-level subcommands wired by earlier plans ...
    }

    #[derive(Args, Debug)]
    pub struct SelfArgs {
        #[command(subcommand)]
        pub command: SelfCommand,
    }

    #[derive(Subcommand, Debug)]
    pub enum SelfCommand {
        /// Manage the cache of per-repo virtualenvs.
        Cache(CacheArgs),
        // ... `Completion(...)` etc. wired by other plans ...
    }

    #[derive(Args, Debug)]
    pub struct CacheArgs {
        #[command(subcommand)]
        pub command: CacheCommand,
    }

    #[derive(Subcommand, Debug)]
    pub enum CacheCommand {
        /// List every cached virtualenv with size and last-use timestamp.
        List,
        /// Prune cached virtualenvs. By default removes orphans (whose
        /// origin repo no longer exists) and stale entries (idle longer
        /// than the configured threshold).
        Prune(PruneArgs),
    }

    #[derive(Args, Debug)]
    pub struct PruneArgs {
        /// Remove every cache entry, not just orphans and stale ones.
        #[arg(long)]
        pub all: bool,
        /// Override the staleness threshold in days. Default 30.
        #[arg(long, value_name = "DAYS", default_value_t = 30)]
        pub stale_after_days: u32,
        /// Show what would be deleted without actually deleting it.
        #[arg(long)]
        pub dry_run: bool,
    }
    ```

- [ ] **Step 5.2: Create the cache-command dispatch module skeleton**

    `src/bin/toolr/self_cmd/mod.rs`:

    ```rust
    //! Dispatchers for `toolr self <...>` subcommands.

    pub mod cache;
    ```

    `src/bin/toolr/self_cmd/cache.rs`:

    ```rust
    //! `toolr self cache <...>` implementation.

    use std::io::{self, Write};
    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result};
    use chrono::{DateTime, Utc};
    use humansize::{format_size, BINARY};

    use _rust_utils::cache::{enumerate_caches, CachedVenv};

    use crate::cli::{CacheCommand, PruneArgs};

    /// Resolve `$XDG_CACHE_HOME/toolr/`. macOS does not set `XDG_CACHE_HOME`
    /// by default; we use `~/.cache/toolr/` consistently across platforms,
    /// matching `00-design.md`'s convention.
    pub fn resolve_cache_root() -> Result<PathBuf> {
        if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
            if !xdg.is_empty() {
                return Ok(PathBuf::from(xdg).join("toolr"));
            }
        }
        let home =
            std::env::var("HOME").context("$HOME is not set; cannot resolve cache root")?;
        Ok(PathBuf::from(home).join(".cache").join("toolr"))
    }

    pub fn run(command: CacheCommand) -> Result<()> {
        let root = resolve_cache_root()?;
        match command {
            CacheCommand::List => run_list(&root, &mut io::stdout().lock()),
            CacheCommand::Prune(args) => run_prune(&root, &args, &mut io::stdout().lock()),
        }
    }

    pub fn run_list(cache_root: &Path, out: &mut dyn Write) -> Result<()> {
        let mut caches = enumerate_caches(cache_root)?;
        // Sort by last-used, most-recent first.
        caches.sort_by(|a, b| b.meta.last_used_at.cmp(&a.meta.last_used_at));

        if caches.is_empty() {
            writeln!(out, "toolr: no cached virtualenvs found under {}", cache_root.display())?;
            return Ok(());
        }
        write_table(out, &caches)?;
        Ok(())
    }

    /// Prune is implemented in Task 6/7. For now, the `List` arm is the only
    /// wired branch; `Prune` returns a not-implemented error so we can land
    /// Task 5 before Task 6.
    pub fn run_prune(_cache_root: &Path, _args: &PruneArgs, _out: &mut dyn Write) -> Result<()> {
        anyhow::bail!("toolr self cache prune is implemented in the next task")
    }

    fn write_table(out: &mut dyn Write, caches: &[CachedVenv]) -> io::Result<()> {
        // Compute column widths from the data + headers.
        let mut w_repo = "REPO".len();
        let mut w_size = "SIZE".len();
        let mut w_last = "LAST USED".len();

        let now = Utc::now();
        let rows: Vec<(String, String, String)> = caches
            .iter()
            .map(|c| {
                let repo = c.meta.repo_path.display().to_string();
                let size = format_size(c.size_bytes, BINARY);
                let last = human_ago(now, c.meta.last_used_at);
                w_repo = w_repo.max(repo.len());
                w_size = w_size.max(size.len());
                w_last = w_last.max(last.len());
                (repo, size, last)
            })
            .collect();

        writeln!(
            out,
            "{:<w_repo$}  {:>w_size$}  {:<w_last$}",
            "REPO",
            "SIZE",
            "LAST USED",
            w_repo = w_repo,
            w_size = w_size,
            w_last = w_last,
        )?;
        for (repo, size, last) in rows {
            writeln!(
                out,
                "{:<w_repo$}  {:>w_size$}  {:<w_last$}",
                repo,
                size,
                last,
                w_repo = w_repo,
                w_size = w_size,
                w_last = w_last,
            )?;
        }
        Ok(())
    }

    /// Human-friendly relative time. Coarse buckets are fine — the column
    /// exists to let users spot ancient entries at a glance.
    pub fn human_ago(now: DateTime<Utc>, then: DateTime<Utc>) -> String {
        let delta = now.signed_duration_since(then);
        let secs = delta.num_seconds();
        if secs < 60 {
            "just now".into()
        } else if secs < 3600 {
            format!("{}m ago", delta.num_minutes())
        } else if secs < 86_400 {
            format!("{}h ago", delta.num_hours())
        } else if delta.num_days() < 30 {
            format!("{}d ago", delta.num_days())
        } else if delta.num_days() < 365 {
            format!("{}mo ago", delta.num_days() / 30)
        } else {
            format!("{}y ago", delta.num_days() / 365)
        }
    }
    ```

- [ ] **Step 5.3: Wire dispatch into `src/bin/toolr/main.rs`**

    Add to the `match` arm:

    ```rust
    mod self_cmd;

    // inside dispatch:
    Some(TopLevel::Self_(args)) => match args.command {
        SelfCommand::Cache(cache_args) => {
            self_cmd::cache::run(cache_args.command)?;
        }
        // ... other self subcommands ...
    },
    ```

    If the surrounding dispatch shape differs (it will once Plan 2 ships),
    fit `self_cmd::cache::run(...)` into whatever the standard pattern is at
    integration time.

- [ ] **Step 5.4: Add an integration test**

    Create `tests/self_cache_list.rs`:

    ```rust
    //! Integration tests for `toolr self cache list`.

    use std::fs;
    use std::path::Path;

    use assert_cmd::Command;
    use chrono::Utc;
    use tempfile::TempDir;

    fn write_entry(cache_root: &Path, key: &str, repo_path: &str, bytes: usize) {
        let cache_dir = cache_root.join(key);
        fs::create_dir_all(cache_dir.join("venv")).unwrap();
        fs::write(cache_dir.join("venv/blob.bin"), vec![0u8; bytes]).unwrap();

        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let json = format!(
            r#"{{
              "schema_version": 1,
              "repo_path": "{repo_path}",
              "toolr_version": "1.0.0",
              "python_version": "3.13.1",
              "created_at": "{now}",
              "last_used_at": "{now}"
            }}"#
        );
        fs::write(cache_dir.join("meta.json"), json).unwrap();
    }

    #[test]
    fn list_reports_no_caches_when_empty() {
        let tmp = TempDir::new().unwrap();
        let mut cmd = Command::cargo_bin("toolr").unwrap();
        cmd.env("XDG_CACHE_HOME", tmp.path())
            .env_remove("HOME")
            .args(["self", "cache", "list"]);
        cmd.assert()
            .success()
            .stdout(predicates::str::contains("no cached virtualenvs"));
    }

    #[test]
    fn list_renders_entries_with_size_and_last_used() {
        let tmp = TempDir::new().unwrap();
        let cache_root = tmp.path().join("toolr");
        fs::create_dir_all(&cache_root).unwrap();
        write_entry(&cache_root, "key-a", "/repo/a", 4096);

        let mut cmd = Command::cargo_bin("toolr").unwrap();
        cmd.env("XDG_CACHE_HOME", tmp.path())
            .env_remove("HOME")
            .args(["self", "cache", "list"]);

        cmd.assert()
            .success()
            .stdout(predicates::str::contains("/repo/a"))
            .stdout(predicates::str::contains("REPO"))
            .stdout(predicates::str::contains("SIZE"))
            .stdout(predicates::str::contains("LAST USED"));
    }
    ```

    Add `predicates = "3"` to `[dev-dependencies]` in `Cargo.toml` if not
    already present (it's a transitive of `assert_cmd` but the explicit pin
    insulates these tests from a version change there).

- [ ] **Step 5.5: Run the tests**

    ```bash
    cargo test --lib cache::
    cargo test --test self_cache_list
    ```

    Expected: all green.

- [ ] **Step 5.6: Commit**

    ```bash
    git add Cargo.toml src/bin/toolr/ tests/self_cache_list.rs
    git commit -m "feat(cli): Add toolr self cache list with tabular output"
    ```

---

## Task 6: Detect orphan and stale entries

Pure logic, no deletion yet. Given an enumerated `Vec<CachedVenv>` and a
threshold, classify entries into `keep`, `orphan`, and `stale`. Task 7 wires
this into actual `prune` deletion.

**Files:**

- Create: `src/cache/classify.rs`
- Modify: `src/cache/mod.rs`
- Modify: `src/cache/tests.rs`
- [ ] **Step 6.1: Write the failing tests**

    Append to `src/cache/tests.rs`:

    ```rust
    use super::classify::{classify_entries, Classification, PruneReason};
    use chrono::Duration as ChronoDuration;

    fn now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 11, 12, 0, 0).unwrap()
    }

    fn entry_at(repo: &str, last_used: chrono::DateTime<Utc>) -> CachedVenv {
        CachedVenv {
            repo_key: "k".into(),
            cache_dir: std::path::PathBuf::from(format!("/cache/{repo}")),
            meta: Meta {
                schema_version: SCHEMA_VERSION,
                repo_path: std::path::PathBuf::from(repo),
                toolr_version: "1.0.0".into(),
                python_version: "3.13.1".into(),
                created_at: last_used,
                last_used_at: last_used,
            },
            size_bytes: 1024,
            is_orphan: false,
        }
    }

    #[test]
    fn classify_marks_missing_repo_path_as_orphan() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist");
        let mut entry = entry_at("/x", now());
        entry.meta.repo_path = missing;
        let result = classify_entries(vec![entry], now(), 30);
        assert_eq!(result.orphan.len(), 1);
        assert_eq!(result.orphan[0].reason, PruneReason::Orphan);
        assert!(result.stale.is_empty());
        assert!(result.keep.is_empty());
    }

    #[test]
    fn classify_marks_old_last_used_as_stale() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("real-repo");
        std::fs::create_dir_all(&repo).unwrap();
        let mut entry = entry_at("ignored", now() - ChronoDuration::days(45));
        entry.meta.repo_path = repo;
        let result = classify_entries(vec![entry], now(), 30);
        assert_eq!(result.stale.len(), 1);
        assert_eq!(result.stale[0].reason, PruneReason::Stale);
        assert!(result.orphan.is_empty());
    }

    #[test]
    fn classify_keeps_recently_used_existing_repos() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("real-repo");
        std::fs::create_dir_all(&repo).unwrap();
        let mut entry = entry_at("ignored", now() - ChronoDuration::days(3));
        entry.meta.repo_path = repo;
        let result = classify_entries(vec![entry], now(), 30);
        assert_eq!(result.keep.len(), 1);
        assert!(result.orphan.is_empty());
        assert!(result.stale.is_empty());
    }

    #[test]
    fn classify_prefers_orphan_over_stale_when_both_apply() {
        // A repo that no longer exists is always reported as orphan, even
        // if it also matches the staleness threshold.
        let mut entry = entry_at("/no/such/repo", now() - ChronoDuration::days(90));
        entry.meta.repo_path = std::path::PathBuf::from("/no/such/repo");
        let result = classify_entries(vec![entry], now(), 30);
        assert_eq!(result.orphan.len(), 1);
        assert!(result.stale.is_empty());
    }
    ```

- [ ] **Step 6.2: Run and verify the tests FAIL**

    ```bash
    cargo test --lib cache::tests::classify
    ```

    Expected: compile error.

- [ ] **Step 6.3: Create `src/cache/classify.rs`**

    ```rust
    //! Sort enumerated cache entries into keep / orphan / stale.

    use chrono::{DateTime, Duration as ChronoDuration, Utc};

    use super::enumerate::CachedVenv;

    /// Why a single entry is up for pruning.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PruneReason {
        /// `meta.repo_path` is not a directory anymore.
        Orphan,
        /// `meta.last_used_at` is older than the staleness threshold.
        Stale,
    }

    /// One entry plus the reason it was selected.
    #[derive(Debug, Clone)]
    pub struct Candidate {
        pub entry: CachedVenv,
        pub reason: PruneReason,
    }

    /// Bucketed classification result.
    #[derive(Debug, Default)]
    pub struct Classification {
        pub keep: Vec<CachedVenv>,
        pub orphan: Vec<Candidate>,
        pub stale: Vec<Candidate>,
    }

    /// Decide what to do with each entry. `stale_after_days` is the
    /// configurable threshold (default 30). Orphan beats stale: an entry
    /// whose repo no longer exists is always reported as orphan.
    pub fn classify_entries(
        entries: Vec<CachedVenv>,
        now: DateTime<Utc>,
        stale_after_days: u32,
    ) -> Classification {
        let threshold = ChronoDuration::days(stale_after_days as i64);
        let mut result = Classification::default();
        for entry in entries {
            if !entry.meta.repo_path.is_dir() {
                let mut e = entry;
                e.is_orphan = true;
                result.orphan.push(Candidate {
                    entry: e,
                    reason: PruneReason::Orphan,
                });
                continue;
            }
            let age = now.signed_duration_since(entry.meta.last_used_at);
            if age >= threshold {
                result.stale.push(Candidate {
                    entry,
                    reason: PruneReason::Stale,
                });
            } else {
                result.keep.push(entry);
            }
        }
        result
    }
    ```

- [ ] **Step 6.4: Re-export and run tests**

    Update `src/cache/mod.rs`:

    ```rust
    pub mod classify;
    pub mod enumerate;
    pub mod init;
    pub mod meta;
    pub mod touch;

    pub use classify::{classify_entries, Candidate, Classification, PruneReason};
    pub use enumerate::{dir_size_bytes, enumerate_caches, CachedVenv};
    pub use init::write_meta_for_new_venv;
    pub use meta::{Meta, MetaError, SCHEMA_VERSION};
    pub use touch::touch_last_used;

    #[cfg(test)]
    mod tests;
    ```

    Then:

    ```bash
    cargo test --lib cache::
    ```

    Expected: 16 tests passing.

- [ ] **Step 6.5: Commit**

    ```bash
    git add src/cache/
    git commit -m "feat(cache): Classify cache entries as keep, orphan, or stale"
    ```

---

## Task 7: `toolr self cache prune` (+ `--all`)

Wire the deletion. Default behaviour: delete orphans + stale entries. With
`--all`: nuke everything (after a confirmation that can be bypassed with
`--yes`). `--dry-run` from Task 5's `PruneArgs` reports what would be
deleted without touching disk.

**Files:**

- Modify: `src/bin/toolr/cli.rs`
- Modify: `src/bin/toolr/self_cmd/cache.rs`
- Create: `tests/self_cache_prune.rs`
- [ ] **Step 7.1: Extend `PruneArgs` with `--yes`**

    In `src/bin/toolr/cli.rs`, add to `PruneArgs`:

    ```rust
    /// Skip the interactive confirmation when used with --all.
    #[arg(long, short = 'y')]
    pub yes: bool,
    ```

- [ ] **Step 7.2: Replace the stub `run_prune` in `src/bin/toolr/self_cmd/cache.rs`**

    ```rust
    use std::io::IsTerminal;

    use _rust_utils::cache::{classify_entries, Candidate, Classification, PruneReason};

    pub fn run_prune(cache_root: &Path, args: &PruneArgs, out: &mut dyn Write) -> Result<()> {
        let mut entries = enumerate_caches(cache_root)?;
        // Sort by repo path for deterministic output.
        entries.sort_by(|a, b| a.meta.repo_path.cmp(&b.meta.repo_path));

        if args.all {
            return prune_all(cache_root, entries, args, out);
        }
        let classification = classify_entries(entries, Utc::now(), args.stale_after_days);
        prune_classified(classification, args, out)
    }

    fn prune_all(
        cache_root: &Path,
        entries: Vec<CachedVenv>,
        args: &PruneArgs,
        out: &mut dyn Write,
    ) -> Result<()> {
        if entries.is_empty() {
            writeln!(out, "toolr: no cache entries to remove under {}", cache_root.display())?;
            return Ok(());
        }
        if !args.yes && !args.dry_run && !confirm_destroy_all(&entries)? {
            writeln!(out, "toolr: aborted, nothing removed")?;
            return Ok(());
        }

        let mut total_bytes: u64 = 0;
        for e in &entries {
            total_bytes = total_bytes.saturating_add(e.size_bytes);
            if args.dry_run {
                writeln!(out, "DRY-RUN would remove {} ({})",
                    e.cache_dir.display(),
                    humansize::format_size(e.size_bytes, humansize::BINARY))?;
            } else {
                remove_entry(&e.cache_dir, out)?;
            }
        }
        let action = if args.dry_run { "would free" } else { "freed" };
        writeln!(
            out,
            "toolr: {action} {} across {} entr{}",
            humansize::format_size(total_bytes, humansize::BINARY),
            entries.len(),
            if entries.len() == 1 { "y" } else { "ies" },
        )?;
        Ok(())
    }

    fn prune_classified(
        classification: Classification,
        args: &PruneArgs,
        out: &mut dyn Write,
    ) -> Result<()> {
        let candidates: Vec<Candidate> = classification
            .orphan
            .into_iter()
            .chain(classification.stale.into_iter())
            .collect();
        if candidates.is_empty() {
            writeln!(out, "toolr: nothing to prune")?;
            return Ok(());
        }

        let mut total_bytes: u64 = 0;
        for c in &candidates {
            total_bytes = total_bytes.saturating_add(c.entry.size_bytes);
            let tag = match c.reason {
                PruneReason::Orphan => "ORPHAN",
                PruneReason::Stale => "STALE",
            };
            if args.dry_run {
                writeln!(
                    out,
                    "DRY-RUN {tag:<7} {} ({})",
                    c.entry.cache_dir.display(),
                    humansize::format_size(c.entry.size_bytes, humansize::BINARY),
                )?;
            } else {
                writeln!(
                    out,
                    "{tag:<7} removing {} ({})",
                    c.entry.cache_dir.display(),
                    humansize::format_size(c.entry.size_bytes, humansize::BINARY),
                )?;
                remove_entry(&c.entry.cache_dir, out)?;
            }
        }
        let action = if args.dry_run { "would free" } else { "freed" };
        writeln!(
            out,
            "toolr: {action} {} across {} entr{}",
            humansize::format_size(total_bytes, humansize::BINARY),
            candidates.len(),
            if candidates.len() == 1 { "y" } else { "ies" },
        )?;
        Ok(())
    }

    fn remove_entry(cache_dir: &Path, out: &mut dyn Write) -> Result<()> {
        match std::fs::remove_dir_all(cache_dir) {
            Ok(()) => Ok(()),
            Err(e) => {
                writeln!(out, "toolr: warning: failed to remove {}: {e}", cache_dir.display())?;
                Ok(())
            }
        }
    }

    fn confirm_destroy_all(entries: &[CachedVenv]) -> Result<bool> {
        if !io::stdin().is_terminal() {
            // Non-interactive shells must use --yes.
            anyhow::bail!(
                "refusing to wipe {} cache entries without --yes (stdin is not a terminal)",
                entries.len(),
            );
        }
        eprint!(
            "toolr: about to remove {} cache entries. continue? [y/N] ",
            entries.len(),
        );
        io::stderr().flush().ok();
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        Ok(matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
    }
    ```

- [ ] **Step 7.3: Add integration tests**

    Create `tests/self_cache_prune.rs`:

    ```rust
    //! Integration tests for `toolr self cache prune` and `--all`.

    use std::fs;
    use std::path::Path;

    use assert_cmd::Command;
    use chrono::{Duration, Utc};
    use tempfile::TempDir;

    /// Helper that writes a cache entry with a configurable last_used_at.
    fn write_entry(
        cache_root: &Path,
        key: &str,
        repo_path: &Path,
        last_used_at: chrono::DateTime<Utc>,
    ) {
        let cache_dir = cache_root.join(key);
        fs::create_dir_all(cache_dir.join("venv")).unwrap();
        fs::write(cache_dir.join("venv/blob.bin"), vec![0u8; 256]).unwrap();
        let json = format!(
            r#"{{
              "schema_version": 1,
              "repo_path": "{}",
              "toolr_version": "1.0.0",
              "python_version": "3.13.1",
              "created_at": "{}",
              "last_used_at": "{}"
            }}"#,
            repo_path.display(),
            last_used_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            last_used_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        );
        fs::write(cache_dir.join("meta.json"), json).unwrap();
    }

    #[test]
    fn prune_removes_orphan_entries() {
        let tmp = TempDir::new().unwrap();
        let cache_root = tmp.path().join("toolr");
        fs::create_dir_all(&cache_root).unwrap();

        // Orphan: repo path does not exist.
        write_entry(
            &cache_root,
            "orphan-key",
            Path::new("/definitely/missing/repo"),
            Utc::now(),
        );
        // Live: repo path is a real dir, recently used.
        let live_repo = tmp.path().join("live-repo");
        fs::create_dir_all(&live_repo).unwrap();
        write_entry(&cache_root, "live-key", &live_repo, Utc::now());

        let mut cmd = Command::cargo_bin("toolr").unwrap();
        cmd.env("XDG_CACHE_HOME", tmp.path())
            .env_remove("HOME")
            .args(["self", "cache", "prune"]);
        cmd.assert()
            .success()
            .stdout(predicates::str::contains("ORPHAN"));

        assert!(!cache_root.join("orphan-key").exists());
        assert!(cache_root.join("live-key").exists());
    }

    #[test]
    fn prune_removes_stale_entries() {
        let tmp = TempDir::new().unwrap();
        let cache_root = tmp.path().join("toolr");
        fs::create_dir_all(&cache_root).unwrap();

        let live_repo = tmp.path().join("live-repo");
        fs::create_dir_all(&live_repo).unwrap();
        // Stale: last used 60 days ago, default threshold 30.
        write_entry(&cache_root, "stale-key", &live_repo, Utc::now() - Duration::days(60));
        // Fresh: last used 2 days ago.
        write_entry(&cache_root, "fresh-key", &live_repo, Utc::now() - Duration::days(2));

        let mut cmd = Command::cargo_bin("toolr").unwrap();
        cmd.env("XDG_CACHE_HOME", tmp.path())
            .env_remove("HOME")
            .args(["self", "cache", "prune"]);
        cmd.assert().success().stdout(predicates::str::contains("STALE"));

        assert!(!cache_root.join("stale-key").exists());
        assert!(cache_root.join("fresh-key").exists());
    }

    #[test]
    fn prune_dry_run_leaves_disk_untouched() {
        let tmp = TempDir::new().unwrap();
        let cache_root = tmp.path().join("toolr");
        fs::create_dir_all(&cache_root).unwrap();
        write_entry(
            &cache_root,
            "orphan-key",
            Path::new("/definitely/missing/repo"),
            Utc::now(),
        );

        let mut cmd = Command::cargo_bin("toolr").unwrap();
        cmd.env("XDG_CACHE_HOME", tmp.path())
            .env_remove("HOME")
            .args(["self", "cache", "prune", "--dry-run"]);
        cmd.assert()
            .success()
            .stdout(predicates::str::contains("DRY-RUN"));

        assert!(cache_root.join("orphan-key").exists());
    }

    #[test]
    fn prune_all_with_yes_nukes_everything() {
        let tmp = TempDir::new().unwrap();
        let cache_root = tmp.path().join("toolr");
        fs::create_dir_all(&cache_root).unwrap();
        let live_repo = tmp.path().join("live-repo");
        fs::create_dir_all(&live_repo).unwrap();

        write_entry(&cache_root, "a", &live_repo, Utc::now());
        write_entry(&cache_root, "b", &live_repo, Utc::now());

        let mut cmd = Command::cargo_bin("toolr").unwrap();
        cmd.env("XDG_CACHE_HOME", tmp.path())
            .env_remove("HOME")
            .args(["self", "cache", "prune", "--all", "--yes"]);
        cmd.assert().success();

        assert!(!cache_root.join("a").exists());
        assert!(!cache_root.join("b").exists());
    }

    #[test]
    fn prune_all_refuses_without_yes_when_non_interactive() {
        let tmp = TempDir::new().unwrap();
        let cache_root = tmp.path().join("toolr");
        fs::create_dir_all(&cache_root).unwrap();
        let live_repo = tmp.path().join("live-repo");
        fs::create_dir_all(&live_repo).unwrap();
        write_entry(&cache_root, "a", &live_repo, Utc::now());

        let mut cmd = Command::cargo_bin("toolr").unwrap();
        cmd.env("XDG_CACHE_HOME", tmp.path())
            .env_remove("HOME")
            .args(["self", "cache", "prune", "--all"]);
        cmd.assert().failure();

        assert!(cache_root.join("a").exists());
    }
    ```

- [ ] **Step 7.4: Run the new tests**

    ```bash
    cargo test --test self_cache_prune
    ```

    Expected: all five tests pass.

- [ ] **Step 7.5: Commit**

    ```bash
    git add src/bin/toolr/cli.rs src/bin/toolr/self_cmd/ tests/self_cache_prune.rs
    git commit -m "feat(cli): Add toolr self cache prune with --all and --dry-run"
    ```

---

## Task 8: Passive size-hint on every invocation

On any toolr invocation (regardless of whether it spawns Python), if the
cache exceeds a configurable size threshold (default 1 GiB) or has more than
a configurable orphan count (default 10), emit a one-line suggestion to
stderr. No automatic deletion. Suppress the hint when the user is already
inside a `toolr self cache ...` subcommand (they're aware) and when the
env-var `TOOLR_NO_CACHE_HINT=1` is set.

**Files:**

- Create: `src/cache/hint.rs`
- Modify: `src/cache/mod.rs`
- Modify: `src/bin/toolr/main.rs`
- Modify: `src/cache/tests.rs`
- [ ] **Step 8.1: Write the failing tests**

    Append to `src/cache/tests.rs`:

    ```rust
    use super::hint::{compute_hint, HintConfig};

    #[test]
    fn hint_is_none_when_cache_is_small_and_clean() {
        let tmp = TempDir::new().unwrap();
        let live_repo = tmp.path().join("live");
        std::fs::create_dir_all(&live_repo).unwrap();
        let cache_root = tmp.path().join("toolr-cache");
        std::fs::create_dir_all(&cache_root).unwrap();
        make_entry(&cache_root, "a", live_repo.to_str().unwrap(), now(), 1024);

        let hint = compute_hint(&cache_root, &HintConfig::default(), now()).unwrap();
        assert!(hint.is_none(), "expected no hint, got {hint:?}");
    }

    #[test]
    fn hint_fires_when_total_size_exceeds_threshold() {
        let tmp = TempDir::new().unwrap();
        let live_repo = tmp.path().join("live");
        std::fs::create_dir_all(&live_repo).unwrap();
        let cache_root = tmp.path().join("toolr-cache");
        std::fs::create_dir_all(&cache_root).unwrap();
        make_entry(&cache_root, "a", live_repo.to_str().unwrap(), now(), 4096);

        let cfg = HintConfig {
            size_threshold_bytes: 1024,
            orphan_threshold: 10,
        };
        let hint = compute_hint(&cache_root, &cfg, now()).unwrap();
        assert!(hint.is_some());
        let s = hint.unwrap();
        assert!(s.contains("Run `toolr self cache prune`"), "got: {s}");
    }

    #[test]
    fn hint_fires_when_orphan_count_exceeds_threshold() {
        let tmp = TempDir::new().unwrap();
        let cache_root = tmp.path().join("toolr-cache");
        std::fs::create_dir_all(&cache_root).unwrap();
        // 3 orphans (all pointing at nonexistent paths).
        for key in &["a", "b", "c"] {
            make_entry(&cache_root, key, "/missing", now(), 32);
        }

        let cfg = HintConfig {
            size_threshold_bytes: 100 * 1024 * 1024 * 1024, // effectively off
            orphan_threshold: 2,
        };
        let hint = compute_hint(&cache_root, &cfg, now()).unwrap();
        assert!(hint.is_some());
        assert!(hint.as_ref().unwrap().contains("orphan"));
    }

    #[test]
    fn hint_is_none_when_cache_root_is_missing() {
        let tmp = TempDir::new().unwrap();
        let hint = compute_hint(
            &tmp.path().join("no-such-dir"),
            &HintConfig::default(),
            now(),
        )
        .unwrap();
        assert!(hint.is_none());
    }
    ```

- [ ] **Step 8.2: Run and verify the tests FAIL**

    ```bash
    cargo test --lib cache::tests::hint
    ```

    Expected: compile error.

- [ ] **Step 8.3: Create `src/cache/hint.rs`**

    ```rust
    //! Compute the passive "your cache is big, consider pruning" message.

    use std::path::Path;

    use anyhow::Result;
    use chrono::{DateTime, Utc};
    use humansize::{format_size, BINARY};

    use super::classify::{classify_entries, Classification};
    use super::enumerate::enumerate_caches;

    /// Tunables for hint emission.
    #[derive(Debug, Clone, Copy)]
    pub struct HintConfig {
        /// Aggregate cache size threshold in bytes. Default 1 GiB.
        pub size_threshold_bytes: u64,
        /// Orphan-entry count threshold. Default 10.
        pub orphan_threshold: usize,
    }

    impl Default for HintConfig {
        fn default() -> Self {
            Self {
                size_threshold_bytes: 1024 * 1024 * 1024,
                orphan_threshold: 10,
            }
        }
    }

    /// Inspect the cache and return a single-line message if either
    /// threshold is exceeded. Returns `Ok(None)` when nothing should be
    /// printed. Errors propagate, but the binary should fall back to
    /// `None` on any error so that hint computation never blocks a real
    /// command.
    pub fn compute_hint(
        cache_root: &Path,
        config: &HintConfig,
        now: DateTime<Utc>,
    ) -> Result<Option<String>> {
        let entries = enumerate_caches(cache_root)?;
        if entries.is_empty() {
            return Ok(None);
        }

        // Sum total size up front.
        let total_bytes: u64 = entries.iter().map(|e| e.size_bytes).sum();

        // Classify so we can count orphans without changing semantics later.
        let Classification {
            keep: _,
            orphan,
            stale,
        } = classify_entries(entries, now, 30);

        let prune_target_count = orphan.len() + stale.len();
        let oversized = total_bytes >= config.size_threshold_bytes;
        let too_many_orphans = orphan.len() > config.orphan_threshold;

        if !oversized && !too_many_orphans {
            return Ok(None);
        }

        let pretty_size = format_size(total_bytes, BINARY);
        let msg = if oversized && too_many_orphans {
            format!(
                "toolr: cache has {} orphan entries (~{}). Run `toolr self cache prune` to clean up.",
                orphan.len(),
                pretty_size,
            )
        } else if oversized {
            format!(
                "toolr: cache has {} entr{} (~{}). Run `toolr self cache prune` to clean up.",
                prune_target_count.max(1),
                if prune_target_count.max(1) == 1 { "y" } else { "ies" },
                pretty_size,
            )
        } else {
            format!(
                "toolr: cache has {} orphan entries (~{}). Run `toolr self cache prune` to clean up.",
                orphan.len(),
                pretty_size,
            )
        };
        Ok(Some(msg))
    }
    ```

- [ ] **Step 8.4: Re-export and run the unit tests**

    Update `src/cache/mod.rs`:

    ```rust
    pub mod classify;
    pub mod enumerate;
    pub mod hint;
    pub mod init;
    pub mod meta;
    pub mod touch;

    pub use classify::{classify_entries, Candidate, Classification, PruneReason};
    pub use enumerate::{dir_size_bytes, enumerate_caches, CachedVenv};
    pub use hint::{compute_hint, HintConfig};
    pub use init::write_meta_for_new_venv;
    pub use meta::{Meta, MetaError, SCHEMA_VERSION};
    pub use touch::touch_last_used;

    #[cfg(test)]
    mod tests;
    ```

    Then:

    ```bash
    cargo test --lib cache::
    ```

    Expected: 20 tests passing.

- [ ] **Step 8.5: Wire emission into the binary entry point**

    In `src/bin/toolr/main.rs`, after CLI parsing but before subcommand
    dispatch, compute and emit the hint. Suppress when the running command
    is itself `toolr self cache ...` (to avoid printing the hint right next
    to the user's explicit `list` output).

    ```rust
    use _rust_utils::cache::{compute_hint, HintConfig};

    fn maybe_emit_cache_hint(cli: &Cli) {
        if std::env::var_os("TOOLR_NO_CACHE_HINT").is_some() {
            return;
        }
        if cli.is_self_cache_subcommand() {
            return;
        }
        let Ok(cache_root) = self_cmd::cache::resolve_cache_root() else {
            return;
        };
        let cfg = HintConfig::default();
        match compute_hint(&cache_root, &cfg, chrono::Utc::now()) {
            Ok(Some(msg)) => {
                eprintln!("{msg}");
            }
            _ => {}
        }
    }
    ```

    Add a small accessor on `Cli` so the hint code doesn't need to inspect
    the enum directly:

    ```rust
    impl Cli {
        pub fn is_self_cache_subcommand(&self) -> bool {
            matches!(
                self.command,
                Some(TopLevel::Self_(ref a)) if matches!(a.command, SelfCommand::Cache(_))
            )
        }
    }
    ```

    Call `maybe_emit_cache_hint(&cli)` once at the top of `main()` (or the
    binary's existing dispatch wrapper) so every invocation passes through
    it.

- [ ] **Step 8.6: Integration test for emission**

    Add to `tests/self_cache_prune.rs` (same fixture style):

    ```rust
    #[test]
    fn passive_hint_appears_when_orphan_count_is_high() {
        let tmp = TempDir::new().unwrap();
        let cache_root = tmp.path().join("toolr");
        fs::create_dir_all(&cache_root).unwrap();
        for i in 0..15 {
            write_entry(
                &cache_root,
                &format!("orphan-{i}"),
                Path::new("/definitely/missing/repo"),
                Utc::now(),
            );
        }

        // Run a command that does NOT itself sit under `self cache` so the
        // suppression rule doesn't fire — `toolr --version` is universally
        // available.
        let mut cmd = Command::cargo_bin("toolr").unwrap();
        cmd.env("XDG_CACHE_HOME", tmp.path())
            .env_remove("HOME")
            .env_remove("TOOLR_NO_CACHE_HINT")
            .args(["--version"]);
        cmd.assert()
            .success()
            .stderr(predicates::str::contains("toolr self cache prune"));
    }

    #[test]
    fn passive_hint_is_suppressed_by_env_var() {
        let tmp = TempDir::new().unwrap();
        let cache_root = tmp.path().join("toolr");
        fs::create_dir_all(&cache_root).unwrap();
        for i in 0..15 {
            write_entry(
                &cache_root,
                &format!("orphan-{i}"),
                Path::new("/definitely/missing/repo"),
                Utc::now(),
            );
        }
        let mut cmd = Command::cargo_bin("toolr").unwrap();
        cmd.env("XDG_CACHE_HOME", tmp.path())
            .env_remove("HOME")
            .env("TOOLR_NO_CACHE_HINT", "1")
            .args(["--version"]);
        cmd.assert()
            .success()
            .stderr(predicates::str::contains("toolr self cache prune").not());
    }
    ```

- [ ] **Step 8.7: Run the integration tests**

    ```bash
    cargo test --test self_cache_prune
    ```

    Expected: all tests pass.

- [ ] **Step 8.8: Commit**

    ```bash
    git add src/bin/toolr/ src/cache/ tests/self_cache_prune.rs
    git commit -m "feat(cache): Emit passive size-hint when cache exceeds thresholds"
    ```

---

## Task 9: End-to-end fixture-cache integration test

A single test that exercises the whole flow: a fixture cache directory with
a mix of orphan, stale, and fresh entries; `list` reports the expected rows;
`prune --dry-run` reports the right set; `prune` deletes them; `prune --all`
nukes the rest.

**Files:**

- Create: `tests/cache_fixtures.rs`
- [ ] **Step 9.1: Write the fixture-driven test**

    ```rust
    //! End-to-end exercise of the toolr self cache surface against a
    //! fixture cache root.

    use std::fs;
    use std::path::Path;

    use assert_cmd::Command;
    use chrono::{Duration, Utc};
    use tempfile::TempDir;

    fn write_entry(
        cache_root: &Path,
        key: &str,
        repo_path: &Path,
        last_used_at: chrono::DateTime<Utc>,
        bytes: usize,
    ) {
        let cache_dir = cache_root.join(key);
        fs::create_dir_all(cache_dir.join("venv")).unwrap();
        fs::write(cache_dir.join("venv/blob.bin"), vec![0u8; bytes]).unwrap();
        let stamp = last_used_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let json = format!(
            r#"{{
              "schema_version": 1,
              "repo_path": "{repo}",
              "toolr_version": "1.0.0",
              "python_version": "3.13.1",
              "created_at": "{stamp}",
              "last_used_at": "{stamp}"
            }}"#,
            repo = repo_path.display(),
            stamp = stamp,
        );
        fs::write(cache_dir.join("meta.json"), json).unwrap();
    }

    fn cmd_in(tmp: &Path, args: &[&str]) -> Command {
        let mut cmd = Command::cargo_bin("toolr").unwrap();
        cmd.env("XDG_CACHE_HOME", tmp)
            .env_remove("HOME")
            .env("TOOLR_NO_CACHE_HINT", "1")
            .args(args);
        cmd
    }

    #[test]
    fn end_to_end_list_then_prune_then_prune_all() {
        let tmp = TempDir::new().unwrap();
        let cache_root = tmp.path().join("toolr");
        fs::create_dir_all(&cache_root).unwrap();

        let live_repo = tmp.path().join("live-repo");
        fs::create_dir_all(&live_repo).unwrap();

        // 1 orphan, 1 stale, 1 fresh.
        write_entry(&cache_root, "orphan", Path::new("/no/such/repo"), Utc::now(), 256);
        write_entry(&cache_root, "stale", &live_repo, Utc::now() - Duration::days(60), 256);
        write_entry(&cache_root, "fresh", &live_repo, Utc::now() - Duration::days(1), 256);

        // --- list ---
        cmd_in(tmp.path(), &["self", "cache", "list"])
            .assert()
            .success()
            .stdout(predicates::str::contains("/no/such/repo"))
            .stdout(predicates::str::contains(live_repo.to_string_lossy().to_string()));

        // --- prune --dry-run: reports orphan + stale, deletes nothing ---
        cmd_in(tmp.path(), &["self", "cache", "prune", "--dry-run"])
            .assert()
            .success()
            .stdout(predicates::str::contains("DRY-RUN"));
        assert!(cache_root.join("orphan").exists());
        assert!(cache_root.join("stale").exists());
        assert!(cache_root.join("fresh").exists());

        // --- prune: removes orphan + stale, keeps fresh ---
        cmd_in(tmp.path(), &["self", "cache", "prune"]).assert().success();
        assert!(!cache_root.join("orphan").exists());
        assert!(!cache_root.join("stale").exists());
        assert!(cache_root.join("fresh").exists());

        // --- prune --all --yes: removes the last entry ---
        cmd_in(tmp.path(), &["self", "cache", "prune", "--all", "--yes"])
            .assert()
            .success();
        assert!(!cache_root.join("fresh").exists());
    }
    ```

- [ ] **Step 9.2: Run the test**

    ```bash
    cargo test --test cache_fixtures
    ```

    Expected: pass.

- [ ] **Step 9.3: Commit**

    ```bash
    git add tests/cache_fixtures.rs
    git commit -m "test(cache): End-to-end fixture run of list, prune, prune --all"
    ```

---

## Task 10: Update the roadmap

Mark Plan 8 as Done once everything above is merged.

**Files:**

- Modify: `specs/rust-front-end/01-roadmap.md`

- [ ] **Step 10.1: Update the Plan 8 entry**

    Change `### Plan 8: Cache management` block:

    ```markdown
    ### Plan 8: Cache management

    - **Status:** ✅ Done
    - **Plan doc:** [09-plan-8-cache.md](./09-plan-8-cache.md)
    - **Depends on:** Plan 3
    - **Unblocks:** —
    - **Produces:**
        - …(unchanged)…
    ```

- [ ] **Step 10.2: Commit**

    ```bash
    git add specs/rust-front-end/01-roadmap.md
    git commit -m "docs(roadmap): Mark Plan 8 as done"
    ```

---

## Done criteria

Plan 8 is complete when:

- `cargo test` passes for all unit and integration tests added by this plan.
- Creating a new venv (Plan 3 path) drops a `meta.json` sidecar at
  `$XDG_CACHE_HOME/toolr/<repo-key>/meta.json` with `repo_path`,
  `toolr_version`, `python_version`, `created_at`, `last_used_at`.
- Every command that resolves to a cached venv updates `last_used_at`
  within the same invocation; failures to update never block the command.
- `toolr self cache list` produces a sorted, tabular report of all cache
  entries with repo path, human-readable size, and human-readable last-use
  age.
- `toolr self cache prune` removes orphans (repo missing) and stale
  entries (idle ≥ 30 days by default; configurable via `--stale-after-days`)
  and leaves fresh entries alone. `--dry-run` reports without deleting.
- `toolr self cache prune --all` deletes every cache entry. Interactive
  shells confirm; non-interactive shells require `--yes`.
- A passive single-line hint is emitted on stderr when the total cache size
  exceeds 1 GiB or orphan count exceeds 10 (both configurable in code). The
  hint is suppressed by `TOOLR_NO_CACHE_HINT=1` and when the running
  command is itself `toolr self cache ...`.
- The roadmap status table reflects Plan 8 as `✅ Done`.

## Open questions (for the implementer)

These are deliberately deferred — surface to the spec author if any block
progress, otherwise resolve in line:

1. **Cache root resolution duplication.** Task 5 introduces
   `resolve_cache_root()` inside the binary's `self_cmd::cache` module.
   Plan 3 is also likely to introduce a cache-root resolver (for venv
   placement). The two implementations must agree on the
   `XDG_CACHE_HOME` → `~/.cache/toolr/` precedence. Consolidating them into
   a single `_rust_utils::paths` module is the obvious cleanup; out of scope
   for Plan 8 unless Plan 3 has not yet landed, in which case Plan 8 should
   own the canonical implementation and Plan 3 imports it.
2. **Configurable hint thresholds.** The 1 GiB / 10 orphan thresholds are
   hard-coded in `HintConfig::default()`. Should they be user-configurable
   via `~/.config/toolr/config.toml` or env vars? `TOOLR_CACHE_SIZE_HINT_MB`
   and `TOOLR_CACHE_ORPHAN_HINT` would be a low-friction first cut; a full
   config file is over-engineering for v1. Decide before the plan is
   merged.
3. **`last_used_at` cost on hot paths.** The plan describes a JSON
   re-write per invocation. For a < 1 KiB file that's well under 1 ms on
   any modern filesystem, but it's still a write per `toolr --help`. The
   spec text says "single mtime touch"; if performance becomes an issue,
   fall back to `filetime::set_file_mtime(&meta_path, FileTime::now())` on
   the warm path and reconcile the JSON's `last_used_at` lazily during
   `list`/`prune`. Document the chosen strategy in code comments.
4. **Concurrent invocations.** Two parallel `toolr ...` runs in the same
   repo could race on `meta.json` rewrites. The `tmp → rename` pattern in
   `Meta::write` keeps the file always-valid, and the only field that
   matters for races is `last_used_at` — losing the older of two
   simultaneous updates is harmless. If telemetry ever shows duplicate
   creation_at clobbering, switch to an advisory `flock` around the
   read-modify-write. Out of scope for v1.
5. **Behaviour when `XDG_CACHE_HOME` is set but unwritable.** Currently the
   binary would surface a generic I/O error. Should `toolr self cache list`
   degrade to "no caches found" instead, so that broken CI environments
   don't spam errors? Decide alongside the broader error-presentation
   pass.
