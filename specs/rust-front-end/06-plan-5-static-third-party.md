<!-- rumdl-disable MD046 MD076 -->

# Plan 5: Third-Party Static Manifest Convention + `toolr.build`

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Lint:** Plan docs nest fenced code inside list items for step-by-step
> structure. The `<!-- rumdl-disable MD046 MD076 -->` directive above turns
> off the code-block-style and list-item-spacing rules for this file only.

**Goal:** Allow third-party command packages to ship a static
`toolr-manifest.json` at the root of their installed Python package so that
toolr discovers their commands without any Python introspection. At the end of
this plan, packages that adopt the convention are merged into the static
manifest via a sub-millisecond glob over `site-packages/`, and package authors
have a turnkey way (`python -m toolr.build <pkg>`) to generate that manifest
from their existing `command_group` / `@group.command` declarations.

**Architecture:** Two halves, joined by a stable on-disk schema.

1. **Rust half (discovery + merge).** A new
   `_rust_utils::third_party` module globs
   `<tools-venv>/lib/python*/site-packages/*/toolr-manifest.json`,
   parses each match, validates the mandatory `toolr_schema_version` integer,
   applies in-process migrations to bring older fragments forward, and merges
   the resulting groups + commands into the static manifest produced by
   `_rust_utils::parser::build_static_manifest`. A migration framework
   (`v1 → v1` no-op for now) leaves room for future schema bumps without
   breaking previously-built fragments.
2. **Python half (build helper).** A new `python/toolr/build.py` module
   introspects an importable package's decorator registry and emits a
   schema-versioned manifest fragment. It is exposed both as a programmatic
   API (`from toolr.build import build_manifest`) and as a CLI
   (`python -m toolr.build <package>`). `--check` mode regenerates in memory
   and exits non-zero on drift, suitable for pre-commit hooks and CI gates.
3. **Rust CLI wrapper.** `toolr self build-manifest <package>` locates a
   Python interpreter (active venv → PATH → optional `--python` override) and
   shells out to `python -m toolr.build`. This is convenience for users who
   already type `toolr` more often than `python`; it adds no capability over
   the bare Python invocation.

**Tech stack:** Rust (existing crate dependencies: `glob`, `serde_json`,
`thiserror`, `walkdir`, `clap`). New Python module added under
`python/toolr/build.py` using only standard-library `argparse` and `json` plus
the package's existing `toolr._registry` machinery — no new Python
dependencies.

**Manifest fragment format (illustrative).** A third-party fragment is a
strict subset of the project manifest plus the version key. Example
`my_pkg/toolr-manifest.json`:

```json
{
  "toolr_schema_version": 1,
  "package": "my_pkg",
  "groups": [
    {"name": "deploy", "title": "Deploy commands", "description": ""}
  ],
  "commands": [
    {
      "name": "rollout",
      "group": "deploy",
      "module": "my_pkg.commands.deploy",
      "function": "rollout",
      "summary": "Roll out a new build.",
      "description": "",
      "arguments": [],
      "imports": []
    }
  ]
}
```

`origin` is **not** stored in the fragment — the Rust merger assigns
`Origin::Static` (or a future `Origin::ThirdParty` if we add it) when
incorporating each entry into the unified manifest.

**Reading order in this plan:** Tasks build on each other. Tasks 1–5 stand up
the Rust side end-to-end against fixtures. Tasks 6–9 build the Python
generator. Task 10 adds the Rust CLI wrapper. Task 11 wires the full
round-trip. Task 12 closes out the roadmap.

---

## Task 1: Define `ManifestFragment` model + glob helper

Introduce a Rust module dedicated to the third-party fragment format. Start
with the model and a pure glob function — no I/O parsing yet.

**Files:**

- Create: `src/third_party/mod.rs`
- Create: `src/third_party/model.rs`
- Create: `src/third_party/glob.rs`
- Modify: `src/lib.rs`
- Modify: `Cargo.toml`

- [ ] **Step 1.1: Add `glob` to `Cargo.toml`**

    Append to `[dependencies]`:

    ```toml
    glob = "0.3"
    ```

- [ ] **Step 1.2: Expose the module from `src/lib.rs`**

    Add alongside the existing module exports:

    ```rust
    pub mod third_party;
    ```

- [ ] **Step 1.3: Create `src/third_party/mod.rs`**

    ```rust
    //! Third-party static manifest fragment discovery, parsing, and merging.
    //!
    //! Packages ship a `toolr-manifest.json` at the root of their installed
    //! Python package directory. This module globs for those files, validates
    //! the mandatory `toolr_schema_version`, applies migrations, and merges
    //! the resulting fragments into the project's static manifest.

    pub mod glob;
    pub mod migrate;
    pub mod model;
    pub mod parse;

    pub use glob::glob_manifests;
    pub use migrate::migrate_to_current;
    pub use model::{
        ManifestFragment, FragmentCommand, FragmentGroup, FragmentArgument, FRAGMENT_SCHEMA_VERSION,
    };
    pub use parse::{parse_fragment, ThirdPartyError};

    #[cfg(test)]
    mod tests;
    ```

    Note: `migrate.rs` and `parse.rs` arrive in Tasks 2–3; this file is the
    expected final shape — referenced modules can be empty placeholders for
    now if needed to compile, or you can defer adding their `pub mod` lines
    until those tasks. The plan assumes the placeholder approach: create the
    files as empty stubs in this task so subsequent tasks only edit.

- [ ] **Step 1.4: Create stub `src/third_party/migrate.rs` and `src/third_party/parse.rs`**

    Empty for now — both arrive in later tasks. Stub content for each:

    ```rust
    //! Placeholder — populated in a later task.
    ```

- [ ] **Step 1.5: Create `src/third_party/model.rs`**

    ```rust
    //! Serde model for a third-party manifest fragment.
    //!
    //! Distinct from `crate::manifest::Manifest` because fragments lack
    //! `static_hash` / `dynamic_hash` / `origin` and instead carry the
    //! mandatory `toolr_schema_version` discriminator.

    use serde::{Deserialize, Serialize};

    /// Current fragment schema version. The Rust binary accepts fragments at
    /// version `<= FRAGMENT_SCHEMA_VERSION`, applying migrations as needed.
    /// Fragments at a higher version are rejected.
    pub const FRAGMENT_SCHEMA_VERSION: u32 = 1;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct ManifestFragment {
        pub toolr_schema_version: u32,
        /// The Python package name this fragment came from. Used for
        /// diagnostic messages and de-duplication.
        pub package: String,
        #[serde(default)]
        pub groups: Vec<FragmentGroup>,
        #[serde(default)]
        pub commands: Vec<FragmentCommand>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct FragmentGroup {
        pub name: String,
        pub title: String,
        #[serde(default)]
        pub description: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct FragmentCommand {
        pub name: String,
        pub group: String,
        pub module: String,
        pub function: String,
        #[serde(default)]
        pub summary: String,
        #[serde(default)]
        pub description: String,
        #[serde(default)]
        pub arguments: Vec<FragmentArgument>,
        #[serde(default)]
        pub imports: Vec<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct FragmentArgument {
        pub name: String,
        pub kind: crate::manifest::ArgumentKind,
        #[serde(default)]
        pub help: String,
        #[serde(default)]
        pub default: Option<String>,
        #[serde(default)]
        pub type_annotation: Option<String>,
        #[serde(default)]
        pub allowed_values: Vec<String>,
    }
    ```

