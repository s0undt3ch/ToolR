<!-- rumdl-disable MD046 MD076 -->

# Plan 1: Rust Binary Skeleton + Static Manifest Layer

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Lint:** Plan docs nest fenced code inside list items for step-by-step
> structure. The `<!-- rumdl-disable MD046 MD076 -->` directive above turns
> off the code-block-style and list-item-spacing rules for this file only.

**Goal:** Stand up the `toolr` Rust binary with manifest-driven CLI parsing. At
the end, `toolr --help`, `toolr --version`, and `toolr <user-group>
[<command>] --help` all work, driven by a static manifest built from
`tools/**/*.py` via AST parsing. Execution of user commands is intentionally
not wired up — that's Plan 2.

**Architecture:** Add a new `[[bin]] name = "toolr"` target to the existing
`toolr-rust-utils` crate. The binary's modules live under `src/cli/`. Static
manifest generation uses `ruff_python_parser` for AST parsing and the existing
`docstrings` module for `Args:` extraction. clap subcommands are constructed
dynamically from the loaded manifest. Manifest is `tools/.toolr-manifest.json`,
JSON, with `schema_version`, `static_hash`, `dynamic_hash`, `groups`,
`commands`. A walk-up-from-cwd discovery routine locates the project root.

**Tech Stack:** Rust 2021, clap (derive feature), ruff_python_parser,
serde_json, blake3, anyhow, assert_cmd (for integration tests),
tempfile (test fixtures).

**Reading order in this plan:** Tasks build on each other. Don't skip ahead;
later tasks reference types defined in earlier ones.

---

## Task 1: Bootstrap the `toolr` binary target

Add a `[[bin]]` target to the existing crate. Verify a placeholder binary
builds and runs.

**Files:**

- Modify: `Cargo.toml`

- Create: `src/bin/toolr/main.rs`

- [ ] **Step 1.1: Add the `[[bin]]` target to `Cargo.toml`**

    Append to the top-level of `Cargo.toml`:

    ```toml
    [[bin]]
    name = "toolr"
    path = "src/bin/toolr/main.rs"
    ```

- [ ] **Step 1.2: Create the placeholder binary entrypoint**

    Create `src/bin/toolr/main.rs`:

    ```rust
    fn main() {
        println!("toolr (placeholder) — {}", env!("CARGO_PKG_VERSION"));
    }
    ```

- [ ] **Step 1.3: Verify the binary builds and runs**

    ```bash
    cargo build --bin toolr
    cargo run --bin toolr --quiet
    ```

    Expected output line: `toolr (placeholder) — 0.0.0`

- [ ] **Step 1.4: Commit**

    ```bash
    git add Cargo.toml src/bin/toolr/main.rs
    git commit -m "feat(rust): Add toolr binary target with placeholder main"
    ```

---

## Task 2: Add CLI scaffolding with clap

Wire up clap so `toolr --version` and `toolr --help` work via real argument
parsing rather than the placeholder println.

**Files:**

- Modify: `Cargo.toml`
- Create: `src/bin/toolr/cli.rs`
- Modify: `src/bin/toolr/main.rs`
- [ ] **Step 2.1: Add clap dependency**

    Add to `[dependencies]` in `Cargo.toml`:

    ```toml
    clap = { version = "4", features = ["derive", "env", "wrap_help"] }
    ```

- [ ] **Step 2.2: Create `src/bin/toolr/cli.rs` with the top-level CLI struct**

    ```rust
    use clap::Parser;

    #[derive(Parser, Debug)]
    #[command(
        name = "toolr",
        version,
        about = "In-project CLI tooling support",
        long_about = None,
        disable_help_subcommand = true,
    )]
    pub struct Cli {
        /// Increase verbosity.
        #[arg(short = 'd', long = "debug", global = true)]
        pub debug: bool,

        /// Suppress non-error output.
        #[arg(short = 'q', long = "quiet", global = true, conflicts_with = "debug")]
        pub quiet: bool,
    }

    impl Cli {
        pub fn parse_args() -> Self {
            <Self as Parser>::parse()
        }
    }
    ```

- [ ] **Step 2.3: Replace main with real CLI dispatch**

    Replace `src/bin/toolr/main.rs` contents:

    ```rust
    mod cli;

    use cli::Cli;

    fn main() {
        let _args = Cli::parse_args();
        // Subcommand dispatch lands in a later task.
        eprintln!("toolr: no user commands registered yet (manifest support comes in Task 15).");
        std::process::exit(0);
    }
    ```

- [ ] **Step 2.4: Verify --version and --help work**

    ```bash
    cargo run --bin toolr --quiet -- --version
    ```

    Expected: `toolr 0.0.0`

    ```bash
    cargo run --bin toolr --quiet -- --help
    ```

    Expected: clap-rendered help with `--debug`, `--quiet`, `--version`, `--help` flags listed.
- [ ] **Step 2.5: Commit**

    ```bash
    git add Cargo.toml src/bin/toolr/
    git commit -m "feat(rust): Wire toolr binary to clap for --help and --version"
    ```

---

## Task 3: Define the manifest data model

Create the serde-derived types that represent a loaded `tools/.toolr-manifest.json`.

**Files:**

- Create: `src/manifest/mod.rs`
- Create: `src/manifest/model.rs`
- Create: `src/manifest/tests.rs`
- Modify: `src/lib.rs`
- [ ] **Step 3.1: Expose a `manifest` module from `src/lib.rs`**

    Add to `src/lib.rs`:

    ```rust
    pub mod manifest;
    ```

- [ ] **Step 3.2: Create `src/manifest/mod.rs`**

    ```rust
    //! Toolr command manifest data model and IO.

    pub mod model;

    pub use model::{Argument, ArgumentKind, Command, Group, Manifest, Origin, SCHEMA_VERSION};

    #[cfg(test)]
    mod tests;
    ```

- [ ] **Step 3.3: Create `src/manifest/model.rs`**

    ```rust
    //! Serde-derived types representing a loaded manifest.

    use serde::{Deserialize, Serialize};

    /// Current manifest schema version. Bump on breaking format changes.
    pub const SCHEMA_VERSION: u32 = 1;

    /// Top-level manifest document.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Manifest {
        pub schema_version: u32,
        /// Hash over `tools/**/*.py` contents — used for fast freshness checks.
        pub static_hash: String,
        /// Hash over the installed package set (versions). Empty until Plan 6
        /// adds dynamic-layer support.
        #[serde(default)]
        pub dynamic_hash: String,
        pub groups: Vec<Group>,
        pub commands: Vec<Command>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Group {
        /// Lowercase group name (e.g. "ci").
        pub name: String,
        /// Short title shown in `--help`.
        pub title: String,
        /// Optional longer description.
        #[serde(default)]
        pub description: String,
        /// Where this group entry came from.
        pub origin: Origin,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Command {
        /// Lowercase command name (e.g. "generate-build-matrix").
        pub name: String,
        /// Parent group name.
        pub group: String,
        /// Module path used by the runner to import.
        pub module: String,
        /// Python function name within that module.
        pub function: String,
        /// First line of the docstring; used in `--help` summaries.
        #[serde(default)]
        pub summary: String,
        /// Full description (rest of the docstring after the first line).
        #[serde(default)]
        pub description: String,
        /// Ordered list of arguments.
        pub arguments: Vec<Argument>,
        /// Top-level imports recorded by the static parser (used by Plan 7).
        #[serde(default)]
        pub imports: Vec<String>,
        /// Where this command entry came from.
        pub origin: Origin,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Argument {
        pub name: String,
        pub kind: ArgumentKind,
        #[serde(default)]
        pub help: String,
        /// String-encoded default. `None` means required.
        #[serde(default)]
        pub default: Option<String>,
        /// Argument's type annotation as written in source (best-effort).
        #[serde(default)]
        pub type_annotation: Option<String>,
        /// For Literal[...] / Enum-backed args, the allowed value strings.
        #[serde(default)]
        pub allowed_values: Vec<String>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum ArgumentKind {
        Positional,
        Optional,
        Flag,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum Origin {
        Static,
        Dynamic,
    }
    ```

- [ ] **Step 3.4: Add the dependency to `Cargo.toml`**

    `serde` is already present. Add `serde_json` to `[dependencies]`:

    ```toml
    serde_json = "1"
    ```