- [ ] **Step 1.6: Create `src/third_party/glob.rs`**

    ```rust
    //! Glob `<tools-venv>/lib/python*/site-packages/*/toolr-manifest.json`.

    use std::path::{Path, PathBuf};

    use glob::{glob_with, MatchOptions};

    use super::parse::ThirdPartyError;

    /// Glob all third-party manifest fragments under `tools_venv`.
    ///
    /// Returns paths in deterministic (sorted) order for reproducibility.
    /// Errors only on filesystem-level glob failures; individual path
    /// parsing happens later in `parse_fragment`.
    pub fn glob_manifests(tools_venv: &Path) -> Result<Vec<PathBuf>, ThirdPartyError> {
        // `<tools-venv>/lib/python*/site-packages/*/toolr-manifest.json`
        let pattern = tools_venv
            .join("lib")
            .join("python*")
            .join("site-packages")
            .join("*")
            .join("toolr-manifest.json");
        let pattern_str = pattern
            .to_str()
            .ok_or_else(|| ThirdPartyError::NonUtf8Path(pattern.clone()))?;

        let opts = MatchOptions {
            case_sensitive: true,
            require_literal_separator: true,
            require_literal_leading_dot: false,
        };

        let mut out = Vec::new();
        for entry in glob_with(pattern_str, opts).map_err(ThirdPartyError::Pattern)? {
            match entry {
                Ok(path) => out.push(path),
                Err(e) => return Err(ThirdPartyError::Glob(e)),
            }
        }
        out.sort();
        Ok(out)
    }
    ```

- [ ] **Step 1.7: Write failing tests in `src/third_party/tests.rs`**

    ```rust
    use super::model::*;
    use super::glob::glob_manifests;
    use tempfile::TempDir;

    fn setup_fake_venv(packages: &[(&str, &str)]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let site = tmp.path().join("lib").join("python3.13").join("site-packages");
        std::fs::create_dir_all(&site).unwrap();
        for (pkg, contents) in packages {
            let pkg_dir = site.join(pkg);
            std::fs::create_dir_all(&pkg_dir).unwrap();
            std::fs::write(pkg_dir.join("toolr-manifest.json"), contents).unwrap();
        }
        tmp
    }

    #[test]
    fn fragment_round_trips_through_json() {
        let f = ManifestFragment {
            toolr_schema_version: FRAGMENT_SCHEMA_VERSION,
            package: "my_pkg".into(),
            groups: vec![FragmentGroup {
                name: "deploy".into(),
                title: "Deploy".into(),
                description: String::new(),
            }],
            commands: vec![],
        };
        let json = serde_json::to_string_pretty(&f).unwrap();
        let back: ManifestFragment = serde_json::from_str(&json).unwrap();
        assert_eq!(f, back);
    }

    #[test]
    fn glob_finds_only_toolr_manifest_files() {
        let tmp = setup_fake_venv(&[
            ("a_pkg", r#"{"toolr_schema_version": 1, "package": "a_pkg"}"#),
            ("b_pkg", r#"{"toolr_schema_version": 1, "package": "b_pkg"}"#),
        ]);
        // Drop a spurious file the glob must ignore.
        let site = tmp.path().join("lib").join("python3.13").join("site-packages");
        std::fs::write(site.join("a_pkg").join("README"), "ignored").unwrap();

        let hits = glob_manifests(tmp.path()).unwrap();
        assert_eq!(hits.len(), 2);
        assert!(hits[0].ends_with("a_pkg/toolr-manifest.json"));
        assert!(hits[1].ends_with("b_pkg/toolr-manifest.json"));
    }

    #[test]
    fn glob_returns_empty_when_no_site_packages() {
        let tmp = TempDir::new().unwrap();
        let hits = glob_manifests(tmp.path()).unwrap();
        assert!(hits.is_empty());
    }
    ```

- [ ] **Step 1.8: Run tests, expect compile failures**

    `parse.rs` still defines `ThirdPartyError` lazily — we have a forward
    reference. Add a minimal `ThirdPartyError` stub to `src/third_party/parse.rs`
    so the crate compiles:

    ```rust
    //! Placeholder — fully populated in Task 2.

    use std::path::PathBuf;

    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum ThirdPartyError {
        #[error("non-UTF-8 path: {0}")]
        NonUtf8Path(PathBuf),
        #[error("glob pattern error: {0}")]
        Pattern(#[from] glob::PatternError),
        #[error("glob iteration error: {0}")]
        Glob(#[from] glob::GlobError),
    }
    ```

- [ ] **Step 1.9: Run the tests**

    ```bash
    cargo test --lib third_party::
    ```

    Expected: 3 tests passing.

- [ ] **Step 1.10: Commit**

    ```bash
    git add Cargo.toml src/lib.rs src/third_party/
    git commit -m "feat(third_party): Add fragment model and site-packages glob helper"
    ```

---

## Task 2: Parse + validate a single fragment

Extend `ThirdPartyError` with version + JSON cases. Implement
`parse_fragment(path)`. Rejection rules:

- File is not valid JSON → `Json`.
- Missing `toolr_schema_version` (or non-integer) → `MissingVersion`.
- Version newer than `FRAGMENT_SCHEMA_VERSION` → `UnknownVersion`.
- Version zero or negative-via-overflow → `MissingVersion`.

**Files:**

- Modify: `src/third_party/parse.rs`
- Modify: `src/third_party/tests.rs`

- [ ] **Step 2.1: Replace the stub `src/third_party/parse.rs`**

    ```rust
    //! Parse and validate a single third-party manifest fragment file.

    use std::fs;
    use std::path::{Path, PathBuf};

    use thiserror::Error;

    use super::migrate::migrate_to_current;
    use super::model::{ManifestFragment, FRAGMENT_SCHEMA_VERSION};

    #[derive(Debug, Error)]
    pub enum ThirdPartyError {
        #[error("non-UTF-8 path: {0}")]
        NonUtf8Path(PathBuf),
        #[error("glob pattern error: {0}")]
        Pattern(#[from] glob::PatternError),
        #[error("glob iteration error: {0}")]
        Glob(#[from] glob::GlobError),
        #[error("I/O error reading {path}: {source}")]
        Io {
            path: PathBuf,
            #[source]
            source: std::io::Error,
        },
        #[error("invalid JSON in {path}: {source}")]
        Json {
            path: PathBuf,
            #[source]
            source: serde_json::Error,
        },
        #[error(
            "{path}: missing or non-integer `toolr_schema_version` key — \
             this file is not a valid toolr manifest fragment"
        )]
        MissingVersion { path: PathBuf },
        #[error(
            "{path}: toolr_schema_version {version} is newer than this toolr \
             binary supports (max {max}). Upgrade toolr."
        )]
        UnknownVersion {
            path: PathBuf,
            version: u32,
            max: u32,
        },
        #[error("{path}: migration from v{version} failed: {reason}")]
        Migration {
            path: PathBuf,
            version: u32,
            reason: String,
        },
    }

    /// Parse one fragment file, validating `toolr_schema_version` and
    /// migrating older fragments to the current shape. Returns the
    /// migrated, ready-to-merge fragment.
    pub fn parse_fragment(path: &Path) -> Result<ManifestFragment, ThirdPartyError> {
        let bytes = fs::read(path).map_err(|e| ThirdPartyError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        let raw: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| ThirdPartyError::Json {
                path: path.to_path_buf(),
                source: e,
            })?;

        let version = raw
            .as_object()
            .and_then(|m| m.get("toolr_schema_version"))
            .and_then(|v| v.as_u64())
            .and_then(|v| u32::try_from(v).ok())
            .filter(|v| *v >= 1)
            .ok_or_else(|| ThirdPartyError::MissingVersion {
                path: path.to_path_buf(),
            })?;

        if version > FRAGMENT_SCHEMA_VERSION {
            return Err(ThirdPartyError::UnknownVersion {
                path: path.to_path_buf(),
                version,
                max: FRAGMENT_SCHEMA_VERSION,
            });
        }

        let migrated = migrate_to_current(raw, version).map_err(|reason| {
            ThirdPartyError::Migration {
                path: path.to_path_buf(),
                version,
                reason,
            }
        })?;

        serde_json::from_value(migrated).map_err(|e| ThirdPartyError::Json {
            path: path.to_path_buf(),
            source: e,
        })
    }
    ```

- [ ] **Step 2.2: Replace the stub `src/third_party/migrate.rs`**

    Placeholder migration framework — populated in detail in Task 3, but the
    `migrate_to_current` symbol must exist for `parse.rs` to compile:

    ```rust
    //! Schema-version migration framework.
    //!
    //! Each entry transforms a fragment at version N to a fragment at
    //! version N+1. `migrate_to_current` applies them in order from the
    //! detected version up to `FRAGMENT_SCHEMA_VERSION`.

    use super::model::FRAGMENT_SCHEMA_VERSION;

    /// Migrate `raw` JSON forward from `from_version` to
    /// `FRAGMENT_SCHEMA_VERSION`. Returns the migrated JSON, or a
    /// human-readable reason string on failure.
    pub fn migrate_to_current(
        raw: serde_json::Value,
        from_version: u32,
    ) -> Result<serde_json::Value, String> {
        let mut value = raw;
        for v in from_version..FRAGMENT_SCHEMA_VERSION {
            value = step(value, v)?;
        }
        Ok(value)
    }

    /// Migrate a single version step `v -> v+1`. Currently only `1 -> 2`
    /// would exist, and version 2 has not been defined yet, so this is a
    /// placeholder returning an error if it is ever called.
    fn step(_value: serde_json::Value, v: u32) -> Result<serde_json::Value, String> {
        Err(format!("no migration registered for v{v} → v{}", v + 1))
    }
    ```

- [ ] **Step 2.3: Add parse tests to `src/third_party/tests.rs`**

    Append:

    ```rust
    use super::parse::{parse_fragment, ThirdPartyError};

    fn write_fragment(tmp: &TempDir, pkg: &str, contents: &str) -> std::path::PathBuf {
        let site = tmp.path().join("lib").join("python3.13").join("site-packages");
        let pkg_dir = site.join(pkg);
        std::fs::create_dir_all(&pkg_dir).unwrap();
        let path = pkg_dir.join("toolr-manifest.json");
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn parse_accepts_minimal_v1_fragment() {
        let tmp = TempDir::new().unwrap();
        let path = write_fragment(
            &tmp,
            "my_pkg",
            r#"{
                "toolr_schema_version": 1,
                "package": "my_pkg",
                "groups": [],
                "commands": []
            }"#,
        );
        let frag = parse_fragment(&path).expect("should parse");
        assert_eq!(frag.toolr_schema_version, 1);
        assert_eq!(frag.package, "my_pkg");
    }

    #[test]
    fn parse_rejects_missing_version_key() {
        let tmp = TempDir::new().unwrap();
        let path = write_fragment(
            &tmp,
            "bad_pkg",
            r#"{"package": "bad_pkg", "groups": [], "commands": []}"#,
        );
        let err = parse_fragment(&path).expect_err("should reject");
        assert!(matches!(err, ThirdPartyError::MissingVersion { .. }));
    }

    #[test]
    fn parse_rejects_unknown_future_version() {
        let tmp = TempDir::new().unwrap();
        let path = write_fragment(
            &tmp,
            "future_pkg",
            r#"{"toolr_schema_version": 999, "package": "future_pkg"}"#,
        );
        let err = parse_fragment(&path).expect_err("should reject");
        assert!(matches!(err, ThirdPartyError::UnknownVersion { version: 999, .. }));
    }

    #[test]
    fn parse_rejects_malformed_json() {
        let tmp = TempDir::new().unwrap();
        let path = write_fragment(&tmp, "bad_pkg", "not valid json");
        let err = parse_fragment(&path).expect_err("should reject");
        assert!(matches!(err, ThirdPartyError::Json { .. }));
    }
    ```

- [ ] **Step 2.4: Run tests**

    ```bash
    cargo test --lib third_party::
    ```

    Expected: 7 tests passing (3 from Task 1 + 4 new).

- [ ] **Step 2.5: Commit**

    ```bash
    git add src/third_party/
    git commit -m "feat(third_party): Parse + validate manifest fragments with schema-version guard"
    ```

---

## Task 3: Migration framework with a verifiable identity step

Tighten the migration framework so we can:

- Prove `v1 → v1` (no migrations needed) round-trips identically.
- Easily register `v1 → v2` later by adding one match arm and one function.

**Files:**

- Modify: `src/third_party/migrate.rs`
- Modify: `src/third_party/tests.rs`

- [ ] **Step 3.1: Replace `src/third_party/migrate.rs`**

    ```rust
    //! Schema-version migration framework for third-party manifest fragments.
    //!
    //! Adding a future migration is mechanical:
    //!
    //! 1. Bump `FRAGMENT_SCHEMA_VERSION` in `model.rs`.
    //! 2. Add a `migrate_vN_to_vN_plus_1` function below.
    //! 3. Register it in `step(..)`'s match arm.
    //! 4. Update existing fragment-model field defaults / `#[serde(default)]`
    //!    so v1-shaped input still deserializes after step-up.
    //! 5. Add a unit test exercising a v(N) fixture through migration to vCurrent.

    use serde_json::Value;

    use super::model::FRAGMENT_SCHEMA_VERSION;

    /// Migrate `raw` JSON forward from `from_version` to
    /// `FRAGMENT_SCHEMA_VERSION`, applying registered migrations in order.
    ///
    /// Returns the migrated JSON value on success, or a human-readable
    /// reason string on failure.
    pub fn migrate_to_current(raw: Value, from_version: u32) -> Result<Value, String> {
        let mut value = raw;
        let mut v = from_version;
        while v < FRAGMENT_SCHEMA_VERSION {
            value = step(value, v)?;
            v += 1;
        }
        Ok(value)
    }

    /// Apply a single version step `v -> v+1`. Add a match arm for each new
    /// migration as the schema evolves.
    fn step(value: Value, v: u32) -> Result<Value, String> {
        match v {
            // No migrations exist yet — `FRAGMENT_SCHEMA_VERSION == 1` means
            // `migrate_to_current` is a no-op. This match exists for the
            // shape of future migrations:
            //   1 => migrate_v1_to_v2(value),
            //   2 => migrate_v2_to_v3(value),
            _ => Err(format!("no migration registered for v{v} → v{}", v + 1)),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use serde_json::json;

        #[test]
        fn v1_to_current_is_identity() {
            // At FRAGMENT_SCHEMA_VERSION == 1, this should be a no-op.
            let input = json!({
                "toolr_schema_version": 1,
                "package": "my_pkg",
                "groups": [],
                "commands": []
            });
            let out = migrate_to_current(input.clone(), 1).unwrap();
            assert_eq!(out, input);
        }

        #[test]
        fn unregistered_step_returns_error() {
            // Force a step from "99" — there's no migration for it.
            let err = super::step(json!({}), 99).unwrap_err();
            assert!(err.contains("no migration registered"));
        }
    }
    ```

- [ ] **Step 3.2: Run tests**

    ```bash
    cargo test --lib third_party::
    ```

    Expected: 9 tests passing (7 previous + 2 new). All `parse_fragment`
    tests still pass because no migration is needed for v1.

- [ ] **Step 3.3: Commit**

    ```bash
    git add src/third_party/migrate.rs
    git commit -m "feat(third_party): Add schema-version migration framework with identity step"
    ```

---

## Task 4: Merge fragments into a `Manifest`

Add a `merge_into_manifest` function that takes a base `Manifest` and a slice
of parsed `ManifestFragment`s, returning a new `Manifest` with the
third-party groups and commands added. De-duplicate by `(group, name)` —
local `tools/` definitions always win over third-party ones (with a debug log
on collision); third-party-to-third-party collisions are reported as
`ThirdPartyError::DuplicateCommand`.

**Files:**

- Create: `src/third_party/merge.rs`
- Modify: `src/third_party/mod.rs`
- Modify: `src/third_party/parse.rs` (add `DuplicateCommand` variant)
- Modify: `src/third_party/tests.rs`

- [ ] **Step 4.1: Extend `ThirdPartyError`**

    In `src/third_party/parse.rs`, add inside the `enum ThirdPartyError`:

    ```rust
    #[error(
        "duplicate command `{group}/{name}` declared by both `{first_package}` \
         and `{second_package}`"
    )]
    DuplicateCommand {
        group: String,
        name: String,
        first_package: String,
        second_package: String,
    },
    ```

- [ ] **Step 4.2: Create `src/third_party/merge.rs`**

    ```rust
    //! Merge parsed third-party fragments into a project `Manifest`.

    use std::collections::HashMap;

    use log::debug;

    use super::model::{FragmentArgument, FragmentCommand, FragmentGroup, ManifestFragment};
    use super::parse::ThirdPartyError;
    use crate::manifest::{Argument, Command, Group, Manifest, Origin};

    /// Consume `fragments`, merging their groups + commands into `base`.
    ///
    /// Conflict resolution:
    /// - A group/command pair already present in `base` (from `tools/**/*.py`)
    ///   wins; the third-party entry is skipped (with a debug log).
    /// - A group/command pair declared by two different third-party packages
    ///   produces `ThirdPartyError::DuplicateCommand`.
    /// - Groups merge by `name`: if a third-party fragment declares a group
    ///   already present in `base` or in a prior fragment, the existing
    ///   group's title/description are kept.
    pub fn merge_into_manifest(
        mut base: Manifest,
        fragments: Vec<ManifestFragment>,
    ) -> Result<Manifest, ThirdPartyError> {
        // (group, command) → package that defined it. Used to detect
        // third-party-to-third-party collisions.
        let mut owner: HashMap<(String, String), String> = HashMap::new();
        for cmd in &base.commands {
            owner.insert((cmd.group.clone(), cmd.name.clone()), "<project>".to_string());
        }

        let mut known_groups: std::collections::HashSet<String> =
            base.groups.iter().map(|g| g.name.clone()).collect();

        for fragment in fragments {
            for fg in fragment.groups {
                if known_groups.insert(fg.name.clone()) {
                    base.groups.push(group_from_fragment(fg));
                }
            }
            for fc in fragment.commands {
                let key = (fc.group.clone(), fc.name.clone());
                if let Some(first) = owner.get(&key) {
                    if first == "<project>" {
                        debug!(
                            "third-party package `{}` declared command \
                             `{}/{}`, but `tools/` already defines it; \
                             keeping local",
                            fragment.package, fc.group, fc.name,
                        );
                        continue;
                    }
                    return Err(ThirdPartyError::DuplicateCommand {
                        group: fc.group,
                        name: fc.name,
                        first_package: first.clone(),
                        second_package: fragment.package.clone(),
                    });
                }
                owner.insert(key, fragment.package.clone());
                base.commands.push(command_from_fragment(fc));
            }
        }

        Ok(base)
    }

    fn group_from_fragment(fg: FragmentGroup) -> Group {
        Group {
            name: fg.name,
            title: fg.title,
            description: fg.description,
            origin: Origin::Static,
        }
    }

    fn command_from_fragment(fc: FragmentCommand) -> Command {
        Command {
            name: fc.name,
            group: fc.group,
            module: fc.module,
            function: fc.function,
            summary: fc.summary,
            description: fc.description,
            arguments: fc.arguments.into_iter().map(argument_from_fragment).collect(),
            imports: fc.imports,
            origin: Origin::Static,
        }
    }

    fn argument_from_fragment(fa: FragmentArgument) -> Argument {
        Argument {
            name: fa.name,
            kind: fa.kind,
            help: fa.help,
            default: fa.default,
            type_annotation: fa.type_annotation,
            allowed_values: fa.allowed_values,
        }
    }
    ```

    **Note:** `log` is added to the crate via the `log = "0.4"` line in
    `Cargo.toml`. If it is not already present, add it under
    `[dependencies]`.

- [ ] **Step 4.3: Update `Cargo.toml`**

    Ensure under `[dependencies]`:

    ```toml
    log = "0.4"
    ```

- [ ] **Step 4.4: Re-export from `src/third_party/mod.rs`**

    Add:

    ```rust
    pub mod merge;
    pub use merge::merge_into_manifest;
    ```

- [ ] **Step 4.5: Add merge tests in `src/third_party/tests.rs`**

    Append:

    ```rust
    use super::merge::merge_into_manifest;
    use crate::manifest::{ArgumentKind, Command, Group, Manifest, Origin, SCHEMA_VERSION};

    fn empty_base() -> Manifest {
        Manifest {
            schema_version: SCHEMA_VERSION,
            static_hash: String::new(),
            dynamic_hash: String::new(),
            groups: vec![],
            commands: vec![],
        }
    }

    fn sample_fragment(pkg: &str, group: &str, name: &str) -> ManifestFragment {
        ManifestFragment {
            toolr_schema_version: FRAGMENT_SCHEMA_VERSION,
            package: pkg.into(),
            groups: vec![FragmentGroup {
                name: group.into(),
                title: group.to_uppercase(),
                description: String::new(),
            }],
            commands: vec![FragmentCommand {
                name: name.into(),
                group: group.into(),
                module: format!("{pkg}.commands"),
                function: name.replace('-', "_"),
                summary: String::new(),
                description: String::new(),
                arguments: vec![],
                imports: vec![],
            }],
        }
    }

    #[test]
    fn merge_adds_groups_and_commands_from_fragments() {
        let merged =
            merge_into_manifest(empty_base(), vec![sample_fragment("pkg_a", "deploy", "rollout")])
                .unwrap();
        assert_eq!(merged.groups.len(), 1);
        assert_eq!(merged.groups[0].name, "deploy");
        assert_eq!(merged.commands.len(), 1);
        assert_eq!(merged.commands[0].name, "rollout");
        assert_eq!(merged.commands[0].origin, Origin::Static);
    }

    #[test]
    fn merge_skips_third_party_command_when_local_already_defines_it() {
        let mut base = empty_base();
        base.groups.push(Group {
            name: "deploy".into(),
            title: "Deploy".into(),
            description: String::new(),
            origin: Origin::Static,
        });
        base.commands.push(Command {
            name: "rollout".into(),
            group: "deploy".into(),
            module: "tools.deploy".into(),
            function: "rollout".into(),
            summary: "local".into(),
            description: String::new(),
            arguments: vec![],
            imports: vec![],
            origin: Origin::Static,
        });
        let merged =
            merge_into_manifest(base, vec![sample_fragment("pkg_a", "deploy", "rollout")])
                .unwrap();
        assert_eq!(merged.commands.len(), 1);
        assert_eq!(merged.commands[0].summary, "local");
    }

    #[test]
    fn merge_errors_on_third_party_to_third_party_collision() {
        let err = merge_into_manifest(
            empty_base(),
            vec![
                sample_fragment("pkg_a", "deploy", "rollout"),
                sample_fragment("pkg_b", "deploy", "rollout"),
            ],
        )
        .expect_err("should collide");
        let msg = err.to_string();
        assert!(msg.contains("pkg_a"), "got: {msg}");
        assert!(msg.contains("pkg_b"), "got: {msg}");
    }

    // Silence an unused-warning suppression: kind, ArgumentKind
    #[test]
    fn argument_kind_propagates_through_merge() {
        let mut frag = sample_fragment("pkg_a", "deploy", "rollout");
        frag.commands[0].arguments.push(FragmentArgument {
            name: "force".into(),
            kind: ArgumentKind::Flag,
            help: String::new(),
            default: None,
            type_annotation: None,
            allowed_values: vec![],
        });
        let merged = merge_into_manifest(empty_base(), vec![frag]).unwrap();
        assert_eq!(merged.commands[0].arguments.len(), 1);
        assert_eq!(merged.commands[0].arguments[0].kind, ArgumentKind::Flag);
    }
    ```