- [ ] **Step 3.5: Write round-trip tests in `src/manifest/tests.rs`**

    ```rust
    use super::model::*;

    fn sample_manifest() -> Manifest {
        Manifest {
            schema_version: SCHEMA_VERSION,
            static_hash: "abc123".into(),
            dynamic_hash: "".into(),
            groups: vec![Group {
                name: "ci".into(),
                title: "CI utilities".into(),
                description: "CI related utilities.".into(),
                origin: Origin::Static,
            }],
            commands: vec![Command {
                name: "generate-build-matrix".into(),
                group: "ci".into(),
                module: "tools.ci".into(),
                function: "generate_build_matrix".into(),
                summary: "Generate a build matrix.".into(),
                description: "".into(),
                arguments: vec![],
                imports: vec!["packaging".into()],
                origin: Origin::Static,
            }],
        }
    }

    #[test]
    fn manifest_round_trips_through_json() {
        let m = sample_manifest();
        let json = serde_json::to_string_pretty(&m).expect("serialize");
        let back: Manifest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(m, back);
    }

    #[test]
    fn missing_optional_fields_default_to_empty() {
        // Minimal JSON should still deserialize.
        let json = r#"{
            "schema_version": 1,
            "static_hash": "h",
            "groups": [],
            "commands": []
        }"#;
        let m: Manifest = serde_json::from_str(json).expect("deserialize minimal");
        assert_eq!(m.schema_version, 1);
        assert!(m.dynamic_hash.is_empty());
    }
    ```

- [ ] **Step 3.6: Run tests**

    ```bash
    cargo test --lib manifest::
    ```

    Expected: 2 tests passing.
- [ ] **Step 3.7: Commit**

    ```bash
    git add Cargo.toml src/lib.rs src/manifest/
    git commit -m "feat(manifest): Add manifest data model with serde round-trip tests"
    ```

---

## Task 4: Manifest read and write

Implement `Manifest::from_file` and `Manifest::to_file`, including schema
version validation.

**Files:**

- Create: `src/manifest/io.rs`

- Modify: `src/manifest/mod.rs`

- Modify: `src/manifest/tests.rs`

- Modify: `Cargo.toml`

- [ ] **Step 4.1: Add anyhow + thiserror to `[dependencies]`**

    ```toml
    anyhow = "1"
    thiserror = "1"
    ```

- [ ] **Step 4.2: Write the failing test in `src/manifest/tests.rs`**

    Append:

    ```rust
    use super::io::{ManifestError, load_manifest, write_manifest};
    use tempfile::TempDir;

    #[test]
    fn write_then_load_round_trips() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".toolr-manifest.json");
        let m = sample_manifest();
        write_manifest(&path, &m).expect("write");
        let loaded = load_manifest(&path).expect("load");
        assert_eq!(m, loaded);
    }

    #[test]
    fn load_rejects_unknown_schema_version() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".toolr-manifest.json");
        std::fs::write(
            &path,
            r#"{"schema_version": 999, "static_hash": "h", "groups": [], "commands": []}"#,
        )
        .unwrap();
        let err = load_manifest(&path).expect_err("should reject");
        assert!(matches!(err, ManifestError::UnknownSchemaVersion(999)));
    }

    #[test]
    fn load_returns_not_found_when_missing() {
        let tmp = TempDir::new().unwrap();
        let err = load_manifest(&tmp.path().join("absent.json")).expect_err("should be missing");
        assert!(matches!(err, ManifestError::Io(_)));
    }
    ```

    Also add `tempfile = "3"` to `[dev-dependencies]` if not already present.

- [ ] **Step 4.3: Run and verify the tests FAIL**

    ```bash
    cargo test --lib manifest::tests::write_then_load_round_trips
    ```

    Expected: compile error (unresolved import `super::io`).

- [ ] **Step 4.4: Create `src/manifest/io.rs`**

    ```rust
    //! Read and write the on-disk manifest file.

    use std::fs;
    use std::path::Path;

    use thiserror::Error;

    use super::model::{Manifest, SCHEMA_VERSION};

    #[derive(Debug, Error)]
    pub enum ManifestError {
        #[error("I/O error: {0}")]
        Io(#[from] std::io::Error),
        #[error("JSON error: {0}")]
        Json(#[from] serde_json::Error),
        #[error("unknown manifest schema_version {0}; this toolr supports up to {}", SCHEMA_VERSION)]
        UnknownSchemaVersion(u32),
    }

    pub fn load_manifest(path: &Path) -> Result<Manifest, ManifestError> {
        let bytes = fs::read(path)?;
        let raw: serde_json::Value = serde_json::from_slice(&bytes)?;
        let version = raw
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        if version > SCHEMA_VERSION {
            return Err(ManifestError::UnknownSchemaVersion(version));
        }
        let manifest: Manifest = serde_json::from_value(raw)?;
        Ok(manifest)
    }

    pub fn write_manifest(path: &Path, manifest: &Manifest) -> Result<(), ManifestError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(manifest)?;
        fs::write(path, bytes)?;
        Ok(())
    }
    ```

- [ ] **Step 4.5: Expose the module**

    Update `src/manifest/mod.rs`:

    ```rust
    pub mod io;
    pub mod model;

    pub use io::{ManifestError, load_manifest, write_manifest};
    pub use model::{Argument, ArgumentKind, Command, Group, Manifest, Origin, SCHEMA_VERSION};

    #[cfg(test)]
    mod tests;
    ```

- [ ] **Step 4.6: Run tests, expect PASS**

    ```bash
    cargo test --lib manifest::
    ```

    Expected: 5 tests passing (2 prior + 3 new).

- [ ] **Step 4.7: Commit**

    ```bash
    git add Cargo.toml src/manifest/
    git commit -m "feat(manifest): Add manifest load/write with schema version validation"
    ```

---

## Task 5: Project root discovery

Walk upward from a starting directory to find the nearest `tools/` directory.
Returns the parent of `tools/` (the project root).

**Files:**

- Create: `src/discovery.rs`
- Create: `src/discovery/tests.rs`
- Modify: `src/lib.rs`
- [ ] **Step 5.1: Add the failing tests**

    Create `src/discovery.rs` first with a stub, then add `src/discovery/tests.rs`:

    Actually, since rust's `#[cfg(test)] mod tests;` lives inside the parent
    module file, write the tests inline in `src/discovery.rs` once the stub
    exists.

    ```rust
    //! Walk upward to locate the project root (parent of `tools/`).

    use std::path::{Path, PathBuf};

    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum DiscoveryError {
        #[error("no `tools/` directory found above {0}")]
        NotFound(PathBuf),
    }

    /// Walk up from `start` until a directory containing `tools/` is found.
    /// Returns that directory (the project root). The check stops at the
    /// filesystem root.
    pub fn discover_project_root(start: &Path) -> Result<PathBuf, DiscoveryError> {
        let mut current = start.to_path_buf();
        loop {
            if current.join("tools").is_dir() {
                return Ok(current);
            }
            if !current.pop() {
                return Err(DiscoveryError::NotFound(start.to_path_buf()));
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use tempfile::TempDir;

        #[test]
        fn finds_tools_in_current_dir() {
            let tmp = TempDir::new().unwrap();
            std::fs::create_dir(tmp.path().join("tools")).unwrap();
            let root = discover_project_root(tmp.path()).unwrap();
            assert_eq!(root, tmp.path());
        }

        #[test]
        fn finds_tools_in_ancestor() {
            let tmp = TempDir::new().unwrap();
            std::fs::create_dir(tmp.path().join("tools")).unwrap();
            let nested = tmp.path().join("a").join("b").join("c");
            std::fs::create_dir_all(&nested).unwrap();
            let root = discover_project_root(&nested).unwrap();
            assert_eq!(root, tmp.path());
        }

        #[test]
        fn returns_not_found_when_no_tools_dir_exists() {
            let tmp = TempDir::new().unwrap();
            let err = discover_project_root(tmp.path()).expect_err("should not find");
            assert!(matches!(err, DiscoveryError::NotFound(_)));
        }
    }
    ```

- [ ] **Step 5.2: Expose the module from `src/lib.rs`**

    Add:

    ```rust
    pub mod discovery;
    ```

- [ ] **Step 5.3: Run tests**

    ```bash
    cargo test --lib discovery::
    ```

    Expected: 3 tests passing.
- [ ] **Step 5.4: Commit**

    ```bash
    git add src/lib.rs src/discovery.rs
    git commit -m "feat(discovery): Walk upward to locate project root from cwd"
    ```