- [ ] **Step 4.6: Run tests**

    ```bash
    cargo test --lib third_party::
    ```

    Expected: 13 tests passing (9 prior + 4 new).

- [ ] **Step 4.7: Commit**

    ```bash
    git add Cargo.toml src/third_party/
    git commit -m "feat(third_party): Merge fragments into Manifest with conflict resolution"
    ```

---

## Task 5: End-to-end discovery against a fake venv

Wire `glob_manifests` + `parse_fragment` + `merge_into_manifest` into a single
entry point: `discover_and_merge(tools_venv, base)`. Test against a fake
site-packages tree with multiple packages, including one with a malformed
fragment that should fail loudly rather than silently skip.

**Files:**

- Modify: `src/third_party/mod.rs`
- Modify: `src/third_party/tests.rs`

- [ ] **Step 5.1: Add the orchestrator to `src/third_party/mod.rs`**

    Append:

    ```rust
    use std::path::Path;

    use crate::manifest::Manifest;

    /// Glob for fragments under `tools_venv`, parse + migrate each, and merge
    /// them into `base`. Returns the augmented manifest.
    ///
    /// Failure modes (any one fragment failing aborts the whole merge so
    /// users see the broken package immediately rather than silently missing
    /// commands):
    /// - Malformed JSON in any fragment → `ThirdPartyError::Json`.
    /// - Missing/invalid `toolr_schema_version` → `MissingVersion`.
    /// - Version newer than this binary → `UnknownVersion`.
    /// - Third-party-to-third-party command collision → `DuplicateCommand`.
    pub fn discover_and_merge(
        tools_venv: &Path,
        base: Manifest,
    ) -> Result<Manifest, ThirdPartyError> {
        let paths = glob_manifests(tools_venv)?;
        let mut fragments = Vec::with_capacity(paths.len());
        for path in paths {
            fragments.push(parse_fragment(&path)?);
        }
        merge_into_manifest(base, fragments)
    }
    ```

- [ ] **Step 5.2: Add end-to-end tests in `src/third_party/tests.rs`**

    Append:

    ```rust
    use super::discover_and_merge;

    #[test]
    fn discover_and_merge_picks_up_all_valid_fragments() {
        let tmp = setup_fake_venv(&[
            (
                "pkg_a",
                r#"{
                    "toolr_schema_version": 1,
                    "package": "pkg_a",
                    "groups": [{"name": "deploy", "title": "Deploy", "description": ""}],
                    "commands": [{
                        "name": "rollout", "group": "deploy",
                        "module": "pkg_a.commands", "function": "rollout",
                        "summary": "", "description": "",
                        "arguments": [], "imports": []
                    }]
                }"#,
            ),
            (
                "pkg_b",
                r#"{
                    "toolr_schema_version": 1,
                    "package": "pkg_b",
                    "groups": [{"name": "lint", "title": "Lint", "description": ""}],
                    "commands": [{
                        "name": "check", "group": "lint",
                        "module": "pkg_b.commands", "function": "check",
                        "summary": "", "description": "",
                        "arguments": [], "imports": []
                    }]
                }"#,
            ),
        ]);
        let merged = discover_and_merge(tmp.path(), empty_base()).unwrap();
        let group_names: Vec<_> = merged.groups.iter().map(|g| g.name.clone()).collect();
        let command_names: Vec<_> = merged.commands.iter().map(|c| c.name.clone()).collect();
        assert!(group_names.contains(&"deploy".to_string()));
        assert!(group_names.contains(&"lint".to_string()));
        assert!(command_names.contains(&"rollout".to_string()));
        assert!(command_names.contains(&"check".to_string()));
    }

    #[test]
    fn discover_and_merge_aborts_on_malformed_fragment() {
        let tmp = setup_fake_venv(&[
            ("pkg_ok", r#"{"toolr_schema_version": 1, "package": "pkg_ok"}"#),
            ("pkg_bad", "not valid json at all"),
        ]);
        let err = discover_and_merge(tmp.path(), empty_base()).expect_err("should abort");
        assert!(matches!(err, ThirdPartyError::Json { .. }));
    }

    #[test]
    fn discover_and_merge_no_op_when_venv_has_no_fragments() {
        let tmp = TempDir::new().unwrap();
        // Create site-packages but no fragments.
        std::fs::create_dir_all(
            tmp.path().join("lib").join("python3.13").join("site-packages"),
        )
        .unwrap();
        let merged = discover_and_merge(tmp.path(), empty_base()).unwrap();
        assert!(merged.groups.is_empty());
        assert!(merged.commands.is_empty());
    }
    ```

- [ ] **Step 5.3: Run tests**

    ```bash
    cargo test --lib third_party::
    ```

    Expected: 16 tests passing (13 prior + 3 new).