---

## Task 6: Hashing primitives

Implement `hash_tools_dir` that produces a stable `static_hash` over
`tools/**/*.py` contents. Used by both manifest freshness and Tab completion.

**Files:**

- Create: `src/hash.rs`
- Modify: `Cargo.toml`
- Modify: `src/lib.rs`
- [ ] **Step 6.1: Add blake3 + walkdir to `Cargo.toml`**

    ```toml
    blake3 = "1"
    walkdir = "2"
    ```

- [ ] **Step 6.2: Write `src/hash.rs` with failing tests**

    ```rust
    //! Stable hashing over `tools/**/*.py` content.

    use std::path::Path;

    use anyhow::Result;
    use blake3::Hasher;
    use walkdir::WalkDir;

    /// Hash all `*.py` files under `tools_dir`. Path order is deterministic
    /// (sorted) so the result is reproducible across runs and machines.
    pub fn hash_tools_dir(tools_dir: &Path) -> Result<String> {
        let mut paths: Vec<_> = WalkDir::new(tools_dir)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_type().is_file()
                    && e.path().extension().is_some_and(|x| x == "py")
            })
            .map(|e| e.into_path())
            .collect();
        paths.sort();

        let mut hasher = Hasher::new();
        for path in &paths {
            let rel = path
                .strip_prefix(tools_dir)
                .unwrap_or(path)
                .to_string_lossy();
            hasher.update(rel.as_bytes());
            hasher.update(b"\0");
            let bytes = std::fs::read(path)?;
            hasher.update(&(bytes.len() as u64).to_le_bytes());
            hasher.update(&bytes);
        }
        Ok(hasher.finalize().to_hex().to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use tempfile::TempDir;

        fn setup(files: &[(&str, &str)]) -> TempDir {
            let tmp = TempDir::new().unwrap();
            for (name, contents) in files {
                let path = tmp.path().join(name);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::write(path, contents).unwrap();
            }
            tmp
        }

        #[test]
        fn identical_trees_hash_identically() {
            let a = setup(&[("a.py", "x"), ("b/c.py", "y")]);
            let b = setup(&[("a.py", "x"), ("b/c.py", "y")]);
            assert_eq!(
                hash_tools_dir(a.path()).unwrap(),
                hash_tools_dir(b.path()).unwrap()
            );
        }

        #[test]
        fn different_content_hashes_differently() {
            let a = setup(&[("a.py", "x")]);
            let b = setup(&[("a.py", "y")]);
            assert_ne!(
                hash_tools_dir(a.path()).unwrap(),
                hash_tools_dir(b.path()).unwrap()
            );
        }

        #[test]
        fn ignores_non_py_files() {
            let a = setup(&[("a.py", "x"), ("readme.md", "ignored")]);
            let b = setup(&[("a.py", "x")]);
            assert_eq!(
                hash_tools_dir(a.path()).unwrap(),
                hash_tools_dir(b.path()).unwrap()
            );
        }
    }
    ```

- [ ] **Step 6.3: Expose the module**

    Add to `src/lib.rs`:

    ```rust
    pub mod hash;
    ```

- [ ] **Step 6.4: Run tests**

    ```bash
    cargo test --lib hash::
    ```

    Expected: 3 tests passing.
- [ ] **Step 6.5: Commit**

    ```bash
    git add Cargo.toml src/lib.rs src/hash.rs
    git commit -m "feat(hash): Deterministic hashing over tools/**/*.py contents"
    ```

---

## Task 7: AST parsing infrastructure

Wire `ruff_python_parser` to read a single Python file into an AST. This task
proves the dependency works; later tasks use the AST.

**Files:**

- Create: `src/parser/mod.rs`
- Create: `src/parser/tests.rs`
- Modify: `Cargo.toml`
- Modify: `src/lib.rs`
- [ ] **Step 7.1: Add `ruff_python_parser` to `Cargo.toml`**

    ```toml
    ruff_python_parser = "0.0.226"   # or the most recent version on crates.io
    ruff_python_ast = "0.0.226"
    ```

    **Note for the implementer:** `ruff_python_parser` versions track ruff releases. If the chosen version is yanked or unavailable, use the latest version where `ruff_python_parser::parse_module` exists. If neither parser is available, fall back to `rustpython-parser = "0.4"` and adapt the AST access patterns; the public API of this module (`parse_python_file`) must stay the same.
- [ ] **Step 7.2: Create `src/parser/mod.rs`**

    ```rust
    //! Static AST parsing of `tools/**/*.py` files.

    use std::path::Path;

    use anyhow::{Context, Result};
    use ruff_python_ast::ModModule;
    use ruff_python_parser::parse_module;

    /// Parse a single Python file and return its module AST.
    pub fn parse_python_file(path: &Path) -> Result<ModModule> {
        let source = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let parsed = parse_module(&source)
            .with_context(|| format!("parsing {}", path.display()))?;
        Ok(parsed.into_syntax())
    }

    #[cfg(test)]
    mod tests;
    ```

- [ ] **Step 7.3: Create `src/parser/tests.rs`**

    ```rust
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_tmp(source: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(source.as_bytes()).unwrap();
        f
    }

    #[test]
    fn parses_a_simple_module() {
        let f = write_tmp("x = 1\n");
        let module = parse_python_file(f.path()).expect("should parse");
        assert!(!module.body.is_empty());
    }

    #[test]
    fn returns_error_on_syntax_error() {
        let f = write_tmp("def broken(\n");
        let err = parse_python_file(f.path()).expect_err("should fail");
        assert!(err.to_string().contains("parsing"));
    }
    ```

- [ ] **Step 7.4: Expose the parser module**

    Add to `src/lib.rs`:

    ```rust
    pub mod parser;
    ```

- [ ] **Step 7.5: Run tests**

    ```bash
    cargo test --lib parser::
    ```

    Expected: 2 tests passing.
- [ ] **Step 7.6: Commit**

    ```bash
    git add Cargo.toml src/lib.rs src/parser/
    git commit -m "feat(parser): Wire ruff_python_parser for AST access"
    ```

---

## Task 8: Extract `command_group()` calls

Walk the AST, find module-level assignments where the RHS is a call to
`command_group(...)`, record the group definition.

**Files:**

- Create: `src/parser/groups.rs`

- Modify: `src/parser/mod.rs`

- [ ] **Step 8.1: Write failing tests in `src/parser/groups.rs`**

    ```rust
    //! Extract `group = command_group(...)` assignments from a module AST.

    use ruff_python_ast::{Expr, ExprCall, ModModule, Stmt, StmtAssign};

    use crate::manifest::{Group, Origin};

    /// A group binding extracted from source. The `var` is the Python local name
    /// that subsequent `@var.command` decorators reference.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct GroupBinding {
        pub var: String,
        pub group: Group,
    }

    /// Walk the module's top-level statements and collect group bindings.
    pub fn extract_groups(module: &ModModule, module_docstring: &str) -> Vec<GroupBinding> {
        let mut out = Vec::new();
        for stmt in &module.body {
            let Stmt::Assign(StmtAssign { targets, value, .. }) = stmt else {
                continue;
            };
            // Only handle `single_var = command_group(...)`.
            let Some(var_name) = single_name_target(targets) else {
                continue;
            };
            let Expr::Call(call) = value.as_ref() else {
                continue;
            };
            if !is_command_group_call(call) {
                continue;
            }
            let Some(binding) = parse_group_call(call, &var_name, module_docstring) else {
                continue;
            };
            out.push(binding);
        }
        out
    }

    fn single_name_target(targets: &[Expr]) -> Option<String> {
        if targets.len() != 1 {
            return None;
        }
        match &targets[0] {
            Expr::Name(n) => Some(n.id.to_string()),
            _ => None,
        }
    }

    fn is_command_group_call(call: &ExprCall) -> bool {
        match call.func.as_ref() {
            Expr::Name(n) => n.id.as_str() == "command_group",
            Expr::Attribute(a) => a.attr.as_str() == "command_group",
            _ => false,
        }
    }

    fn parse_group_call(call: &ExprCall, var: &str, module_docstring: &str) -> Option<GroupBinding> {
        // Positional args: name, title. Keyword `docstring` may be __doc__.
        let name = call.arguments.args.first().and_then(literal_str)?;
        let title = call.arguments.args.get(1).and_then(literal_str).unwrap_or_default();
        let description = call
            .arguments
            .keywords
            .iter()
            .find(|k| k.arg.as_ref().map(|n| n.as_str()) == Some("docstring"))
            .and_then(|k| match &k.value {
                Expr::Name(n) if n.id.as_str() == "__doc__" => Some(module_docstring.to_string()),
                e => literal_str(e),
            })
            .unwrap_or_default();
        Some(GroupBinding {
            var: var.to_string(),
            group: Group {
                name,
                title,
                description,
                origin: Origin::Static,
            },
        })
    }

    fn literal_str(expr: &Expr) -> Option<String> {
        match expr {
            Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
            _ => None,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::parser::parse_python_file;
        use tempfile::NamedTempFile;
        use std::io::Write;

        fn parse_src(src: &str) -> ruff_python_ast::ModModule {
            let mut f = NamedTempFile::new().unwrap();
            f.write_all(src.as_bytes()).unwrap();
            parse_python_file(f.path()).unwrap()
        }

        #[test]
        fn extracts_command_group_with_literal_args() {
            let src = r#"group = command_group("ci", "CI utilities")"#;
            let m = parse_src(src);
            let groups = extract_groups(&m, "");
            assert_eq!(groups.len(), 1);
            assert_eq!(groups[0].var, "group");
            assert_eq!(groups[0].group.name, "ci");
            assert_eq!(groups[0].group.title, "CI utilities");
        }

        #[test]
        fn resolves_docstring_keyword_to_module_doc() {
            let src = r#"group = command_group("ci", "CI utilities", docstring=__doc__)"#;
            let m = parse_src(src);
            let groups = extract_groups(&m, "module-level doc");
            assert_eq!(groups[0].group.description, "module-level doc");
        }

        #[test]
        fn ignores_non_command_group_assignments() {
            let src = r#"x = 1
y = some_other_func("ci")
"#;
            let m = parse_src(src);
            assert!(extract_groups(&m, "").is_empty());
        }
    }
    ```

- [ ] **Step 8.2: Re-export from `src/parser/mod.rs`**

    Add to `src/parser/mod.rs`:

    ```rust
    pub mod groups;
    pub use groups::{GroupBinding, extract_groups};
    ```

- [ ] **Step 8.3: Run tests**

    ```bash
    cargo test --lib parser::groups::
    ```

    Expected: 3 tests passing.

- [ ] **Step 8.4: Commit**

    ```bash
    git add src/parser/
    git commit -m "feat(parser): Extract command_group() calls from module ASTs"
    ```

---

## Task 9: Extract `@group.command` decorated functions

For every function decorated with `@<group-var>.command`, emit a command
record. Group binding from Task 8 is looked up by var name.

**Files:**

- Create: `src/parser/commands.rs`
- Modify: `src/parser/mod.rs`
- [ ] **Step 9.1: Write failing tests + implementation in `src/parser/commands.rs`**

    ```rust
    //! Extract `@<group>.command`-decorated function definitions.

    use std::collections::HashMap;

    use ruff_python_ast::{Decorator, Expr, ModModule, Stmt, StmtFunctionDef};

    use super::groups::GroupBinding;
    use crate::manifest::{Command, Origin};

    /// Walk module body for functions decorated with `@<var>.command` where
    /// `<var>` matches a known group binding. Emit one Command per match.
    pub fn extract_commands(
        module: &ModModule,
        module_path: &str,
        bindings: &[GroupBinding],
    ) -> Vec<Command> {
        let by_var: HashMap<&str, &str> = bindings
            .iter()
            .map(|b| (b.var.as_str(), b.group.name.as_str()))
            .collect();
        let mut out = Vec::new();
        for stmt in &module.body {
            let Stmt::FunctionDef(func) = stmt else {
                continue;
            };
            let Some(group_var) = command_decorator_target(&func.decorator_list) else {
                continue;
            };
            let Some(&group_name) = by_var.get(group_var.as_str()) else {
                continue;
            };
            out.push(build_command(func, group_name, module_path));
        }
        out
    }

    fn command_decorator_target(decorators: &[Decorator]) -> Option<String> {
        for d in decorators {
            // Match @<name>.command
            if let Expr::Attribute(attr) = &d.expression {
                if attr.attr.as_str() == "command" {
                    if let Expr::Name(n) = attr.value.as_ref() {
                        return Some(n.id.to_string());
                    }
                }
            }
        }
        None
    }

    fn build_command(func: &StmtFunctionDef, group: &str, module_path: &str) -> Command {
        // Argument extraction lands in Task 10; summary/description from
        // docstring lands in Task 11. For now, populate skeleton.
        Command {
            name: func.name.as_str().replace('_', "-"),
            group: group.to_string(),
            module: module_path.to_string(),
            function: func.name.as_str().to_string(),
            summary: String::new(),
            description: String::new(),
            arguments: Vec::new(),
            imports: Vec::new(),
            origin: Origin::Static,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::parser::groups::extract_groups;
        use crate::parser::parse_python_file;
        use tempfile::NamedTempFile;
        use std::io::Write;

        fn parse_src(src: &str) -> ruff_python_ast::ModModule {
            let mut f = NamedTempFile::new().unwrap();
            f.write_all(src.as_bytes()).unwrap();
            parse_python_file(f.path()).unwrap()
        }

        #[test]
        fn extracts_decorated_function_as_command() {
            let src = r#"group = command_group("ci", "CI utilities")

@group.command
def generate_build_matrix(ctx):
    pass
"#;
            let m = parse_src(src);
            let bindings = extract_groups(&m, "");
            let commands = extract_commands(&m, "tools.ci", &bindings);
            assert_eq!(commands.len(), 1);
            assert_eq!(commands[0].name, "generate-build-matrix");
            assert_eq!(commands[0].group, "ci");
            assert_eq!(commands[0].function, "generate_build_matrix");
            assert_eq!(commands[0].module, "tools.ci");
        }

        #[test]
        fn ignores_functions_with_unknown_group_var() {
            let src = r#"
@other.command
def x(ctx):
    pass
"#;
            let m = parse_src(src);
            let bindings = vec![];
            let commands = extract_commands(&m, "tools.x", &bindings);
            assert!(commands.is_empty());
        }

        #[test]
        fn ignores_undecorated_functions() {
            let src = r#"group = command_group("ci", "CI utilities")

def bare_function(ctx):
    pass
"#;
            let m = parse_src(src);
            let bindings = extract_groups(&m, "");
            let commands = extract_commands(&m, "tools.ci", &bindings);
            assert!(commands.is_empty());
        }
    }
    ```

- [ ] **Step 9.2: Re-export**

    Append to `src/parser/mod.rs`:

    ```rust
    pub mod commands;
    pub use commands::extract_commands;
    ```

- [ ] **Step 9.3: Run tests**

    ```bash
    cargo test --lib parser::commands::
    ```

    Expected: 3 tests passing.

- [ ] **Step 9.4: Commit**

    ```bash
    git add src/parser/
    git commit -m "feat(parser): Extract @group.command decorated functions"
    ```

---

## Task 10: Extract function signatures

Populate `Command.arguments` from the function parameter list. Skip the first
`ctx: Context` argument. Capture type annotations as raw strings (resolution
to allowed values comes in later tasks).

**Files:**