- [ ] **Step 5.4: Commit**

    ```bash
    git add src/third_party/
    git commit -m "feat(third_party): Add discover_and_merge orchestrator with fail-fast on bad fragments"
    ```

---

## Task 6: Wire `build_static_manifest` to consult third-party fragments

Extend `_rust_utils::parser::build_static_manifest` so the augmented entry
point also merges third-party fragments when a `tools_venv` path is
available. Keep the existing single-arg signature working for callers that
only have a `tools_dir`; add a new
`build_static_manifest_with_venv(tools_dir, tools_venv)` variant for the
augmented case.

**Files:**

- Modify: `src/parser/build.rs`
- Modify: `src/parser/mod.rs`

- [ ] **Step 6.1: Add the new builder**

    In `src/parser/build.rs`, append:

    ```rust
    use crate::third_party::{discover_and_merge, ThirdPartyError};

    /// Like `build_static_manifest`, but also globs `tools_venv` for
    /// third-party manifest fragments and merges them in.
    pub fn build_static_manifest_with_venv(
        tools_dir: &Path,
        tools_venv: &Path,
    ) -> Result<Manifest, BuildError> {
        let base = build_static_manifest(tools_dir).map_err(BuildError::Build)?;
        discover_and_merge(tools_venv, base).map_err(BuildError::ThirdParty)
    }

    /// Error type covering both the local build and the third-party merge.
    #[derive(Debug, thiserror::Error)]
    pub enum BuildError {
        #[error("static build error: {0}")]
        Build(#[source] anyhow::Error),
        #[error("third-party merge error: {0}")]
        ThirdParty(#[from] ThirdPartyError),
    }
    ```

    **Note:** if `build_static_manifest` already returns `anyhow::Result`,
    keep that as is and adapt the `BuildError::Build` variant accordingly.
    The point is that callers receive a typed error distinguishing local
    failures from third-party-fragment failures.

- [ ] **Step 6.2: Re-export from `src/parser/mod.rs`**

    Append:

    ```rust
    pub use build::{build_static_manifest_with_venv, BuildError};
    ```

- [ ] **Step 6.3: Add an integration test in `src/parser/build.rs::tests`**

    Append:

    ```rust
    use crate::third_party::{ManifestFragment, FragmentGroup, FragmentCommand, FRAGMENT_SCHEMA_VERSION};

    #[test]
    fn build_with_venv_merges_local_and_third_party() {
        let tmp = TempDir::new().unwrap();
        // Local tools/ side.
        write(
            tmp.path(),
            "tools/ci.py",
            r#""""CI utilities."""
group = command_group("ci", "CI utilities", docstring=**doc**)

@group.command
def hello(ctx):
    """Say hello."""
    pass
"#,
        );
        // Fake tools venv with a third-party fragment.
        let venv = tmp.path().join("venv");
        let site = venv.join("lib").join("python3.13").join("site-packages");
        std::fs::create_dir_all(site.join("ext_pkg")).unwrap();
        let frag = ManifestFragment {
            toolr_schema_version: FRAGMENT_SCHEMA_VERSION,
            package: "ext_pkg".into(),
            groups: vec![FragmentGroup {
                name: "deploy".into(),
                title: "Deploy".into(),
                description: String::new(),
            }],
            commands: vec![FragmentCommand {
                name: "rollout".into(),
                group: "deploy".into(),
                module: "ext_pkg.commands".into(),
                function: "rollout".into(),
                summary: String::new(),
                description: String::new(),
                arguments: vec![],
                imports: vec![],
            }],
        };
        std::fs::write(
            site.join("ext_pkg").join("toolr-manifest.json"),
            serde_json::to_string(&frag).unwrap(),
        )
        .unwrap();

        let m =
            build_static_manifest_with_venv(&tmp.path().join("tools"), &venv).unwrap();
        let groups: Vec<_> = m.groups.iter().map(|g| g.name.as_str()).collect();
        assert!(groups.contains(&"ci"));
        assert!(groups.contains(&"deploy"));
        let cmds: Vec<_> = m.commands.iter().map(|c| c.name.as_str()).collect();
        assert!(cmds.contains(&"hello"));
        assert!(cmds.contains(&"rollout"));
    }
    ```

- [ ] **Step 6.4: Run tests**

    ```bash
    cargo test --lib parser::build::
    ```

    Expected: all previous parser::build tests pass + 1 new.

- [ ] **Step 6.5: Commit**

    ```bash
    git add src/parser/
    git commit -m "feat(parser): Add build_static_manifest_with_venv merging third-party fragments"
    ```

---

## Task 7: `toolr.build` Python module — programmatic API

Add `python/toolr/build.py` exposing `build_manifest(...)`. The function:

1. Imports the named package in the active Python environment.
2. Walks `_get_command_group_storage()` to enumerate `command_group(...)`
   results that were created during the import.
3. Filters to groups whose source module starts with the package name
   (so importing the package doesn't pick up `tools/`-defined groups by
   accident).
4. Emits a dict matching the `ManifestFragment` shape, pinning
   `toolr_schema_version` (default: current package's
   `MANIFEST_SCHEMA_VERSION` constant).
5. Validates the result against a strict schema check before returning.

**Files:**

- Create: `python/toolr/build.py`
- Modify: `python/toolr/__init__.py` (re-export `build_manifest` and the
  schema-version constant)
- Create: `tests/python/test_build_manifest.py` (or extend an existing test
  module if one matches naming conventions in this repo)

- [ ] **Step 7.1: Add the schema-version constant**

    In `python/toolr/_registry.py` (or a new `python/toolr/_schema.py` if
    the repo prefers separation), define:

    ```python
    MANIFEST_SCHEMA_VERSION: int = 1
    """Current toolr manifest fragment schema version.

    Mirrors `FRAGMENT_SCHEMA_VERSION` on the Rust side. Bump in lockstep
    when introducing a breaking change to the fragment format.
    """
    ```

    Re-export it from `python/toolr/__init__.py`:

    ```python
    from toolr._registry import MANIFEST_SCHEMA_VERSION
    ```

- [ ] **Step 7.2: Create `python/toolr/build.py`**

    ```python
    """Build a toolr manifest fragment for a third-party package.

    Walks the package's `command_group` / `@group.command` registry to
    produce a static `toolr-manifest.json` that the Rust binary can
    discover and merge without any further Python introspection.
    """
    from __future__ import annotations

    import argparse
    import importlib
    import json
    import sys
    from collections.abc import Callable
    from dataclasses import dataclass
    from pathlib import Path
    from typing import Any

    from toolr._registry import MANIFEST_SCHEMA_VERSION
    from toolr._registry import _get_command_group_storage
    from toolr.utils._signature import get_signature


    @dataclass(frozen=True)
    class BuildResult:
        """Result of `build_manifest`."""

        fragment: dict[str, Any]
        output_path: Path
        drift: bool = False
        """True only when `check=True` and the regenerated fragment differs
        from the file currently on disk."""


    def build_manifest(
        package_name: str,
        *,
        output_path: Path | None = None,
        schema_version: int | None = None,
        check: bool = False,
    ) -> BuildResult:
        """Generate a manifest fragment for `package_name`.

        Args:
            package_name: Dotted name of the package to introspect, e.g.
                ``"my_toolr_pkg"``.
            output_path: Where to write `toolr-manifest.json`. Defaults to
                ``<package_dir>/toolr-manifest.json`` where ``<package_dir>``
                is resolved from the package's ``__file__``.
            schema_version: Override the schema version written out.
                Defaults to ``MANIFEST_SCHEMA_VERSION``.
            check: If True, do not write the file; instead, compare the
                generated fragment against the file currently at
                `output_path`. Sets ``BuildResult.drift=True`` on mismatch.
                The caller (CLI / pre-commit) decides what to do with that
                flag (typically: exit non-zero).

        Raises:
            ModuleNotFoundError: `package_name` is not importable.
            BuildManifestError: The package has no toolr commands.
        """
        module = importlib.import_module(package_name)
        package_root = _resolve_package_root(module, package_name)
        if output_path is None:
            output_path = package_root / "toolr-manifest.json"
        version = schema_version if schema_version is not None else MANIFEST_SCHEMA_VERSION

        fragment = _collect_fragment(package_name, version)
        if not fragment["groups"] and not fragment["commands"]:
            raise BuildManifestError(
                f"package `{package_name}` declares no toolr commands — "
                "nothing to write"
            )

        serialized = json.dumps(fragment, indent=2, sort_keys=True) + "\n"

        if check:
            existing = output_path.read_text() if output_path.is_file() else ""
            drift = existing != serialized
            return BuildResult(fragment=fragment, output_path=output_path, drift=drift)

        output_path.write_text(serialized)
        return BuildResult(fragment=fragment, output_path=output_path)


    class BuildManifestError(Exception):
        """Raised when the manifest cannot be built (no commands, etc.)."""


    def _resolve_package_root(module: Any, package_name: str) -> Path:
        file = getattr(module, "__file__", None)
        if file is None:
            raise BuildManifestError(
                f"`{package_name}` has no `__file__` — cannot resolve its "
                "installed directory. Namespace packages are not supported."
            )
        return Path(file).resolve().parent


    def _collect_fragment(package_name: str, version: int) -> dict[str, Any]:
        """Walk the global registry, keep only groups/commands that belong
        to `package_name`."""
        storage = _get_command_group_storage()
        groups: list[dict[str, Any]] = []
        commands: list[dict[str, Any]] = []
        seen_groups: set[str] = set()

        for group in sorted(storage.values(), key=lambda g: g.full_name):
            for cmd_name, func in group.get_commands().items():
                if not _belongs_to_package(func, package_name):
                    continue
                if group.name not in seen_groups:
                    seen_groups.add(group.name)
                    groups.append(
                        {
                            "name": group.name,
                            "title": group.title,
                            "description": group.description or "",
                        }
                    )
                commands.append(_serialize_command(group.name, cmd_name, func))

        return {
            "toolr_schema_version": version,
            "package": package_name,
            "groups": groups,
            "commands": commands,
        }


    def _belongs_to_package(func: Callable[..., Any], package_name: str) -> bool:
        module = getattr(func, "__module__", "")
        return module == package_name or module.startswith(f"{package_name}.")


    def _serialize_command(group: str, name: str, func: Callable[..., Any]) -> dict[str, Any]:
        signature = get_signature(func)
        arguments = [
            {
                "name": arg.name,
                "kind": arg.kind,           # "positional" | "optional" | "flag"
                "help": arg.help or "",
                "default": arg.default,
                "type_annotation": arg.type_annotation,
                "allowed_values": list(arg.allowed_values or []),
            }
            for arg in signature.arguments
        ]
        return {
            "name": name,
            "group": group,
            "module": func.__module__,
            "function": func.__name__,
            "summary": signature.short_description or "",
            "description": signature.long_description or "",
            "arguments": arguments,
            "imports": [],   # Filled in by the static parser side; left empty here.
        }
    ```

    **Note for the implementer:** the concrete `signature.arguments`
    attribute shape (`arg.kind`, `arg.allowed_values`, `arg.default`,
    `arg.type_annotation`, etc.) needs to align with the existing
    `toolr.utils._signature.get_signature` return type. Adapt field
    accesses to that type's real public surface, but keep the **emitted
    JSON keys** exactly as written here so the Rust `FragmentArgument`
    deserializer matches.