- Create: `src/parser/signatures.rs`
- Modify: `src/parser/commands.rs`
- Modify: `src/parser/mod.rs`
- [ ] **Step 10.1: Create `src/parser/signatures.rs` with tests**

    ```rust
    //! Extract function arguments from a `def` AST node.

    use ruff_python_ast::{Expr, Parameters, StmtFunctionDef};

    use crate::manifest::{Argument, ArgumentKind};

    /// Build the argument list for a command from its function definition.
    /// Skips the first parameter (assumed to be `ctx: Context`).
    pub fn extract_arguments(func: &StmtFunctionDef) -> Vec<Argument> {
        let Parameters { args, kwonlyargs, .. } = func.parameters.as_ref();
        // Skip ctx (first positional).
        let positional: Vec<_> = args.iter().skip(1).collect();
        let mut out = Vec::new();
        for p in positional {
            let kind = if p.default.is_some() {
                ArgumentKind::Optional
            } else {
                ArgumentKind::Positional
            };
            out.push(Argument {
                name: p.parameter.name.to_string(),
                kind,
                help: String::new(),
                default: p.default.as_ref().map(|d| literal_default(d)),
                type_annotation: p
                    .parameter
                    .annotation
                    .as_ref()
                    .map(|a| annotation_to_string(a)),
                allowed_values: Vec::new(),
            });
        }
        for p in kwonlyargs {
            out.push(Argument {
                name: p.parameter.name.to_string(),
                kind: ArgumentKind::Flag,
                help: String::new(),
                default: p.default.as_ref().map(|d| literal_default(d)),
                type_annotation: p
                    .parameter
                    .annotation
                    .as_ref()
                    .map(|a| annotation_to_string(a)),
                allowed_values: Vec::new(),
            });
        }
        out
    }

    fn literal_default(expr: &Expr) -> String {
        match expr {
            Expr::StringLiteral(s) => format!("\"{}\"", s.value.to_str()),
            Expr::NumberLiteral(n) => format!("{:?}", n.value),
            Expr::BooleanLiteral(b) => b.value.to_string(),
            Expr::NoneLiteral(_) => "None".to_string(),
            _ => "<expr>".to_string(),
        }
    }

    fn annotation_to_string(expr: &Expr) -> String {
        // Best-effort textual rendering. Detailed resolution lands in
        // Tasks 12-14.
        match expr {
            Expr::Name(n) => n.id.to_string(),
            Expr::Attribute(a) => format!("{}.{}", annotation_to_string(&a.value), a.attr),
            Expr::Subscript(s) => format!(
                "{}[{}]",
                annotation_to_string(&s.value),
                annotation_to_string(&s.slice)
            ),
            _ => "<expr>".to_string(),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::parser::parse_python_file;
        use ruff_python_ast::Stmt;
        use tempfile::NamedTempFile;
        use std::io::Write;

        fn first_func(src: &str) -> StmtFunctionDef {
            let mut f = NamedTempFile::new().unwrap();
            f.write_all(src.as_bytes()).unwrap();
            let m = parse_python_file(f.path()).unwrap();
            for stmt in m.body {
                if let Stmt::FunctionDef(f) = stmt {
                    return f;
                }
            }
            panic!("no function found");
        }

        #[test]
        fn skips_ctx_first_argument() {
            let func = first_func("def f(ctx, name): pass\n");
            let args = extract_arguments(&func);
            assert_eq!(args.len(), 1);
            assert_eq!(args[0].name, "name");
        }

        #[test]
        fn marks_arguments_with_defaults_as_optional() {
            let func = first_func("def f(ctx, name=\"x\"): pass\n");
            let args = extract_arguments(&func);
            assert_eq!(args[0].kind, ArgumentKind::Optional);
            assert_eq!(args[0].default.as_deref(), Some("\"x\""));
        }

        #[test]
        fn captures_type_annotations_as_strings() {
            let func = first_func("def f(ctx, name: str = \"x\"): pass\n");
            let args = extract_arguments(&func);
            assert_eq!(args[0].type_annotation.as_deref(), Some("str"));
        }
    }
    ```

- [ ] **Step 10.2: Wire signatures into `build_command`**

    Edit `src/parser/commands.rs`. Replace the `build_command` function:

    ```rust
    use super::signatures::extract_arguments;

    fn build_command(func: &StmtFunctionDef, group: &str, module_path: &str) -> Command {
        Command {
            name: func.name.as_str().replace('_', "-"),
            group: group.to_string(),
            module: module_path.to_string(),
            function: func.name.as_str().to_string(),
            summary: String::new(),
            description: String::new(),
            arguments: extract_arguments(func),
            imports: Vec::new(),
            origin: Origin::Static,
        }
    }
    ```

- [ ] **Step 10.3: Re-export**

    Add to `src/parser/mod.rs`:

    ```rust
    pub mod signatures;
    pub use signatures::extract_arguments;
    ```

- [ ] **Step 10.4: Run tests**

    ```bash
    cargo test --lib parser::
    ```

    Expected: all parser tests passing (3 + 3 + 3 from earlier tasks).
- [ ] **Step 10.5: Commit**

    ```bash
    git add src/parser/
    git commit -m "feat(parser): Extract function signatures into Command arguments"
    ```

---

## Task 11: Parse docstrings for summary + description + arg help

The existing `docstrings` module already parses Google-style docstrings. Reuse
it to populate `Command.summary`, `Command.description`, and per-argument help
text.

**Files:**

- Modify: `src/parser/commands.rs`

- Modify: `src/parser/tests.rs` (if present) or `src/parser/commands.rs` tests

- [ ] **Step 11.1: Inspect the existing docstrings API**

    ```bash
    grep -nE "pub (fn|struct)" src/docstrings.rs | head -40
    ```

    Identify the parse entrypoint (likely `pub fn parse(input: &str) -> ParsedDocstring` or similar) and the per-argument extraction. If the public API doesn't expose what we need, write a thin adapter inside this task — do **not** modify `src/docstrings.rs` itself.

- [ ] **Step 11.2: Add docstring extraction helper to `src/parser/commands.rs`**

    Above `build_command`:

    ```rust
    use ruff_python_ast::Stmt;

    fn function_docstring(func: &StmtFunctionDef) -> String {
        let Some(first) = func.body.first() else {
            return String::new();
        };
        let Stmt::Expr(e) = first else {
            return String::new();
        };
        let ruff_python_ast::Expr::StringLiteral(s) = e.value.as_ref() else {
            return String::new();
        };
        s.value.to_str().to_string()
    }
    ```

    Then update `build_command` to populate `summary`, `description`, and
    arg `help` fields. Call into the existing `docstrings` module:

    ```rust
    fn build_command(func: &StmtFunctionDef, group: &str, module_path: &str) -> Command {
        let raw_doc = function_docstring(func);
        let parsed = crate::docstrings::parse(&raw_doc); // adjust to real API
        let mut arguments = extract_arguments(func);
        for arg in &mut arguments {
            if let Some(help) = parsed.arg_help(&arg.name) {
                arg.help = help.to_string();
            }
        }
        Command {
            name: func.name.as_str().replace('_', "-"),
            group: group.to_string(),
            module: module_path.to_string(),
            function: func.name.as_str().to_string(),
            summary: parsed.summary().to_string(),
            description: parsed.description().to_string(),
            arguments,
            imports: Vec::new(),
            origin: Origin::Static,
        }
    }
    ```

    **Note:** The `parsed.summary()`, `parsed.description()`, and
    `parsed.arg_help(...)` accessors must exist on whatever the docstrings
    module exposes. If the existing API is different, add small adapter
    methods in this task, but keep the changes self-contained.

- [ ] **Step 11.3: Add tests in `src/parser/commands.rs::tests`**

    Append:

    ```rust
    #[test]
    fn populates_summary_from_first_docstring_line() {
        let src = r#"group = command_group("ci", "CI utilities")

@group.command
def hello(ctx):
    """Say hello."""
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "");
        let commands = extract_commands(&m, "tools.ci", &bindings);
        assert_eq!(commands[0].summary, "Say hello.");
    }

    #[test]
    fn populates_arg_help_from_args_section() {
        let src = r#"group = command_group("ci", "CI utilities")

@group.command
def hello(ctx, name="world"):
    """Say hello.

    Args:
        name: Who to greet.
    """
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "");
        let commands = extract_commands(&m, "tools.ci", &bindings);
        let name_arg = commands[0]
            .arguments
            .iter()
            .find(|a| a.name == "name")
            .unwrap();
        assert_eq!(name_arg.help, "Who to greet.");
    }
    ```

- [ ] **Step 11.4: Run tests**

    ```bash
    cargo test --lib parser::commands::
    ```

    Expected: all previous + 2 new tests passing.

- [ ] **Step 11.5: Commit**

    ```bash
    git add src/parser/
    git commit -m "feat(parser): Populate command summary, description, and arg help from docstrings"
    ```

---

## Task 12: Extract local `typing.Literal[...]` values

For arguments annotated `Literal["a", "b"]`, populate `Argument.allowed_values`
with `["a", "b"]`. Local-only resolution; cross-file is Task 14.

**Files:**

- Modify: `src/parser/signatures.rs`

- [ ] **Step 12.1: Add tests**

    Append to `signatures::tests`:

    ```rust
    #[test]
    fn extracts_literal_values() {
        let func = first_func(r#"
from typing import Literal
def f(ctx, mode: Literal["a", "b"]): pass
"#);
        let args = extract_arguments(&func);
        assert_eq!(args[0].allowed_values, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn leaves_allowed_values_empty_for_non_literal_types() {
        let func = first_func("def f(ctx, name: str): pass\n");
        let args = extract_arguments(&func);
        assert!(args[0].allowed_values.is_empty());
    }
    ```