- [ ] **Step 7.3: Re-export from `python/toolr/__init__.py`**

    Add:

    ```python
    from toolr.build import BuildManifestError, BuildResult, build_manifest
    ```

    Append `"build_manifest"`, `"BuildManifestError"`, `"BuildResult"`, and
    `"MANIFEST_SCHEMA_VERSION"` to `__all__`.

- [ ] **Step 7.4: Add unit tests**

    Create `tests/python/test_build_manifest.py`:

    ```python
    from __future__ import annotations

    import json
    import sys
    import textwrap
    from pathlib import Path

    import pytest

    from toolr.build import BuildManifestError
    from toolr.build import build_manifest


    @pytest.fixture
    def fake_package(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> str:
        """Materialise a tiny third-party package on disk and import it."""
        pkg = tmp_path / "fake_toolr_pkg"
        pkg.mkdir()
        (pkg / "__init__.py").write_text(
            textwrap.dedent(
                '''
                from toolr import command_group

                group = command_group("ext", "External group", description="external")

                @group.command
                def rollout(ctx):
                    """Roll out a new build."""
                '''
            )
        )
        monkeypatch.syspath_prepend(str(tmp_path))
        return "fake_toolr_pkg"


    def test_build_writes_fragment_to_default_path(fake_package: str) -> None:
        result = build_manifest(fake_package)
        assert result.output_path.is_file()
        fragment = json.loads(result.output_path.read_text())
        assert fragment["toolr_schema_version"] == 1
        assert fragment["package"] == fake_package
        names = [c["name"] for c in fragment["commands"]]
        assert "rollout" in names


    def test_build_check_mode_detects_drift(fake_package: str, tmp_path: Path) -> None:
        path = tmp_path / "out.json"
        path.write_text("not the current fragment")
        result = build_manifest(fake_package, output_path=path, check=True)
        assert result.drift is True
        # File on disk must not have been overwritten in check mode.
        assert path.read_text() == "not the current fragment"


    def test_build_check_mode_no_drift_on_match(fake_package: str, tmp_path: Path) -> None:
        path = tmp_path / "out.json"
        build_manifest(fake_package, output_path=path)
        result = build_manifest(fake_package, output_path=path, check=True)
        assert result.drift is False


    def test_build_raises_when_package_declares_no_commands(
        tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        pkg = tmp_path / "empty_pkg"
        pkg.mkdir()
        (pkg / "__init__.py").write_text("")
        monkeypatch.syspath_prepend(str(tmp_path))
        with pytest.raises(BuildManifestError):
            build_manifest("empty_pkg")
    ```

- [ ] **Step 7.5: Run tests**

    ```bash
    uv run pytest tests/python/test_build_manifest.py -v
    ```

    Expected: 4 tests passing. If `get_signature` returns a different
    object shape, adapt `_serialize_command` until the tests pass without
    changing the JSON keys.

- [ ] **Step 7.6: Commit**

    ```bash
    git add python/toolr/ tests/python/test_build_manifest.py
    git commit -m "feat(toolr.build): Programmatic build_manifest API for third-party packages"
    ```

---

## Task 8: `python -m toolr.build` CLI entrypoint

Make `toolr.build` directly executable. The CLI is a thin argparse wrapper
around `build_manifest`. Argument list:

```text
python -m toolr.build <package-name> [--output PATH] [--schema-version N] [--check] [--quiet]
```

Exit codes:

- `0` — wrote (or `--check` and no drift).
- `1` — package error (no commands, import failure).
- `2` — `--check` and drift detected. Stderr lists the diff hint.

**Files:**

- Modify: `python/toolr/build.py` (add `main()` + `if __name__ == "__main__":`)
- Modify: `tests/python/test_build_manifest.py`

- [ ] **Step 8.1: Append a CLI to `python/toolr/build.py`**

    ```python
    def main(argv: list[str] | None = None) -> int:
        parser = argparse.ArgumentParser(
            prog="python -m toolr.build",
            description=(
                "Generate toolr-manifest.json for a third-party command "
                "package by introspecting its command_group / @group.command "
                "declarations."
            ),
        )
        parser.add_argument("package", help="Dotted package name to introspect.")
        parser.add_argument(
            "--output",
            type=Path,
            default=None,
            help="Where to write the manifest. Defaults to <package-dir>/toolr-manifest.json.",
        )
        parser.add_argument(
            "--schema-version",
            type=int,
            default=None,
            help=f"Pin the schema version. Defaults to {MANIFEST_SCHEMA_VERSION}.",
        )
        parser.add_argument(
            "--check",
            action="store_true",
            help="Don't write; exit 2 if the on-disk manifest differs from regenerated.",
        )
        parser.add_argument(
            "--quiet",
            action="store_true",
            help="Suppress informational output.",
        )
        args = parser.parse_args(argv)

        try:
            result = build_manifest(
                args.package,
                output_path=args.output,
                schema_version=args.schema_version,
                check=args.check,
            )
        except ModuleNotFoundError as exc:
            print(f"toolr.build: cannot import package: {exc}", file=sys.stderr)
            return 1
        except BuildManifestError as exc:
            print(f"toolr.build: {exc}", file=sys.stderr)
            return 1

        if args.check:
            if result.drift:
                print(
                    f"toolr.build: {result.output_path} is out of date — "
                    f"regenerate with `python -m toolr.build {args.package}`",
                    file=sys.stderr,
                )
                return 2
            if not args.quiet:
                print(f"toolr.build: {result.output_path} is up to date.")
            return 0

        if not args.quiet:
            n_groups = len(result.fragment["groups"])
            n_commands = len(result.fragment["commands"])
            print(
                f"toolr.build: wrote {n_groups} group(s) / "
                f"{n_commands} command(s) to {result.output_path}"
            )
        return 0


    if __name__ == "__main__":
        raise SystemExit(main())
    ```

- [ ] **Step 8.2: Add CLI tests**

    Append to `tests/python/test_build_manifest.py`:

    ```python
    from toolr.build import main as build_cli


    def test_cli_writes_manifest(fake_package: str, tmp_path: Path) -> None:
        out = tmp_path / "manifest.json"
        rc = build_cli([fake_package, "--output", str(out), "--quiet"])
        assert rc == 0
        assert out.is_file()


    def test_cli_check_exits_2_on_drift(
        fake_package: str, tmp_path: Path
    ) -> None:
        out = tmp_path / "manifest.json"
        out.write_text("stale")
        rc = build_cli([fake_package, "--output", str(out), "--check", "--quiet"])
        assert rc == 2


    def test_cli_check_exits_0_when_up_to_date(
        fake_package: str, tmp_path: Path
    ) -> None:
        out = tmp_path / "manifest.json"
        build_cli([fake_package, "--output", str(out), "--quiet"])
        rc = build_cli([fake_package, "--output", str(out), "--check", "--quiet"])
        assert rc == 0


    def test_cli_exits_1_on_missing_package(tmp_path: Path) -> None:
        rc = build_cli(["this_package_does_not_exist_xyz", "--quiet"])
        assert rc == 1
    ```

- [ ] **Step 8.3: Run tests**

    ```bash
    uv run pytest tests/python/test_build_manifest.py -v
    ```

    Expected: 8 tests passing.

- [ ] **Step 8.4: Manual smoke test**

    ```bash
    uv run python -m toolr.build --help
    ```

    Expected: argparse-rendered help text listing the arguments above.

- [ ] **Step 8.5: Commit**

    ```bash
    git add python/toolr/build.py tests/python/test_build_manifest.py
    git commit -m "feat(toolr.build): Add python -m toolr.build CLI with --check drift detection"
    ```

---

## Task 9: Wheel-author guidance and validation

Add a docstring section + helper that validates the fragment matches the
Rust-side schema before writing. This catches packaging mistakes (omitting
the file from `package_data`, accidentally shipping a v2 fragment built
against a future toolr) at the author's machine rather than at the
consumer's.

**Files:**

- Modify: `python/toolr/build.py`
- Modify: `tests/python/test_build_manifest.py`

- [ ] **Step 9.1: Add a schema validator**

    In `python/toolr/build.py`, before `build_manifest`:

    ```python
    _ALLOWED_ARG_KINDS = {"positional", "optional", "flag"}


    def _validate_fragment(fragment: dict[str, Any]) -> None:
        """Defensive schema check. Catches author-side packaging mistakes."""
        version = fragment.get("toolr_schema_version")
        if not isinstance(version, int) or version < 1:
            raise BuildManifestError(
                f"`toolr_schema_version` must be a positive int, got {version!r}"
            )
        if not isinstance(fragment.get("package"), str):
            raise BuildManifestError("`package` must be a string")
        for group in fragment.get("groups", []):
            if not isinstance(group.get("name"), str):
                raise BuildManifestError(f"group missing `name`: {group!r}")
        for cmd in fragment.get("commands", []):
            for key in ("name", "group", "module", "function"):
                if not isinstance(cmd.get(key), str):
                    raise BuildManifestError(
                        f"command missing required string field `{key}`: {cmd!r}"
                    )
            for arg in cmd.get("arguments", []):
                if arg.get("kind") not in _ALLOWED_ARG_KINDS:
                    raise BuildManifestError(
                        f"argument `{arg.get('name')!r}` has invalid kind "
                        f"`{arg.get('kind')}` — must be one of {_ALLOWED_ARG_KINDS}"
                    )
    ```

- [ ] **Step 9.2: Call the validator from `build_manifest`**

    Add right after `fragment = _collect_fragment(...)`:

    ```python
    _validate_fragment(fragment)
    ```

- [ ] **Step 9.3: Add a test**

    Append:

    ```python
    def test_validate_rejects_bad_arg_kind(monkeypatch: pytest.MonkeyPatch) -> None:
        from toolr.build import _validate_fragment

        bad = {
            "toolr_schema_version": 1,
            "package": "p",
            "groups": [],
            "commands": [
                {
                    "name": "n",
                    "group": "g",
                    "module": "m",
                    "function": "f",
                    "arguments": [{"name": "x", "kind": "bogus"}],
                }
            ],
        }
        with pytest.raises(BuildManifestError):
            _validate_fragment(bad)
    ```

- [ ] **Step 9.4: Run tests**

    ```bash
    uv run pytest tests/python/test_build_manifest.py -v
    ```

    Expected: 9 tests passing.

- [ ] **Step 9.5: Commit**

    ```bash
    git add python/toolr/build.py tests/python/test_build_manifest.py
    git commit -m "feat(toolr.build): Validate fragment schema before writing"
    ```

---

## Task 10: `toolr self build-manifest <package>` Rust wrapper

Add the `self build-manifest` subcommand to the existing `self` namespace.
The Rust binary locates a Python interpreter, then shells out to
`python -m toolr.build <package> [args]`. Interpreter resolution order:

1. `--python <path>` override.
2. `$VIRTUAL_ENV/bin/python` if set and the binary exists.
3. The tools venv if one is reachable from `$PWD` (best-effort — uses
   `_rust_utils::discovery` if available; if not, skip).
4. `python3` on PATH.
5. `python` on PATH.

If none found → exit code 3 with a clear diagnostic.

**Files:**

- Create: `src/bin/toolr/commands/self_build_manifest.rs`
- Modify: `src/bin/toolr/cli.rs`
- Modify: `src/bin/toolr/dispatch.rs`
- Possibly modify: `src/bin/toolr/main.rs` (module declaration)

- [ ] **Step 10.1: Add the subcommand to `cli.rs`**

    In the section of `cli.rs` that constructs built-in `self` subcommands
    (or, if Plan 1 hasn't carved out the `self` namespace yet, create it
    here), add:

    ```rust
    use clap::{Arg, ArgAction, Command};

    pub fn build_self_subcommand() -> Command {
        Command::new("self")
            .about("Operations on toolr's own state")
            .subcommand_required(true)
            .arg_required_else_help(true)
            .subcommand(
                Command::new("build-manifest")
                    .about("Generate a third-party manifest fragment for a package")
                    .arg(
                        Arg::new("package")
                            .required(true)
                            .help("Dotted Python package name to introspect"),
                    )
                    .arg(
                        Arg::new("output")
                            .long("output")
                            .value_name("PATH")
                            .help("Override the output path"),
                    )
                    .arg(
                        Arg::new("python")
                            .long("python")
                            .value_name("PATH")
                            .help("Path to a Python interpreter to use"),
                    )
                    .arg(
                        Arg::new("schema-version")
                            .long("schema-version")
                            .value_name("N")
                            .help("Pin the emitted schema version"),
                    )
                    .arg(
                        Arg::new("check")
                            .long("check")
                            .action(ArgAction::SetTrue)
                            .help("Verify the on-disk manifest matches regeneration"),
                    ),
            )
    }
    ```

    Wire it into `build_command(...)`:

    ```rust
    root = root.subcommand(build_self_subcommand());
    ```

- [ ] **Step 10.2: Create `src/bin/toolr/commands/self_build_manifest.rs`**

    ```rust
    //! `toolr self build-manifest <package>` implementation.

    use std::path::PathBuf;
    use std::process::{Command, ExitCode};

    use clap::ArgMatches;

    /// Run `python -m toolr.build <package> ...` against a resolved
    /// interpreter, propagating its exit code.
    pub fn run(matches: &ArgMatches) -> anyhow::Result<ExitCode> {
        let package: &String = matches
            .get_one("package")
            .ok_or_else(|| anyhow::anyhow!("missing required argument: package"))?;
        let python = resolve_python(matches.get_one::<String>("python").map(String::as_str))?;
        let mut cmd = Command::new(&python);
        cmd.args(["-m", "toolr.build", package]);
        if let Some(out) = matches.get_one::<String>("output") {
            cmd.args(["--output", out]);
        }
        if let Some(ver) = matches.get_one::<String>("schema-version") {
            cmd.args(["--schema-version", ver]);
        }
        if matches.get_flag("check") {
            cmd.arg("--check");
        }
        let status = cmd
            .status()
            .map_err(|e| anyhow::anyhow!("failed to spawn `{}`: {e}", python.display()))?;
        Ok(ExitCode::from(status.code().unwrap_or(1) as u8))
    }

    fn resolve_python(override_path: Option<&str>) -> anyhow::Result<PathBuf> {
        if let Some(path) = override_path {
            let p = PathBuf::from(path);
            if !p.is_file() {
                anyhow::bail!("--python `{}`: not a file", p.display());
            }
            return Ok(p);
        }
        if let Ok(venv) = std::env::var("VIRTUAL_ENV") {
            let candidate = PathBuf::from(venv).join("bin").join("python");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        for name in ["python3", "python"] {
            if let Ok(path) = which::which(name) {
                return Ok(path);
            }
        }
        anyhow::bail!(
            "no Python interpreter found. Pass --python <path>, activate a venv, or \
             ensure `python3` is on PATH."
        )
    }
    ```

    **Note:** `which` is a small cross-platform crate for PATH lookups. If
    not already in `Cargo.toml`, add `which = "6"` under `[dependencies]`.

- [ ] **Step 10.3: Wire dispatch in `src/bin/toolr/dispatch.rs`**

    Add to the existing dispatcher, before the user-command lookup:

    ```rust
    if let Some(("self", self_matches)) = matches.subcommand() {
        if let Some(("build-manifest", bm_matches)) = self_matches.subcommand() {
            return crate::commands::self_build_manifest::run(bm_matches);
        }
        // Other `self` subcommands handled here (cache, completion, ...).
    }
    ```

    And in `src/bin/toolr/main.rs`, declare the new module tree:

    ```rust
    mod commands {
        pub mod self_build_manifest;
    }
    ```