- [ ] **Step 12.2: Implement Literal value extraction**

    Inside `signatures.rs`, after `annotation_to_string`:

    ```rust
    fn literal_values(annotation: &Expr) -> Vec<String> {
        let Expr::Subscript(sub) = annotation else {
            return Vec::new();
        };
        // The subscripted expression must be named "Literal".
        let is_literal = match sub.value.as_ref() {
            Expr::Name(n) => n.id.as_str() == "Literal",
            Expr::Attribute(a) => a.attr.as_str() == "Literal",
            _ => false,
        };
        if !is_literal {
            return Vec::new();
        }
        match sub.slice.as_ref() {
            Expr::Tuple(t) => t.elts.iter().filter_map(literal_str_value).collect(),
            other => literal_str_value(other).into_iter().collect(),
        }
    }

    fn literal_str_value(expr: &Expr) -> Option<String> {
        match expr {
            Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
            _ => None,
        }
    }
    ```

    And update the loop bodies to call `literal_values(&annotation)` and write
    the result into `allowed_values`:

    Inside `extract_arguments`, replace the `out.push(Argument { ... })` blocks
    so each one computes `let allowed = p.parameter.annotation.as_ref().map(|a| literal_values(a)).unwrap_or_default();` and passes that into the struct.

- [ ] **Step 12.3: Run tests**

    ```bash
    cargo test --lib parser::signatures::
    ```

    Expected: all signature tests pass, including the two new ones.

- [ ] **Step 12.4: Commit**

    ```bash
    git add src/parser/signatures.rs
    git commit -m "feat(parser): Extract typing.Literal values into allowed_values"
    ```

---

## Task 13: Extract local `enum.Enum` members

Build a symbol table over the parsed module's top-level class definitions.
For arguments annotated with a local enum, look up its members and use their
values as `allowed_values`.

**Files:**

- Create: `src/parser/symbols.rs`
- Modify: `src/parser/signatures.rs`
- Modify: `src/parser/mod.rs`
- [ ] **Step 13.1: Create `src/parser/symbols.rs`**

    ```rust
    //! Symbol table for resolving local type names to their declarations.

    use std::collections::HashMap;

    use ruff_python_ast::{Expr, ModModule, Stmt, StmtClassDef};

    /// Mapping of local class name → enum member values, for classes that
    /// look like an `Enum` subclass.
    #[derive(Debug, Default, Clone)]
    pub struct EnumTable {
        members: HashMap<String, Vec<String>>,
    }

    impl EnumTable {
        pub fn from_module(module: &ModModule) -> Self {
            let mut table = EnumTable::default();
            for stmt in &module.body {
                let Stmt::ClassDef(class) = stmt else {
                    continue;
                };
                if !is_enum_subclass(class) {
                    continue;
                }
                let values = class
                    .body
                    .iter()
                    .filter_map(member_value)
                    .collect::<Vec<_>>();
                if !values.is_empty() {
                    table.members.insert(class.name.to_string(), values);
                }
            }
            table
        }

        pub fn lookup(&self, name: &str) -> Option<&[String]> {
            self.members.get(name).map(|v| v.as_slice())
        }

        pub fn merge(&mut self, other: EnumTable) {
            self.members.extend(other.members);
        }
    }

    fn is_enum_subclass(class: &StmtClassDef) -> bool {
        let Some(args) = class.arguments.as_ref() else {
            return false;
        };
        args.args.iter().any(|base| matches_enum_name(base))
    }

    fn matches_enum_name(expr: &Expr) -> bool {
        let name = match expr {
            Expr::Name(n) => n.id.as_str(),
            Expr::Attribute(a) => a.attr.as_str(),
            _ => return false,
        };
        matches!(name, "Enum" | "IntEnum" | "StrEnum" | "Flag" | "IntFlag")
    }

    fn member_value(stmt: &Stmt) -> Option<String> {
        let Stmt::Assign(a) = stmt else {
            return None;
        };
        let Expr::StringLiteral(s) = a.value.as_ref() else {
            // Only string-valued enums are usable for argument value
            // completion. Int enums and auto() are recorded as member names
            // for the user's awareness; richer extraction is future work.
            if let Expr::Name(_) | Expr::Call(_) = a.value.as_ref() {
                if a.targets.len() == 1 {
                    if let Expr::Name(t) = &a.targets[0] {
                        return Some(t.id.to_string());
                    }
                }
            }
            return None;
        };
        Some(s.value.to_str().to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::parser::parse_python_file;
        use tempfile::NamedTempFile;
        use std::io::Write;

        fn parse(src: &str) -> ModModule {
            let mut f = NamedTempFile::new().unwrap();
            f.write_all(src.as_bytes()).unwrap();
            parse_python_file(f.path()).unwrap()
        }

        #[test]
        fn collects_string_enum_values() {
            let src = r#"
from enum import StrEnum

class Mode(StrEnum):
    FAST = "fast"
    SLOW = "slow"
"#;
            let table = EnumTable::from_module(&parse(src));
            let vals = table.lookup("Mode").unwrap();
            assert_eq!(vals, &["fast".to_string(), "slow".to_string()]);
        }

        #[test]
        fn ignores_non_enum_classes() {
            let src = r#"
class Foo:
    X = "x"
"#;
            let table = EnumTable::from_module(&parse(src));
            assert!(table.lookup("Foo").is_none());
        }
    }
    ```

- [ ] **Step 13.2: Plumb the table through `extract_arguments`**

    Update `src/parser/signatures.rs`:

    Add an `EnumTable` parameter so callers can pass the symbol table:

    ```rust
    use super::symbols::EnumTable;

    pub fn extract_arguments(func: &StmtFunctionDef, enums: &EnumTable) -> Vec<Argument> {
        // …same logic as before, but compute allowed_values as:
        //   let mut allowed = literal_values(&annotation);
        //   if allowed.is_empty() {
        //       if let Some(name) = referenced_name(&annotation) {
        //           if let Some(vals) = enums.lookup(name) {
        //               allowed = vals.to_vec();
        //           }
        //       }
        //   }
    }

    fn referenced_name(expr: &Expr) -> Option<&str> {
        match expr {
            Expr::Name(n) => Some(n.id.as_str()),
            _ => None,
        }
    }
    ```

    Update tests that called `extract_arguments(&func)` to pass
    `&EnumTable::default()`.

- [ ] **Step 13.3: Update `build_command` to pass an `EnumTable`**

    `build_command` now takes an `&EnumTable` argument too. Same for
    `extract_commands`. Update callers (within parser tests).

- [ ] **Step 13.4: Run tests**

    ```bash
    cargo test --lib parser::
    ```

    Expected: all tests passing.

- [ ] **Step 13.5: Re-export**

    Add to `src/parser/mod.rs`:

    ```rust
    pub mod symbols;
    pub use symbols::EnumTable;
    ```

- [ ] **Step 13.6: Commit**

    ```bash
    git add src/parser/
    git commit -m "feat(parser): Resolve local enum.Enum members for allowed_values"
    ```

---

## Task 14: Top-level static manifest builder

Tie everything together. Walk `tools/**/*.py`, parse each, build a unified
`EnumTable` across files, extract groups + commands, compute `static_hash`,
return a `Manifest`.

**Files:**

- Create: `src/parser/build.rs`
- Modify: `src/parser/mod.rs`
- [ ] **Step 14.1: Create `src/parser/build.rs`**

    ```rust
    //! Build a complete static `Manifest` from a `tools/` directory.

    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    use anyhow::Result;
    use walkdir::WalkDir;

    use crate::hash::hash_tools_dir;
    use crate::manifest::{Manifest, SCHEMA_VERSION};
    use crate::parser::{
        commands::extract_commands, groups::extract_groups, parse_python_file, symbols::EnumTable,
    };

    /// Build the static portion of a manifest from a tools directory.
    pub fn build_static_manifest(tools_dir: &Path) -> Result<Manifest> {
        let py_files = list_python_files(tools_dir);

        // Pass 1: build cross-file enum table from every module.
        let mut enums = EnumTable::default();
        for path in &py_files {
            let module = parse_python_file(path)?;
            enums.merge(EnumTable::from_module(&module));
        }

        // Pass 2: extract groups + commands per file using the merged table.
        let mut all_groups = Vec::new();
        let mut all_commands = Vec::new();
        let mut seen_groups = HashSet::new();
        for path in &py_files {
            let module = parse_python_file(path)?;
            let module_path = module_path_for(tools_dir, path);
            let module_doc = module_docstring(&module);
            let bindings = extract_groups(&module, &module_doc);
            let commands = extract_commands(&module, &module_path, &bindings, &enums);
            for binding in bindings {
                if seen_groups.insert(binding.group.name.clone()) {
                    all_groups.push(binding.group);
                }
            }
            all_commands.extend(commands);
        }

        let static_hash = hash_tools_dir(tools_dir)?;
        Ok(Manifest {
            schema_version: SCHEMA_VERSION,
            static_hash,
            dynamic_hash: String::new(),
            groups: all_groups,
            commands: all_commands,
        })
    }

    fn list_python_files(tools_dir: &Path) -> Vec<PathBuf> {
        let mut paths: Vec<_> = WalkDir::new(tools_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "py"))
            .map(|e| e.into_path())
            .collect();
        paths.sort();
        paths
    }

    fn module_path_for(tools_dir: &Path, file: &Path) -> String {
        let rel = file.strip_prefix(tools_dir).unwrap_or(file);
        let mut parts: Vec<String> = rel
            .with_extension("")
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        if parts.last().map(String::as_str) == Some("__init__") {
            parts.pop();
        }
        let mut out = String::from("tools");
        for p in parts {
            out.push('.');
            out.push_str(&p);
        }
        out
    }

    fn module_docstring(module: &ruff_python_ast::ModModule) -> String {
        use ruff_python_ast::Stmt;
        let Some(Stmt::Expr(e)) = module.body.first() else {
            return String::new();
        };
        if let ruff_python_ast::Expr::StringLiteral(s) = e.value.as_ref() {
            return s.value.to_str().to_string();
        }
        String::new()
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use tempfile::TempDir;

        fn write(tmp: &Path, name: &str, contents: &str) {
            let path = tmp.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, contents).unwrap();
        }

        #[test]
        fn builds_manifest_from_single_tools_file() {
            let tmp = TempDir::new().unwrap();
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
            let m = build_static_manifest(&tmp.path().join("tools")).unwrap();
            assert_eq!(m.schema_version, SCHEMA_VERSION);
            assert_eq!(m.groups.len(), 1);
            assert_eq!(m.groups[0].name, "ci");
            assert_eq!(m.commands.len(), 1);
            assert_eq!(m.commands[0].name, "hello");
            assert!(!m.static_hash.is_empty());
        }
    }
    ```

- [ ] **Step 14.2: Update `extract_commands` signature**

    The new `build.rs` calls `extract_commands(&module, &module_path, &bindings, &enums)`. Adjust the signature in `src/parser/commands.rs` to accept `&EnumTable` and thread it into `build_command`/`extract_arguments`.

- [ ] **Step 14.3: Re-export**

    Add to `src/parser/mod.rs`:

    ```rust
    pub mod build;
    pub use build::build_static_manifest;
    ```

- [ ] **Step 14.4: Run all tests**

    ```bash
    cargo test --lib
    ```

    Expected: all manifest, discovery, hash, and parser tests pass.

- [ ] **Step 14.5: Commit**

    ```bash
    git add src/parser/
    git commit -m "feat(parser): Build complete static manifest from tools directory"
    ```

---

## Task 15: Wire manifest into clap subcommands

Load the manifest at startup, build clap subcommands dynamically from it, and
dispatch on the parsed argv. Use `clap::Command::new` (the builder API) rather
than `derive`, because the subcommand set is data-driven.

**Files:**

- Create: `src/bin/toolr/dispatch.rs`

- Modify: `src/bin/toolr/cli.rs`

- Modify: `src/bin/toolr/main.rs`

- [ ] **Step 15.1: Rebuild the CLI as a builder so we can attach subcommands**

    Replace `src/bin/toolr/cli.rs`:

    ```rust
    use clap::{Arg, ArgAction, ArgMatches, Command};

    use _rust_utils::manifest::{Manifest, Origin};

    /// Construct the full clap Command tree, given a loaded manifest.
    /// User-defined groups appear as top-level subcommands.
    pub fn build_command(manifest: &Manifest) -> Command {
        let mut root = Command::new("toolr")
            .version(env!("CARGO_PKG_VERSION"))
            .about("In-project CLI tooling support")
            .disable_help_subcommand(true)
            .arg(
                Arg::new("debug")
                    .short('d')
                    .long("debug")
                    .action(ArgAction::SetTrue)
                    .global(true)
                    .help("Increase verbosity"),
            )
            .arg(
                Arg::new("quiet")
                    .short('q')
                    .long("quiet")
                    .action(ArgAction::SetTrue)
                    .global(true)
                    .conflicts_with("debug")
                    .help("Suppress non-error output"),
            );

        // User-defined groups.
        for group in &manifest.groups {
            let mut g = Command::new(group.name.clone())
                .about(group.title.clone());
            if !group.description.is_empty() {
                g = g.long_about(group.description.clone());
            }
            for cmd in manifest.commands.iter().filter(|c| c.group == group.name) {
                g = g.subcommand(build_user_command(cmd));
            }
            root = root.subcommand(g);
        }

        root
    }

    fn build_user_command(cmd: &_rust_utils::manifest::Command) -> Command {
        let mut c = Command::new(cmd.name.clone()).about(cmd.summary.clone());
        if !cmd.description.is_empty() {
            c = c.long_about(cmd.description.clone());
        }
        for arg in &cmd.arguments {
            let mut a = Arg::new(arg.name.clone()).help(arg.help.clone());
            match arg.kind {
                _rust_utils::manifest::ArgumentKind::Positional => {
                    a = a.required(true);
                }
                _rust_utils::manifest::ArgumentKind::Optional => {
                    a = a.long(arg.name.clone()).required(false);
                    if let Some(def) = &arg.default {
                        a = a.default_value(def.clone());
                    }
                }
                _rust_utils::manifest::ArgumentKind::Flag => {
                    a = a.long(arg.name.clone()).action(ArgAction::SetTrue);
                }
            }
            if !arg.allowed_values.is_empty() {
                a = a.value_parser(arg.allowed_values.clone());
            }
            c = c.arg(a);
        }
        c
    }
    ```

- [ ] **Step 15.2: Replace `src/bin/toolr/main.rs`**

    ```rust
    mod cli;
    mod dispatch;

    use std::path::PathBuf;
    use std::process::ExitCode;

    use _rust_utils::discovery::discover_project_root;
    use _rust_utils::manifest::{Manifest, load_manifest};

    fn main() -> ExitCode {
        match run() {
            Ok(code) => code,
            Err(e) => {
                eprintln!("toolr: {e:#}");
                ExitCode::from(2)
            }
        }
    }

    fn run() -> anyhow::Result<ExitCode> {
        let cwd = std::env::current_dir()?;
        let manifest = load_or_empty(&cwd);
        let mut command = cli::build_command(&manifest);
        let matches = command.clone().get_matches();
        dispatch::dispatch(&matches, &manifest, &mut command)
    }

    fn load_or_empty(cwd: &std::path::Path) -> Manifest {
        let Ok(root) = discover_project_root(cwd) else {
            return empty_manifest();
        };
        let manifest_path = root.join("tools").join(".toolr-manifest.json");
        load_manifest(&manifest_path).unwrap_or_else(|_| empty_manifest())
    }

    fn empty_manifest() -> Manifest {
        Manifest {
            schema_version: _rust_utils::manifest::SCHEMA_VERSION,
            static_hash: String::new(),
            dynamic_hash: String::new(),
            groups: Vec::new(),
            commands: Vec::new(),
        }
    }
    ```

- [ ] **Step 15.3: Create `src/bin/toolr/dispatch.rs` with the stub dispatcher**

    ```rust
    use std::process::ExitCode;

    use clap::ArgMatches;
    use _rust_utils::manifest::Manifest;

    pub fn dispatch(
        matches: &ArgMatches,
        manifest: &Manifest,
        root: &mut clap::Command,
    ) -> anyhow::Result<ExitCode> {
        let Some((group_name, group_matches)) = matches.subcommand() else {
            root.print_help()?;
            return Ok(ExitCode::SUCCESS);
        };
        let Some((cmd_name, _)) = group_matches.subcommand() else {
            // toolr <group> with no command → print group help
            return Ok(ExitCode::SUCCESS);
        };
        let cmd = manifest
            .commands
            .iter()
            .find(|c| c.group == group_name && c.name == cmd_name)
            .ok_or_else(|| anyhow::anyhow!("unknown command: {group_name} {cmd_name}"))?;

        // Plan 2 wires this up to a Python subprocess.
        eprintln!(
            "toolr: execution backend not yet implemented (would run {}/{}). \
             See specs/rust-front-end/03-plan-2-runner-execute.md.",
            cmd.group, cmd.name
        );
        Ok(ExitCode::from(64))
    }
    ```