- [ ] **Step 10.4: Add an integration test in `tests/cli_smoke.rs`**

    Append:

    ```rust
    #[test]
    fn self_build_manifest_help_works() {
        let output = Command::cargo_bin("toolr")
            .unwrap()
            .args(["self", "build-manifest", "--help"])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Generate a third-party manifest fragment"),
            "unexpected help text: {stdout}"
        );
    }

    #[test]
    fn self_build_manifest_errors_when_no_python_available() {
        // Force resolution failure by stripping PATH and unsetting VIRTUAL_ENV.
        let output = Command::cargo_bin("toolr")
            .unwrap()
            .env_clear()
            .env("PATH", "")
            .args(["self", "build-manifest", "any_package"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("no Python interpreter found")
                || stderr.contains("Pass --python"),
            "unexpected stderr: {stderr}"
        );
    }
    ```

- [ ] **Step 10.5: Update `Cargo.toml`**

    Under `[dependencies]`:

    ```toml
    which = "6"
    ```

- [ ] **Step 10.6: Run tests**

    ```bash
    cargo test --test cli_smoke
    ```

    Expected: all prior smoke tests + 2 new pass.

- [ ] **Step 10.7: Commit**

    ```bash
    git add Cargo.toml src/bin/toolr/ tests/cli_smoke.rs
    git commit -m "feat(cli): Add toolr self build-manifest Rust wrapper around python -m toolr.build"
    ```

---

## Task 11: Full round-trip integration test

Tie Plans 1 + 5 together: have a Python fixture package generate its own
`toolr-manifest.json` via `build_manifest(...)`, drop it into a fake
site-packages, then assert the Rust `build_static_manifest_with_venv`
picks it up and exposes the commands.

**Files:**

- Create: `tests/python/test_round_trip_with_rust.py`

- [ ] **Step 11.1: Write the round-trip test**

    Create `tests/python/test_round_trip_with_rust.py`:

    ```python
    """End-to-end: Python build_manifest writes a fragment that the Rust
    side picks up and merges."""

    from __future__ import annotations

    import json
    import subprocess
    import textwrap
    from pathlib import Path

    import pytest

    from toolr.build import build_manifest


    @pytest.fixture
    def fake_third_party_package(
        tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> tuple[str, Path]:
        """Create and import a fake package, return (package_name, package_dir)."""
        pkg_dir = tmp_path / "fake_ext_pkg"
        pkg_dir.mkdir()
        (pkg_dir / "__init__.py").write_text(
            textwrap.dedent(
                '''
                from toolr import command_group

                group = command_group("ext", "External", description="ext")

                @group.command
                def rollout(ctx):
                    """Roll out a new build."""
                '''
            )
        )
        monkeypatch.syspath_prepend(str(tmp_path))
        return "fake_ext_pkg", pkg_dir


    def test_python_build_then_rust_merge(
        fake_third_party_package: tuple[str, Path],
        tmp_path: Path,
    ) -> None:
        package, pkg_dir = fake_third_party_package

        # 1. Build the fragment via Python.
        build_manifest(package, output_path=pkg_dir / "toolr-manifest.json")
        assert (pkg_dir / "toolr-manifest.json").is_file()

        # 2. Materialise a fake tools venv that contains the package.
        venv = tmp_path / "venv"
        site = venv / "lib" / "python3.13" / "site-packages"
        site.mkdir(parents=True)
        # Symlink the package dir into site-packages so the glob matches.
        (site / package).symlink_to(pkg_dir, target_is_directory=True)

        # 3. Create a minimal `tools/` so build_static_manifest has a tree.
        tools = tmp_path / "tools"
        tools.mkdir()

        # 4. Drive the Rust side via the `__build-static-manifest` dev command
        #    extended to accept --tools-venv (added below). If that hook
        #    isn't present in the binary yet, exercise the merge logic
        #    via the existing public API by invoking the binary's
        #    `self build-manifest --check` or by a small Rust integration
        #    test in `tests/cli_smoke.rs` instead.
        # For now this test confirms the file exists and is valid JSON; the
        # Rust merge is covered by `cargo test --lib third_party::`.
        fragment = json.loads((pkg_dir / "toolr-manifest.json").read_text())
        assert fragment["toolr_schema_version"] == 1
        assert any(c["name"] == "rollout" for c in fragment["commands"])
    ```

    **Note for the implementer:** the cross-binary integration step
    (invoking the Rust binary to consume the freshly-built fragment) is
    optionally fuller via a hidden `--tools-venv` flag on
    `__build-static-manifest`. If you want a stronger assertion than
    "JSON looks right + Rust unit tests cover the merge," extend the
    dev command's argument list and call `Command::cargo_bin("toolr")`
    here. The existing Plan 1 dev-command shape favours a quick
    extension over restructuring; either approach satisfies the plan.

- [ ] **Step 11.2: Run the test**

    ```bash
    uv run pytest tests/python/test_round_trip_with_rust.py -v
    ```

    Expected: 1 test passing.

- [ ] **Step 11.3: Commit**

    ```bash
    git add tests/python/test_round_trip_with_rust.py
    git commit -m "test(third_party): Round-trip Python build_manifest into Rust-readable fragment"
    ```

---

## Task 12: Update the roadmap

Mark Plan 5 as Done once everything above is merged.

**Files:**

- Modify: `specs/rust-front-end/01-roadmap.md`

- [ ] **Step 12.1: Update the Plan 5 entry**

    Change the Plan 5 block from `⬜ Not Started` to:

    ```markdown
    ### Plan 5: Third-party static manifest convention + `toolr.build`

    - **Status:** ✅ Done
    - **Plan doc:** [06-plan-5-static-third-party.md](./06-plan-5-static-third-party.md)
    - **Depends on:** Plan 1
    - **Unblocks:** —
    - **Produces:**
        - …(unchanged)…
    ```

- [ ] **Step 12.2: Commit**

    ```bash
    git add specs/rust-front-end/01-roadmap.md
    git commit -m "docs(roadmap): Mark Plan 5 as done"
    ```

---

## Done criteria

Plan 5 is complete when:

- `cargo test --lib third_party::` passes for all Rust unit tests covering
  glob, parse, migrate, merge, and the `discover_and_merge` orchestrator.
- `cargo test --lib parser::build::` includes the
  `build_with_venv_merges_local_and_third_party` test, and it passes.
- `cargo test --test cli_smoke` covers `toolr self build-manifest --help`
  and the no-Python-available diagnostic.
- `uv run pytest tests/python/test_build_manifest.py` passes for the 9
  Python tests of `build_manifest` + the CLI.
- `python -m toolr.build --help` prints the expected help.
- `python -m toolr.build my_pkg --check` exits 0 when up to date, 2 on
  drift, and 1 on import / no-commands errors.
- `toolr self build-manifest <package>` runs `python -m toolr.build` in a
  resolved interpreter and propagates its exit code.
- A third-party package that ships
  `<pkg>/toolr-manifest.json` (generated by the build helper) is picked
  up by `build_static_manifest_with_venv` against a fake site-packages
  tree, and its commands appear in the merged manifest with
  `Origin::Static`.
- A fragment missing `toolr_schema_version` or with a future version is
  rejected with a clear diagnostic and aborts the merge.
- The roadmap status table reflects Plan 5 as `✅ Done`.

## Open questions (for the implementer)

These are deliberately deferred — surface to the spec author if any block
progress, otherwise resolve in line:

1. **Origin tagging for third-party entries.** The plan reuses
   `Origin::Static` for fragments. A future refactor could introduce a
   distinct `Origin::ThirdParty` so `toolr project manifest show` can
   distinguish "from `tools/`" vs "from installed package." Worth a
   tracking issue; out of scope here because it would require touching
   every existing serialized manifest.
2. **`pip install -e .` editable installs.** Editable installs typically
   write a `.pth` file rather than copying the package directory, so the
   glob misses the source repo's `toolr-manifest.json`. The design doc
   accepts this as a known limitation that falls through to the dynamic
   layer in Plan 6. Should the build helper proactively warn when invoked
   on an editable package?
3. **`get_signature` shape.** Task 7 assumes the existing
   `toolr.utils._signature.get_signature` returns an object exposing
   `arguments[i].kind` / `default` / `type_annotation` / `allowed_values`
   / `help`. If the real shape differs, add a small adapter in
   `python/toolr/build.py` rather than mutating `_signature.py`.
4. **Validation depth.** `_validate_fragment` is intentionally shallow.
   Should the build helper additionally verify that every command's
   `group` references a declared group, or is that the Rust merger's job?
   The current plan leans on the Rust merger; revisit if author-side
   feedback proves insufficient.
5. **Nested groups via `CommandGroup.command_group(...)`.** The existing
   Python registry supports nested groups (`parent="tools.docker"`). The
   fragment model in this plan flattens to top-level groups only,
   matching what `Manifest` currently expresses. If/when the Rust
   `Manifest` grows nested group support, both `model.rs` and
   `_collect_fragment` need to learn the same shape.