- [ ] **Step 15.4: Add an integration test**

    Create `tests/cli_smoke.rs`:

    ```rust
    use assert_cmd::Command;
    use tempfile::TempDir;

    fn fixture_with_manifest(json: &str) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        std::fs::create_dir(&tools).unwrap();
        std::fs::write(tools.join(".toolr-manifest.json"), json).unwrap();
        tmp
    }

    #[test]
    fn version_flag_works_with_no_project() {
        let tmp = TempDir::new().unwrap();
        Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .arg("--version")
            .assert()
            .success();
    }

    #[test]
    fn help_lists_groups_from_manifest() {
        let json = r#"{
            "schema_version": 1,
            "static_hash": "h",
            "dynamic_hash": "",
            "groups": [
                {"name": "ci", "title": "CI utilities", "description": "", "origin": "static"}
            ],
            "commands": [
                {
                    "name": "hello", "group": "ci", "module": "tools.ci",
                    "function": "hello", "summary": "Say hello.",
                    "description": "", "arguments": [], "imports": [],
                    "origin": "static"
                }
            ]
        }"#;
        let tmp = fixture_with_manifest(json);
        let output = Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .arg("--help")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("ci"), "expected ci group in help, got:\n{stdout}");
    }

    #[test]
    fn running_a_user_command_emits_not_implemented_stub() {
        let json = r#"{
            "schema_version": 1, "static_hash": "h", "dynamic_hash": "",
            "groups": [{"name": "ci", "title": "CI", "description": "", "origin": "static"}],
            "commands": [{
                "name": "hello", "group": "ci", "module": "tools.ci",
                "function": "hello", "summary": "", "description": "",
                "arguments": [], "imports": [], "origin": "static"
            }]
        }"#;
        let tmp = fixture_with_manifest(json);
        let output = Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .args(["ci", "hello"])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(64));
        assert!(stderr.contains("execution backend not yet implemented"));
    }
    ```

    Add `assert_cmd = "2"` to `[dev-dependencies]` in `Cargo.toml`.

- [ ] **Step 15.5: Run the smoke tests**

    ```bash
    cargo test --test cli_smoke
    ```

    Expected: 3 tests passing.

- [ ] **Step 15.6: Commit**

    ```bash
    git add Cargo.toml src/bin/toolr/ tests/cli_smoke.rs
    git commit -m "feat(cli): Build subcommand tree from manifest and stub execution"
    ```

---

## Task 16: `toolr build-manifest` developer command

Add a temporary developer-facing command — `toolr __build-static-manifest` —
that runs the static builder and writes the result to
`tools/.toolr-manifest.json`. This is not in the final CLI surface (Plan 6
introduces `toolr project manifest rebuild` for the dynamic+static rebuild)
but is essential for Plan 1's end-to-end test.

**Files:**

- Modify: `src/bin/toolr/cli.rs`

- Modify: `src/bin/toolr/dispatch.rs`

- [ ] **Step 16.1: Add the hidden subcommand**

    In `src/bin/toolr/cli.rs`, after constructing `root`, add:

    ```rust
    root = root.subcommand(
        Command::new("__build-static-manifest")
            .hide(true)
            .about("(internal) Regenerate the static manifest in place"),
    );
    ```

- [ ] **Step 16.2: Handle dispatch**

    In `src/bin/toolr/dispatch.rs`, before the `find` step:

    ```rust
    if let Some(("__build-static-manifest", _)) = matches.subcommand() {
        return run_build_static_manifest();
    }
    ```

    And add:

    ```rust
    fn run_build_static_manifest() -> anyhow::Result<ExitCode> {
        let cwd = std::env::current_dir()?;
        let root = _rust_utils::discovery::discover_project_root(&cwd)?;
        let tools = root.join("tools");
        let manifest = _rust_utils::parser::build_static_manifest(&tools)?;
        let path = tools.join(".toolr-manifest.json");
        _rust_utils::manifest::write_manifest(&path, &manifest)?;
        println!(
            "toolr: wrote {} groups / {} commands to {}",
            manifest.groups.len(),
            manifest.commands.len(),
            path.display()
        );
        Ok(ExitCode::SUCCESS)
    }
    ```

- [ ] **Step 16.3: Manual end-to-end smoke test against the real `tools/ci.py`**

    ```bash
    cargo build --bin toolr --release
    ./target/release/toolr __build-static-manifest
    cat tools/.toolr-manifest.json | jq '.groups[].name, .commands[].name'
    ./target/release/toolr --help
    ./target/release/toolr ci --help
    ```

    Expected:

    - The manifest file is written.
    - `.groups[].name` includes `"ci"`.
    - `.commands[].name` includes the names of the functions in `tools/ci.py`.
    - `toolr --help` lists the `ci` group.
    - `toolr ci --help` lists the `ci` commands.

- [ ] **Step 16.4: Commit**

    ```bash
    git add src/bin/toolr/
    git commit -m "feat(cli): Add hidden __build-static-manifest dev command"
    ```

---

## Task 17: Update the roadmap

Mark Plan 1 as Done in the roadmap once everything above is merged.

**Files:**

- Modify: `specs/rust-front-end/01-roadmap.md`

- [ ] **Step 17.1: Update the Plan 1 entry**

    Change `### Plan 1: Rust binary skeleton + static manifest layer` block:

    ```markdown
    ### Plan 1: Rust binary skeleton + static manifest layer

    - **Status:** ✅ Done
    - **Plan doc:** [02-plan-1-rust-skeleton.md](./02-plan-1-rust-skeleton.md)
    - **Depends on:** —
    - **Unblocks:** Plans 2, 4, 5
    - **Produces:**
        - …(unchanged)…
    ```

- [ ] **Step 17.2: Commit**

    ```bash
    git add specs/rust-front-end/01-roadmap.md
    git commit -m "docs(roadmap): Mark Plan 1 as done"
    ```

---

## Done criteria

Plan 1 is complete when:

- `cargo test` passes for all unit and integration tests.
- `cargo run --bin toolr -- --help` lists user-defined groups discovered from
  `tools/`.
- `cargo run --bin toolr -- <group> --help` lists the group's commands.
- `cargo run --bin toolr -- <group> <command> --help` shows the command's
  arguments with help text drawn from docstrings.
- Running a real user command exits with code 64 and the stub message,
  matching what Plan 2 will replace.
- The manifest committed at `tools/.toolr-manifest.json` is regenerable via
  `toolr __build-static-manifest` and stable across runs (same input → same
  hash → identical JSON).
- The roadmap status table reflects Plan 1 as `✅ Done`.

## Open questions (for the implementer)

These are deliberately deferred — surface to the spec author if any block
progress, otherwise resolve in line:

1. `ruff_python_parser` version selection. The plan pins `0.0.226` as a
   conservative anchor. The implementer should bump to the latest stable
   release that still exposes `parse_module(&str) -> Result<Parsed<ModModule>>`
   on crates.io.
2. The existing `docstrings` module's exact public API may differ from the
   stub used in Task 11 (`parsed.summary()`, `parsed.arg_help(...)`). If so,
   add small adapter methods inside `parser/commands.rs` — do not modify
   `src/docstrings.rs` itself in this plan.
3. The crate's `[lib]` is currently `cdylib + rlib` for the Python extension.
   Adding `[[bin]]` should coexist cleanly with maturin, but the implementer
   should `maturin develop` once after Task 1 to confirm the wheel still
   builds.
4. The lib's import name is `_rust_utils` (driven by maturin's
   `module-name = "toolr.utils._rust_utils"`). The plan's binary code uses
   that name directly (`use _rust_utils::manifest::Manifest;`), which is
   visually awkward. A follow-up cleanup could rename `[lib] name` to
   `toolr_rust_utils` and update maturin's `module-name` to match — out of
   scope for Plan 1, but worth a tracking issue.
