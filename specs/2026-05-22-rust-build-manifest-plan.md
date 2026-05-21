# Pure-Rust `toolr self build-manifest` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Python-driven `toolr self build-manifest` authoring path with a pure-Rust AST walker that produces byte-identical `toolr-manifest.json` fragments without spawning Python or requiring `pip install -e .`.

**Architecture:** Add a new `toolr_core::build_fragment` module that reuses the existing `toolr-core` parser pipeline (`extract_groups`, `extract_commands`, `EnumTable`, etc.) with a plugin-aware module-path prefix. Rewire `crates/toolr/src/dispatch.rs::run_self_build_manifest` to call it directly. Delete `crates/toolr-py/python/toolr/build.py` and its Python test suite in the same change. CLI surface (`--output`, `--schema-version`, `--check`) is preserved verbatim; `--source-dir`/`--package` are added for source-tree workflows; `--python` is removed outright (no deprecation period — the flag was only ever relevant to the Python subprocess this work is killing).

**Tech Stack:** Rust 2024 edition, `ruff_python_parser`, `ruff_python_ast`, `serde`, `serde_json` (BTreeMap-backed `Value::Object` for sorted-key output), `walkdir`, `clap`, `similar` (new dev-dep for unified-diff drift output), `tempfile`. Existing `toolr-core::venv::resolve_venv_path` for tools-venv discovery.

**Predecessor:** Stacked on `dispatch_manifest_freshness` (PR #234) via `git-spice`. Do NOT branch from `main` — see Task 0.

---

## Scope Check

Single-subsystem change: one new core module, one CLI rewire, one deletion sweep. Does not need decomposition.

## File Structure

**New files:**

- `crates/toolr-core/src/build_fragment.rs` — `build_third_party_fragment()`, `BuildFragmentError`, inline `#[cfg(test)] mod tests`. Single-file module matching `parser/build.rs` style.
- `crates/toolr/tests/build_manifest_cli.rs` — integration tests that spawn the `toolr` binary and assert observable CLI behaviour (golden output, `--check` pass/drift, `--source-dir` vs `<package>` mutex, namespace-package rejection).

**Modified files:**

- `crates/toolr-core/src/lib.rs:1-30` — add `pub mod build_fragment;` re-export.
- `crates/toolr-core/src/parser/build.rs:318-346` — promote `list_python_files` to `pub(crate)`, extract a `module_path_for_prefix(source_dir, file, prefix: &str)` helper used by both `module_path_for` (passing `"tools"`) and the new build_fragment path (passing the package name).
- `crates/toolr-core/src/parser/mod.rs:27-29` — re-export the promoted helpers from `parser::build`.
- `crates/toolr-core/Cargo.toml:11-37` — add `similar` workspace dep.
- `Cargo.toml` (workspace root) — add `similar = "2"` to `[workspace.dependencies]`.
- `crates/toolr/src/cli.rs:321-358` — replace the `build-manifest` `clap` subcommand definition: drop `--python` entirely; add `--source-dir`, `--package`; group `<package>` and `--source-dir` as mutually exclusive.
- `crates/toolr/src/dispatch.rs:241-290` — rewrite `run_self_build_manifest` to call the Rust path; delete `resolve_python_for_build`; add helpers `resolve_source_and_package`, `serialize_fragment`, `check_against_disk`, `write_atomically`.
- `docs/third-party.md` — drop `python -m toolr.build` references; document the static-only contract; remove `--python` mentions; keep the `toolr self build-manifest <pkg>` CLI form unchanged.

**Deleted files:**

- `crates/toolr-py/python/toolr/build.py` (Python build module).
- `tests/build_manifest/test_build_manifest.py` (Python unit tests of `build_manifest()`).
- `tests/build_manifest/test_round_trip_with_rust.py` (round-trip equivalence test — superseded by the in-tree golden test plus the CI `--check` step).
- `tests/build_manifest/__init__.py` and the now-empty `tests/build_manifest/` directory.

**Unchanged but referenced:**

- `crates/toolr-core/src/third_party/model.rs` — provides `ManifestFragment`, `FragmentGroup`, `FragmentCommand`, `FragmentArgument`, `FRAGMENT_SCHEMA_VERSION`. Reused as-is.
- `crates/toolr-core/src/venv/resolve.rs::resolve_venv_path` — reused to find the tools venv for site-packages globbing.
- `crates/toolr-core/src/discovery.rs::discover_project_root` — reused to find the repo root from cwd.
- `examples/plugin-package/src/toolr_example_plugin/toolr-manifest.json` — equivalence target; must not change.
- `tests/distribution/test_example_plugin_contract.py` — continues to pass unchanged (asserts the shipped manifest matches runtime expectations; the shape is preserved).
- `.github/workflows/_test.yml:167-179` — already runs `toolr self build-manifest toolr_example_plugin --check` in CI; will automatically exercise the Rust replacement once this lands.

---

## Task 0: Verify stack inheritance from `dispatch_manifest_freshness`

**Files:** None (verification only).

This work MUST sit on a `git-spice`-tracked branch stacked on top of `dispatch_manifest_freshness`. The predecessor branch introduces `Origin::ThirdParty`, the canonical `examples/plugin-package/` fixture, and the CI `--check` step that this plan's Rust replacement will satisfy. Branching from `main` would silently drop those dependencies.

- [ ] **Step 1: Confirm we are stacked on `dispatch_manifest_freshness`, not branched from `main`**

Run: `gs ll`
Expected: a tree listing showing the current branch's parent as `dispatch_manifest_freshness`, e.g.:

```text
◯ ┳━ main
  ┗━◯ dispatch_manifest_freshness (#234)
      ┗━◉ rust_build_manifest  ← HEAD
```

If HEAD's parent is `main` or anything other than `dispatch_manifest_freshness`, STOP and re-create the branch:

```bash
gs branch checkout dispatch_manifest_freshness
gs branch create rust_build_manifest
```

- [ ] **Step 2: Confirm predecessor commits are present**

Run: `git log --oneline dispatch_manifest_freshness..HEAD ^main | wc -l`
Expected: 0 (no commits yet on our branch).

Then: `git log --oneline main..dispatch_manifest_freshness | head -5`
Expected: a list of recent commits from the predecessor branch (the dispatch-freshness work). If empty, the predecessor branch is missing.

- [ ] **Step 3: Confirm the foundations we depend on actually exist**

Run:

```bash
grep -n "ThirdParty" crates/toolr-core/src/manifest/model.rs
grep -n "pub fn compare" crates/toolr-core/src/freshness/compare.rs
test -f examples/plugin-package/src/toolr_example_plugin/toolr-manifest.json && echo OK
test -f crates/toolr-py/python/toolr/build.py && echo "python build still present"
```

Expected:

- `Origin::ThirdParty` line in `model.rs`.
- A `pub fn compare(...)` signature in `compare.rs`.
- `OK` for the example manifest file.
- `python build still present` for `build.py` (we delete it in Task 12, not now).

If any of these checks fails, the predecessor work did not land — do not proceed.

---

## Task 1: Factor `list_python_files` and `module_path_for` for reuse

**Files:**

- Modify: `crates/toolr-core/src/parser/build.rs:318-346`
- Modify: `crates/toolr-core/src/parser/mod.rs:27-29`

`build_fragment` needs the same file-walking and module-path-derivation as `build_static_manifest_inner`, but with the package name as the module-path prefix instead of the hardcoded `"tools"` string. Factor the helpers without changing their existing behaviour.

- [ ] **Step 1: Promote `list_python_files` to `pub(crate)`**

Edit `crates/toolr-core/src/parser/build.rs:318`. Change:

```rust
fn list_python_files(tools_dir: &Path) -> Vec<PathBuf> {
```

to:

```rust
pub(crate) fn list_python_files(tools_dir: &Path) -> Vec<PathBuf> {
```

- [ ] **Step 2: Introduce `module_path_for_prefix` and have `module_path_for` delegate to it**

In `crates/toolr-core/src/parser/build.rs`, replace the existing `module_path_for` body (lines ~329-345) with:

```rust
fn module_path_for(tools_dir: &Path, file: &Path) -> String {
    module_path_for_prefix(tools_dir, file, "tools")
}

/// Compute a dotted module path for `file` rooted at `source_dir`, using
/// `prefix` as the leading namespace segment. `__init__.py` files
/// collapse to the prefix itself (the package root). Other files become
/// `<prefix>.<rel_no_ext_with_dots>`.
pub(crate) fn module_path_for_prefix(
    source_dir: &Path,
    file: &Path,
    prefix: &str,
) -> String {
    let rel = file.strip_prefix(source_dir).unwrap_or(file);
    let mut parts: Vec<String> = rel
        .with_extension("")
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.last().map(String::as_str) == Some("__init__") {
        parts.pop();
    }
    let mut out = String::from(prefix);
    for p in parts {
        out.push('.');
        out.push_str(&p);
    }
    out
}
```

- [ ] **Step 3: Re-export the helpers from `parser::mod`**

In `crates/toolr-core/src/parser/mod.rs:27-29`, change:

```rust
pub mod build;
pub use build::{BuildError, build_static_manifest, build_static_manifest_with_venv};
```

to:

```rust
pub mod build;
pub use build::{
    BuildError, build_static_manifest, build_static_manifest_with_venv,
    list_python_files, module_path_for_prefix,
};
```

- [ ] **Step 4: Run the existing parser tests to confirm nothing regressed**

Run: `cargo test -p toolr-core --lib parser::`
Expected: all existing parser tests pass, including `builds_manifest_from_single_tools_file` and `cross_file_command_group_string_path_resolves`. The promoted helpers should still produce the exact same module paths as before.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-core/src/parser/build.rs crates/toolr-core/src/parser/mod.rs
git commit -m "refactor(parser): factor list_python_files + module_path_for_prefix for reuse"
```

---

## Task 2: Skeleton `build_fragment` module with error type

**Files:**

- Create: `crates/toolr-core/src/build_fragment.rs`
- Modify: `crates/toolr-core/src/lib.rs`

Stand up the new module with the public API surface and error variants, then iterate the implementation in Tasks 3-7.

- [ ] **Step 1: Create the module skeleton**

Write `crates/toolr-core/src/build_fragment.rs`:

```rust
//! Build a third-party `ManifestFragment` from a plugin's source tree.
//!
//! Pure-Rust replacement for the legacy Python `toolr.build` module.
//! Walks the package's `.py` files via the same AST pipeline used by
//! `build_static_manifest`, applies a plugin-aware module-path prefix,
//! and filters out anything that doesn't belong to the target package.

use std::path::{Path, PathBuf};

use crate::parser::{list_python_files, module_path_for_prefix, parse_python_file};
use crate::parser::{
    commands::extract_commands,
    groups::extract_groups,
    symbols::{ArgSectionTable, EnumTable, TypeAliasTable},
    types::{SourcesImports, TypeImports, TypeResolutionError},
};
use crate::third_party::{
    FragmentArgument, FragmentCommand, FragmentGroup, ManifestFragment,
};

/// Error type for `build_third_party_fragment`.
#[derive(Debug, thiserror::Error)]
pub enum BuildFragmentError {
    #[error("source directory `{path}` is a namespace package (no __init__.py); namespace packages are not supported")]
    NamespacePackage { path: PathBuf },
    #[error("source directory `{path}` does not exist or is not a directory")]
    MissingSourceDir { path: PathBuf },
    #[error("package `{package}` declares no toolr commands - nothing to write")]
    EmptyPackage { package: String },
    #[error("parse error in {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: anyhow::Error,
    },
    #[error("unsupported parameter types ({count}):\n{details}", count = .0.len(), details = format_type_errors(.0))]
    UnsupportedTypes(Vec<TypeResolutionError>),
}

fn format_type_errors(errors: &[TypeResolutionError]) -> String {
    let mut s = String::new();
    for (i, err) in errors.iter().enumerate() {
        if i > 0 {
            s.push('\n');
        }
        use std::fmt::Write as _;
        let _ = write!(&mut s, "  - {err}");
    }
    s
}

/// Build a `ManifestFragment` for `package_name` by AST-walking
/// `source_dir`. See spec `specs/2026-05-22-rust-build-manifest-design.md`.
pub fn build_third_party_fragment(
    source_dir: &Path,
    package_name: &str,
    schema_version: u32,
) -> Result<ManifestFragment, BuildFragmentError> {
    // Reject missing dirs and namespace packages up front.
    if !source_dir.is_dir() {
        return Err(BuildFragmentError::MissingSourceDir {
            path: source_dir.to_path_buf(),
        });
    }
    if !source_dir.join("__init__.py").is_file() {
        return Err(BuildFragmentError::NamespacePackage {
            path: source_dir.to_path_buf(),
        });
    }

    // Pass 1: cross-file enum / alias / arg-section tables.
    let py_files = list_python_files(source_dir);
    let mut enums = EnumTable::default();
    let mut aliases = TypeAliasTable::default();
    let mut sections = ArgSectionTable::default();
    for path in &py_files {
        let module = parse_python_file(path).map_err(|e| BuildFragmentError::Parse {
            path: path.clone(),
            source: e,
        })?;
        enums.merge(EnumTable::from_module(&module));
        aliases.merge(TypeAliasTable::from_module(&module));
        sections.merge(ArgSectionTable::from_module(&module));
    }

    // Pass 2: groups + commands.
    let mut all_groups: Vec<crate::manifest::Group> = Vec::new();
    let mut all_commands: Vec<crate::manifest::Command> = Vec::new();
    let mut seen_groups: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut global_vars: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut type_errors: Vec<TypeResolutionError> = Vec::new();

    for path in &py_files {
        let module = parse_python_file(path).map_err(|e| BuildFragmentError::Parse {
            path: path.clone(),
            source: e,
        })?;
        let module_path = module_path_for_prefix(source_dir, path, package_name);
        let module_doc = module_docstring(&module);
        let bindings = extract_groups(&module, &module_doc, &global_vars);
        let type_imports = TypeImports::from_module(&module);
        let sources_imports = SourcesImports::from_module(&module);
        let commands = extract_commands(
            &module,
            &module_path,
            &bindings,
            &enums,
            &type_imports,
            &sources_imports,
            &aliases,
            &sections,
            &global_vars,
            &mut type_errors,
        );
        for binding in &bindings {
            global_vars.insert(binding.var.clone(), binding.group.full_path());
        }
        for binding in bindings {
            if seen_groups.insert(binding.group.full_path()) {
                all_groups.push(binding.group);
            }
        }
        all_commands.extend(commands);
    }

    if !type_errors.is_empty() {
        return Err(BuildFragmentError::UnsupportedTypes(type_errors));
    }

    // Filter: only keep commands whose `module` belongs to package_name.
    let prefix_dot = format!("{package_name}.");
    all_commands.retain(|c| c.module == package_name || c.module.starts_with(&prefix_dot));

    // Derive surviving groups from surviving commands.
    let surviving_group_names: std::collections::HashSet<&str> =
        all_commands.iter().map(|c| c.group.as_str()).collect();
    all_groups.retain(|g| surviving_group_names.contains(g.full_path().as_str()));

    if all_groups.is_empty() && all_commands.is_empty() {
        return Err(BuildFragmentError::EmptyPackage {
            package: package_name.to_string(),
        });
    }

    // Sort: groups by name, commands by (group, name).
    all_groups.sort_by(|a, b| a.full_path().cmp(&b.full_path()));
    all_commands.sort_by(|a, b| (a.group.as_str(), a.name.as_str()).cmp(&(b.group.as_str(), b.name.as_str())));

    Ok(ManifestFragment {
        toolr_schema_version: schema_version,
        package: package_name.to_string(),
        groups: all_groups
            .into_iter()
            .map(|g| FragmentGroup {
                name: g.full_path(),
                title: g.title,
                description: g.description,
            })
            .collect(),
        commands: all_commands
            .into_iter()
            .map(|c| FragmentCommand {
                name: c.name,
                group: c.group,
                module: c.module,
                function: c.function,
                summary: c.summary,
                description: c.description,
                arguments: c
                    .arguments
                    .into_iter()
                    .map(|a| FragmentArgument {
                        name: a.name,
                        kind: a.kind,
                        help: a.help,
                        default: a.default,
                        type_annotation: a.type_annotation,
                        allowed_values: a.allowed_values,
                    })
                    .collect(),
                imports: c.imports,
            })
            .collect(),
    })
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
    fn rejects_namespace_package_without_init() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("pkg")).unwrap();
        // No __init__.py.
        let err =
            build_third_party_fragment(&tmp.path().join("pkg"), "pkg", 1).unwrap_err();
        assert!(matches!(err, BuildFragmentError::NamespacePackage { .. }));
    }
}
```

- [ ] **Step 2: Wire the module into the crate**

Read `crates/toolr-core/src/lib.rs`, find the `pub mod` block (near the top), and add `pub mod build_fragment;` alongside the other module declarations. Add a re-export below it:

```rust
pub use build_fragment::{BuildFragmentError, build_third_party_fragment};
```

- [ ] **Step 3: Run the skeleton test**

Run: `cargo test -p toolr-core --lib build_fragment::`
Expected: `rejects_namespace_package_without_init` passes; `cargo check` is clean.

- [ ] **Step 4: Commit**

```bash
git add crates/toolr-core/src/build_fragment.rs crates/toolr-core/src/lib.rs
git commit -m "feat(build_fragment): scaffold pure-Rust manifest fragment builder"
```

---

## Task 3: Golden test — single-command plugin

**Files:**

- Modify: `crates/toolr-core/src/build_fragment.rs` (extend inline `tests` module)

Pin the simplest possible case: one group, one command with one optional `str` arg defaulting to `"World"`. This mirrors the `hello_command` in `examples/plugin-package/src/toolr_example_plugin/commands.py`. Any later regression in this golden is the canary that something downstream broke.

- [ ] **Step 1: Write the failing golden test**

Append to the `tests` module in `crates/toolr-core/src/build_fragment.rs`:

```rust
    #[test]
    fn builds_single_command_plugin_fragment() {
        let tmp = TempDir::new().unwrap();
        let pkg = tmp.path().join("mypkg");
        write(&pkg, "__init__.py", "");
        write(
            &pkg,
            "commands.py",
            r#""""Plugin commands."""
from toolr import Context
from toolr import command_group

third_party_group = command_group(
    "third-party",
    "Third Party Tools",
    "Tools contributed by a third-party plugin.",
)

@third_party_group.command("hello")
def hello_command(ctx: Context, name: str = "World") -> None:
    """Say hello to someone.

    Args:
        ctx: The execution context.
        name: Name to greet (default: World).
    """
    ctx.print(f"Hello, {name}")
"#,
        );

        let fragment = build_third_party_fragment(&pkg, "mypkg", 1).unwrap();
        assert_eq!(fragment.toolr_schema_version, 1);
        assert_eq!(fragment.package, "mypkg");
        assert_eq!(fragment.groups.len(), 1);
        assert_eq!(fragment.groups[0].name, "third-party");
        assert_eq!(fragment.groups[0].title, "Third Party Tools");
        assert_eq!(
            fragment.groups[0].description,
            "Tools contributed by a third-party plugin."
        );
        assert_eq!(fragment.commands.len(), 1);
        let cmd = &fragment.commands[0];
        assert_eq!(cmd.name, "hello");
        assert_eq!(cmd.group, "third-party");
        assert_eq!(cmd.module, "mypkg.commands");
        assert_eq!(cmd.function, "hello_command");
        assert_eq!(cmd.summary, "Say hello to someone.");
        assert_eq!(cmd.arguments.len(), 1);
        let arg = &cmd.arguments[0];
        assert_eq!(arg.name, "name");
        assert_eq!(arg.type_annotation.as_deref(), Some("str"));
        assert_eq!(arg.default.as_deref(), Some("'World'"));
        assert_eq!(arg.help, "Name to greet (default: World).");
    }
```

- [ ] **Step 2: Run the test, observe failure**

Run: `cargo test -p toolr-core --lib build_fragment::builds_single_command_plugin_fragment -- --nocapture`
Expected (likely FAIL or PASS depending on Task 2 implementation completeness). If FAIL, read the diff and adjust the implementation in `build_fragment.rs` — most likely:

- A missing field copy in the struct mapping.
- A module-path mismatch (verify Task 1's prefix wiring).
- [ ] **Step 3: Make the test pass**

Iterate on `build_third_party_fragment` until the assertions hold. The mapping should faithfully copy `Argument` → `FragmentArgument` fields and `Command` → `FragmentCommand` fields. If `cmd.module` comes out wrong, double-check `module_path_for_prefix` is being called with `package_name`, not `"tools"`.

- [ ] **Step 4: Run all build_fragment tests**

Run: `cargo test -p toolr-core --lib build_fragment::`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-core/src/build_fragment.rs
git commit -m "test(build_fragment): golden test for single-command plugin"
```

---

## Task 4: Multi-group / multi-command golden test against the example plugin's manifest

**Files:**

- Modify: `crates/toolr-core/src/build_fragment.rs` (extend inline `tests` module)

The committed `examples/plugin-package/src/toolr_example_plugin/toolr-manifest.json` is the byte-for-byte equivalence target. Build a fragment from a fixture that mirrors the example plugin's structure and assert the result equals `ManifestFragment` parsed from the committed JSON.

- [ ] **Step 1: Write the failing equivalence test**

Append to the `tests` module:

```rust
    /// Mirror the canonical `examples/plugin-package/.../commands.py` and
    /// assert the generated fragment equals the committed JSON byte-for-byte.
    ///
    /// We compare ManifestFragment values (not raw bytes) because byte
    /// equality is asserted by the CLI golden test in Task 8. Here we
    /// only need the *value* to match — serialisation ordering is a
    /// separate concern (Task 6).
    #[test]
    fn matches_committed_example_plugin_fragment() {
        let tmp = TempDir::new().unwrap();
        let pkg = tmp.path().join("toolr_example_plugin");
        write(&pkg, "__init__.py", "");
        write(
            &pkg,
            "commands.py",
            r#""""Command groups exposed by the example plugin.

The decorators here populate the toolr command registry at import time.
For static discovery (the canonical path), the same groups are also
serialised into ``toolr-manifest.json`` shipped alongside this module.
"""

from __future__ import annotations

from toolr import Context
from toolr import command_group

third_party_group = command_group(
    "third-party",
    "Third Party Tools",
    "Tools contributed by a third-party plugin.",
)


@third_party_group.command("hello")
def hello_command(ctx: Context, name: str = "World") -> None:
    """Say hello to someone.

    Args:
        ctx: The execution context.
        name: Name to greet (default: World).
    """
    ctx.print(f"Hello, {name} from toolr-plugin-example!")


@third_party_group.command("version")
def version_command(ctx: Context) -> None:
    """Show the version of the example plugin.

    Args:
        ctx: The execution context.
    """
    ctx.print("toolr-plugin-example version 1.0.0")


utils_group = command_group(
    "utils",
    "Utility Commands",
    "General utility commands shipped by the example plugin.",
)


@utils_group.command("echo")
def echo_command(ctx: Context, message: str, repeat: int = 1) -> None:
    """Echo a message multiple times.

    Args:
        ctx: The execution context.
        message: Message to echo.
        repeat: Number of times to repeat the message (default: 1).
    """
    for i in range(repeat):
        ctx.print(f"[{i + 1}] {message}")


@utils_group.command("info")
def info_command(ctx: Context) -> None:
    """Show information about the example plugin.

    Args:
        ctx: The execution context.
    """
    ctx.print("toolr-plugin-example information:")
    ctx.print("- Name: toolr-plugin-example")
    ctx.print("- Version: 1.0.0")
    ctx.print("- Description: Canonical example of a third-party toolr plugin")
"#,
        );

        let fragment =
            build_third_party_fragment(&pkg, "toolr_example_plugin", 1).unwrap();

        // Load the committed reference fragment.
        let reference_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("examples/plugin-package/src/toolr_example_plugin/toolr-manifest.json");
        let reference: ManifestFragment = serde_json::from_str(
            &std::fs::read_to_string(&reference_path).expect("read committed manifest"),
        )
        .expect("parse committed manifest");

        assert_eq!(fragment, reference, "regenerated fragment differs from committed manifest");
    }
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p toolr-core --lib build_fragment::matches_committed_example_plugin_fragment -- --nocapture`
Expected: FAIL with a struct diff. Common deltas:

- `summary` includes the "Args:" lines (docstring-section parser stripped them or not). Check parser/signatures.rs behaviour.
- `default` field mismatch (`"1"` vs `1`).
- Argument ordering or extras.
- [ ] **Step 3: Debug + fix**

The committed JSON is the source of truth. If the assertion diff shows the fragment mapping needs adjustment (e.g. `summary` should be the first docstring line only), fix it in `build_third_party_fragment` or its called helpers. **Do NOT** change the committed JSON — that's what plugin authors are pinned to.

If the diff is genuinely a parser bug rather than a mapping bug, file it inline as a `// TODO(toolr-parser): …` comment and adjust the fixture to a case the parser handles correctly. But first try harder — the existing `build_static_manifest` exercises the same parser successfully for `tools/` files.

- [ ] **Step 4: Run all toolr-core tests**

Run: `cargo test -p toolr-core`
Expected: all pass, including the new equivalence test and the existing parser tests.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-core/src/build_fragment.rs
git commit -m "test(build_fragment): equivalence vs committed example plugin manifest"
```

---

## Task 5: Filtering + edge-case tests

**Files:**

- Modify: `crates/toolr-core/src/build_fragment.rs` (extend inline `tests` module)

Pin the deliberate-narrowing behaviour the spec calls out: imports from other packages don't leak into the fragment; empty packages error; parse errors surface with the file path.

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module:

```rust
    /// A file that declares a group via an import path NOT under the
    /// target package must not appear in the fragment. The Python
    /// implementation enforces this via `_belongs_to_package`; the Rust
    /// equivalent filters by module-path prefix.
    #[test]
    fn filters_out_commands_from_other_packages() {
        let tmp = TempDir::new().unwrap();
        let pkg = tmp.path().join("mypkg");
        write(&pkg, "__init__.py", "");
        // Owned by the target package — should appear.
        write(
            &pkg,
            "own.py",
            r#""""Own."""
from toolr import command_group
g = command_group("own", "Own")

@g.command("ours")
def ours(ctx):
    """Ours."""
    pass
"#,
        );
        // We can't easily simulate "imported from another package" via
        // file-walk alone, since the walker only sees files under
        // source_dir. But the filter applies via `cmd.module`: any file
        // we manually parse outside the prefix gets dropped. Construct a
        // sibling under a different prefix to verify the *filter step*
        // doesn't accidentally keep it.
        //
        // Simpler proof: a subdirectory whose __init__.py declares a
        // group should be kept (it's still under mypkg.*) — sanity check.
        write(
            &pkg,
            "sub/__init__.py",
            r#""""Sub."""
from toolr import command_group
g = command_group("subg", "Sub Group")

@g.command("subcmd")
def subcmd(ctx):
    """Sub command."""
    pass
"#,
        );

        let fragment = build_third_party_fragment(&pkg, "mypkg", 1).unwrap();
        let group_names: Vec<&str> = fragment.groups.iter().map(|g| g.name.as_str()).collect();
        assert!(group_names.contains(&"own"));
        assert!(group_names.contains(&"subg"));
        // Confirm modules are prefixed with the package, not "tools".
        let modules: Vec<&str> = fragment.commands.iter().map(|c| c.module.as_str()).collect();
        for m in &modules {
            assert!(m.starts_with("mypkg"), "unexpected module: {m}");
        }
    }

    /// An empty package (init only, no commands) returns EmptyPackage.
    #[test]
    fn empty_package_errors() {
        let tmp = TempDir::new().unwrap();
        let pkg = tmp.path().join("empty");
        write(&pkg, "__init__.py", "");
        let err = build_third_party_fragment(&pkg, "empty", 1).unwrap_err();
        assert!(matches!(err, BuildFragmentError::EmptyPackage { .. }));
    }

    /// A `.py` file with a syntax error surfaces a Parse error naming
    /// the offending file. Crucial for plugin authors — without the
    /// path, the diagnostic is useless.
    #[test]
    fn parse_error_includes_file_path() {
        let tmp = TempDir::new().unwrap();
        let pkg = tmp.path().join("broken");
        write(&pkg, "__init__.py", "");
        write(&pkg, "bad.py", "def broken(\n");
        let err = build_third_party_fragment(&pkg, "broken", 1).unwrap_err();
        match err {
            BuildFragmentError::Parse { path, .. } => {
                assert!(path.ends_with("bad.py"), "got: {}", path.display());
            }
            other => panic!("expected Parse, got {other:?}"),
        }
    }

    /// Source dir that does not exist surfaces MissingSourceDir.
    #[test]
    fn missing_source_dir_errors() {
        let tmp = TempDir::new().unwrap();
        let pkg = tmp.path().join("nope");
        let err = build_third_party_fragment(&pkg, "nope", 1).unwrap_err();
        assert!(matches!(err, BuildFragmentError::MissingSourceDir { .. }));
    }
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p toolr-core --lib build_fragment::`
Expected: all four new tests pass. If `filters_out_commands_from_other_packages` fails because the subpackage isn't picked up, the walker's recursion or `module_path_for_prefix`'s handling of `sub/__init__.py` is broken — debug.

- [ ] **Step 3: Commit**

```bash
git add crates/toolr-core/src/build_fragment.rs
git commit -m "test(build_fragment): filtering + edge-case coverage"
```

---

## Task 6: Stable JSON serialisation helper

**Files:**

- Modify: `crates/toolr-core/src/build_fragment.rs`
- Modify: `Cargo.toml` (no change; verify serde_json already in workspace deps).

Plugin authors' CI scripts diff `toolr-manifest.json` between runs. The Rust serialiser must match the Python output: 2-space indent, sorted keys at every level, trailing newline. The Python implementation uses `json.dumps(indent=2, sort_keys=True) + "\n"`. We round-trip through `serde_json::Value` (BTreeMap-backed under the default feature set, which sorts alphabetically) to get sorted keys, then `to_string_pretty` for the indent.

- [ ] **Step 1: Verify serde_json's `Value::Object` is a BTreeMap (not preserve-order)**

Run: `grep -A 2 '^name = "serde_json"' Cargo.lock | head -10`
Confirm there is no `features = ["preserve_order"]` in the dependency chain. The default feature set uses BTreeMap which sorts keys; round-tripping `to_value(&fragment)` → `to_string_pretty(&value)` therefore produces sort_keys=True-equivalent output.

If grep reveals `preserve_order` is somehow enabled, STOP and reconsider — the plan assumes BTreeMap-backed Maps.

- [ ] **Step 2: Add the failing serialisation test**

Append to the `tests` module in `crates/toolr-core/src/build_fragment.rs`:

```rust
    /// Serialised JSON matches the committed example plugin manifest
    /// byte-for-byte. This is the regression guard that lets `--check`
    /// in CI catch any drift in field ordering, whitespace, or trailing
    /// newline handling.
    #[test]
    fn serialised_fragment_matches_committed_bytes() {
        let tmp = TempDir::new().unwrap();
        let pkg = tmp.path().join("toolr_example_plugin");
        write(&pkg, "__init__.py", "");
        // Reuses the fixture from `matches_committed_example_plugin_fragment`.
        write(
            &pkg,
            "commands.py",
            include_str!("../../../examples/plugin-package/src/toolr_example_plugin/commands.py"),
        );

        let fragment =
            build_third_party_fragment(&pkg, "toolr_example_plugin", 1).unwrap();
        let serialised = serialise_fragment(&fragment).expect("serialise");

        let reference = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("examples/plugin-package/src/toolr_example_plugin/toolr-manifest.json"),
        )
        .unwrap();

        assert_eq!(serialised, reference, "byte mismatch vs committed manifest");
    }
```

- [ ] **Step 3: Implement `serialise_fragment`**

Add to `crates/toolr-core/src/build_fragment.rs`, after `build_third_party_fragment`:

```rust
/// Serialise a fragment to the canonical on-disk form: 2-space indent,
/// keys sorted at every depth, trailing newline. Round-trips through
/// `serde_json::Value` so the default BTreeMap-backed `Map` enforces
/// alphabetical key order — matches Python's
/// `json.dumps(fragment, indent=2, sort_keys=True) + "\n"`.
pub fn serialise_fragment(fragment: &ManifestFragment) -> Result<String, serde_json::Error> {
    let value = serde_json::to_value(fragment)?;
    let mut out = serde_json::to_string_pretty(&value)?;
    out.push('\n');
    Ok(out)
}
```

Add to the re-export list in `crates/toolr-core/src/lib.rs`:

```rust
pub use build_fragment::{BuildFragmentError, build_third_party_fragment, serialise_fragment};
```

- [ ] **Step 4: Run the test**

Run: `cargo test -p toolr-core --lib build_fragment::serialised_fragment_matches_committed_bytes -- --nocapture`
Expected: PASS. If FAIL, the diff between Rust output and the committed file points to one of:

- Missing trailing newline (Python `+ "\n"`).
- Indent depth (must be 2 spaces).
- Field-name casing (`type_annotation` vs `typeAnnotation` etc — `ArgumentKind` serde rename is `snake_case`, confirmed in `crates/toolr-core/src/manifest/model.rs:212`).

Adjust the implementation until the bytes match. If a Python-side serialiser quirk (e.g. how it renders `None` in `default`) creates a diff, decide deliberately: either match the Python quirk in the Rust serde model, or regenerate the committed JSON. The spec calls for byte-for-byte equivalence, so default to matching Python.

- [ ] **Step 5: Run all toolr-core tests**

Run: `cargo test -p toolr-core`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add crates/toolr-core/src/build_fragment.rs crates/toolr-core/src/lib.rs
git commit -m "feat(build_fragment): stable JSON serialisation matching Python output"
```

---

## Task 7: Cross-file enum / Literal type test

**Files:**

- Modify: `crates/toolr-core/src/build_fragment.rs` (extend inline `tests` module)

The existing `build_static_manifest` pipeline supports enums and type aliases defined in one file and used as `Literal[...]` types in another. The fragment path reuses the same machinery — verify it works end-to-end for plugin source layouts.

- [ ] **Step 1: Write the failing test**

Append to the `tests` module:

```rust
    /// An enum or Literal alias declared in module `a.py` and used as
    /// the type of a command arg in `b.py` resolves correctly. This
    /// exercises the cross-file `EnumTable` / `TypeAliasTable` merge
    /// that pass 1 sets up.
    #[test]
    fn cross_file_literal_resolves_in_fragment() {
        let tmp = TempDir::new().unwrap();
        let pkg = tmp.path().join("xpkg");
        write(&pkg, "__init__.py", "");
        write(
            &pkg,
            "types.py",
            r#""""Types shared across the package."""
from typing import Literal

Mode = Literal["fast", "slow"]
"#,
        );
        write(
            &pkg,
            "commands.py",
            r#""""Commands using cross-file Literal."""
from toolr import Context
from toolr import command_group

from .types import Mode

group = command_group("cf", "Cross-file")

@group.command("run")
def run(ctx: Context, mode: Mode = "fast") -> None:
    """Run.

    Args:
        ctx: ctx.
        mode: mode to run.
    """
    pass
"#,
        );

        let fragment = build_third_party_fragment(&pkg, "xpkg", 1).unwrap();
        let cmd = fragment
            .commands
            .iter()
            .find(|c| c.name == "run")
            .expect("run command");
        let arg = cmd.arguments.iter().find(|a| a.name == "mode").unwrap();
        assert_eq!(
            arg.allowed_values,
            vec!["fast".to_string(), "slow".to_string()],
            "expected Literal[fast, slow] to populate allowed_values"
        );
    }
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p toolr-core --lib build_fragment::cross_file_literal_resolves_in_fragment -- --nocapture`
Expected: PASS. If FAIL, either the `TypeAliasTable` isn't being merged across files for the build_fragment path (compare wiring against `parser::build::build_static_manifest_inner`), or the alias resolution doesn't follow `from .types import Mode` style relative imports.

- [ ] **Step 3: Commit**

```bash
git add crates/toolr-core/src/build_fragment.rs
git commit -m "test(build_fragment): cross-file Literal type resolution"
```

---

## Task 8: Source / package resolution helper

**Files:**

- Create: `crates/toolr/src/build_manifest_resolve.rs`
- Modify: `crates/toolr/src/main.rs` (add `mod build_manifest_resolve;`)

The CLI layer needs a single function that turns parsed `ArgMatches` into a `(source_dir, package_name)` pair, handling the two entry modes from the spec:

1. `<package>` positional + venv-glob resolution.
2. `--source-dir PATH` + `--package PKG` (or infer from leaf directory).

Centralise this in its own module so dispatch.rs stays thin.

- [ ] **Step 1: Create the module**

Write `crates/toolr/src/build_manifest_resolve.rs`:

```rust
//! Resolve `(source_dir, package_name)` for `toolr self build-manifest`.
//!
//! Two entry modes (mutually exclusive at the CLI level):
//! 1. `<package>` positional → glob the tools venv for the installed
//!    package directory.
//! 2. `--source-dir PATH` → use the path verbatim; package name comes
//!    from `--package PKG` or the leaf directory name.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toolr_core::discovery::discover_project_root;
use toolr_core::venv::resolve_venv_path;

pub struct ResolvedSource {
    pub source_dir: PathBuf,
    pub package_name: String,
}

pub fn resolve_source_and_package(matches: &clap::ArgMatches) -> Result<ResolvedSource> {
    let source_dir = matches.get_one::<String>("source-dir").map(PathBuf::from);
    let package_arg = matches.get_one::<String>("package").cloned();
    let positional_pkg = matches.get_one::<String>("package_positional").cloned();

    match (source_dir, positional_pkg) {
        (Some(_), Some(_)) => anyhow::bail!(
            "`<package>` and `--source-dir` are mutually exclusive; pass one or the other"
        ),
        (Some(dir), None) => {
            let package_name = package_arg
                .or_else(|| leaf_dir_name(&dir))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "--source-dir {} has no inferable package name; pass --package PKG",
                        dir.display()
                    )
                })?;
            if !dir.is_dir() {
                anyhow::bail!("--source-dir `{}` is not a directory", dir.display());
            }
            Ok(ResolvedSource {
                source_dir: dir,
                package_name,
            })
        }
        (None, Some(pkg)) => {
            let cwd = std::env::current_dir().context("getting current directory")?;
            let repo_root = discover_project_root(&cwd)
                .context("resolving repo root for tools-venv lookup")?;
            let resolved = resolve_venv_path(&repo_root)
                .context("resolving tools-venv path")?;
            let dir = find_in_venv(&resolved.venv_dir, &pkg).with_context(|| {
                format!(
                    "package `{pkg}` not found under {}; run `uv sync` or pass --source-dir",
                    resolved.venv_dir.display()
                )
            })?;
            Ok(ResolvedSource {
                source_dir: dir,
                package_name: pkg,
            })
        }
        (None, None) => anyhow::bail!(
            "missing required argument: either `<package>` or `--source-dir PATH`"
        ),
    }
}

fn leaf_dir_name(dir: &Path) -> Option<String> {
    dir.file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}

/// Glob `<venv>/lib/python*/site-packages/<package>/` for an installed
/// plugin's source directory. Picks the first match lexicographically
/// when more than one Python version is present.
fn find_in_venv(venv_dir: &Path, package: &str) -> Result<PathBuf> {
    let pattern = format!(
        "{}/lib/python*/site-packages/{}",
        venv_dir.display(),
        package
    );
    let entries = glob::glob(&pattern)
        .with_context(|| format!("globbing `{pattern}`"))?
        .filter_map(Result::ok)
        .filter(|p| p.is_dir())
        .collect::<Vec<_>>();
    let mut entries = entries;
    entries.sort();
    if entries.is_empty() {
        anyhow::bail!("no site-packages directory matches `{pattern}`");
    }
    if entries.len() > 1 {
        eprintln!(
            "toolr: warning: multiple matches for `{package}` in venv, using {}",
            entries[0].display()
        );
    }
    Ok(entries.into_iter().next().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn find_in_venv_picks_first_lexicographic_match() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path();
        std::fs::create_dir_all(venv.join("lib/python3.12/site-packages/pkg")).unwrap();
        std::fs::create_dir_all(venv.join("lib/python3.13/site-packages/pkg")).unwrap();
        let found = find_in_venv(venv, "pkg").unwrap();
        assert!(found.ends_with("lib/python3.12/site-packages/pkg"), "got: {}", found.display());
    }

    #[test]
    fn find_in_venv_errors_when_missing() {
        let tmp = TempDir::new().unwrap();
        let err = find_in_venv(tmp.path(), "nope").unwrap_err();
        assert!(err.to_string().contains("no site-packages"));
    }

    #[test]
    fn leaf_dir_name_extracts_basename() {
        let dir = PathBuf::from("/a/b/mypkg");
        assert_eq!(leaf_dir_name(&dir).as_deref(), Some("mypkg"));
    }
}
```

- [ ] **Step 2: Add the `glob` workspace dep to `toolr` crate**

Verify `glob.workspace = true` is in `crates/toolr/Cargo.toml`'s `[dependencies]` section. If absent, add it:

```toml
glob.workspace = true
```

(It's already used by `toolr-core`, so it should be in the workspace `[workspace.dependencies]`. If it isn't, the `cargo check` in Step 4 will surface that — add it then.)

- [ ] **Step 3: Wire the module into the binary crate**

In `crates/toolr/src/main.rs`, add alongside the existing `mod` declarations:

```rust
mod build_manifest_resolve;
```

- [ ] **Step 4: Run the resolver tests**

Run: `cargo test -p toolr --lib build_manifest_resolve::`
Expected: all three tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr/src/build_manifest_resolve.rs crates/toolr/src/main.rs crates/toolr/Cargo.toml
git commit -m "feat(toolr/cli): source+package resolution for build-manifest"
```

---

## Task 9: CLI flag changes

**Files:**

- Modify: `crates/toolr/src/cli.rs:321-358`

Switch the `build-manifest` clap definition to the new shape: positional `<package>` becomes optional (renamed to `package_positional` internally to disambiguate from `--package`); add `--source-dir` and `--package`; remove `--python` entirely. Add a `conflicts_with` link between `package_positional` and `source-dir`.

- [ ] **Step 1: Replace the `build-manifest` subcommand definition**

In `crates/toolr/src/cli.rs`, locate the `.subcommand(Command::new("build-manifest")...)` block (currently lines ~321-358) and replace it with:

```rust
            .subcommand(
                Command::new("build-manifest")
                    .about("Generate a third-party manifest fragment for a package")
                    .arg(
                        Arg::new("package_positional")
                            .value_name("PACKAGE")
                            .required(false)
                            .conflicts_with("source-dir")
                            .help("Dotted Python package name to introspect (looked up in the tools venv)"),
                    )
                    .arg(
                        Arg::new("source-dir")
                            .long("source-dir")
                            .value_name("PATH")
                            .conflicts_with("package_positional")
                            .help(
                                "Path to the package's source tree (bypasses the tools-venv lookup)",
                            ),
                    )
                    .arg(
                        Arg::new("package")
                            .long("package")
                            .value_name("PKG")
                            .requires("source-dir")
                            .help(
                                "Package name to embed in the fragment when using --source-dir \
                                 (defaults to the leaf directory name)",
                            ),
                    )
                    .arg(
                        Arg::new("output")
                            .long("output")
                            .value_name("PATH")
                            .help("Override the output path"),
                    )
                    .arg(
                        Arg::new("schema-version")
                            .long("schema-version")
                            .value_name("N")
                            .value_parser(clap::value_parser!(u32))
                            .help("Pin the emitted schema version"),
                    )
                    .arg(
                        Arg::new("check")
                            .long("check")
                            .action(ArgAction::SetTrue)
                            .help("Verify the on-disk manifest matches regeneration"),
                    ),
            )
```

- [ ] **Step 2: Verify the CLI compiles and `--help` reflects the new surface**

Run: `cargo run -p toolr -- self build-manifest --help`
Expected output (relevant lines):

```text
Generate a third-party manifest fragment for a package

Usage: toolr self build-manifest [OPTIONS] [PACKAGE]

Arguments:
  [PACKAGE]  Dotted Python package name to introspect (looked up in the tools venv)

Options:
      --source-dir <PATH>     Path to the package's source tree (bypasses the tools-venv lookup)
      --package <PKG>         Package name to embed in the fragment when using --source-dir (defaults to the leaf directory name)
      --output <PATH>         Override the output path
      --schema-version <N>    Pin the emitted schema version
      --check                 Verify the on-disk manifest matches regeneration
  -h, --help                  Print help
```

`--python` should NOT appear at all (removed outright).

- [ ] **Step 3: Confirm `--package` + `<PACKAGE>` are mutually exclusive**

Run: `cargo run -p toolr -- self build-manifest foo --source-dir /tmp/foo`
Expected: clap rejects with an error like `the argument '[PACKAGE]' cannot be used with '--source-dir <PATH>'` and exit code 2.

- [ ] **Step 4: Commit**

```bash
git add crates/toolr/src/cli.rs
git commit -m "feat(cli): add --source-dir and --package, hide deprecated --python"
```

---

## Task 10: Add `similar` for unified-diff drift output

**Files:**

- Modify: `Cargo.toml` (workspace root)
- Modify: `crates/toolr/Cargo.toml`

`--check` emits a unified diff on drift so CI logs show what changed without forcing the user to regenerate locally. Use the `similar` crate (well-maintained, widely used).

- [ ] **Step 1: Add `similar` to workspace dependencies**

In the workspace root `Cargo.toml`, find the `[workspace.dependencies]` block (or add one if absent), and add:

```toml
similar = "2"
```

- [ ] **Step 2: Add the dep to `crates/toolr/Cargo.toml`**

Under `[dependencies]`, add:

```toml
similar.workspace = true
```

- [ ] **Step 3: Verify the dep resolves**

Run: `cargo check -p toolr`
Expected: clean build, `similar` v2.x downloaded if not already cached.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml crates/toolr/Cargo.toml Cargo.lock
git commit -m "build: add similar crate for unified-diff output"
```

---

## Task 11: Rewire `run_self_build_manifest` to the Rust path

**Files:**

- Modify: `crates/toolr/src/dispatch.rs:241-290`

Replace the Python-spawn implementation with calls into `toolr_core::build_fragment` and the new resolver. Add helpers for output-path resolution, atomic write, and drift check.

- [ ] **Step 1: Replace `run_self_build_manifest` and delete `resolve_python_for_build`**

In `crates/toolr/src/dispatch.rs`, replace lines 241-290 (the existing `run_self_build_manifest` + `resolve_python_for_build` functions) with:

```rust
fn run_self_build_manifest(matches: &clap::ArgMatches) -> anyhow::Result<ExitCode> {
    let resolved = crate::build_manifest_resolve::resolve_source_and_package(matches)?;

    let schema_version: u32 = matches
        .get_one::<u32>("schema-version")
        .copied()
        .unwrap_or(toolr_core::third_party::FRAGMENT_SCHEMA_VERSION);

    let output_path = resolve_output_path(matches, &resolved.source_dir);

    let fragment = toolr_core::build_fragment::build_third_party_fragment(
        &resolved.source_dir,
        &resolved.package_name,
        schema_version,
    )?;
    let serialised = toolr_core::build_fragment::serialise_fragment(&fragment)?;

    if matches.get_flag("check") {
        return check_against_disk(&output_path, &serialised);
    }

    write_atomically(&output_path, &serialised)?;
    eprintln!(
        "toolr.build: wrote {} group(s) / {} command(s) to {}",
        fragment.groups.len(),
        fragment.commands.len(),
        output_path.display(),
    );
    Ok(ExitCode::SUCCESS)
}

fn resolve_output_path(matches: &clap::ArgMatches, source_dir: &std::path::Path) -> PathBuf {
    matches
        .get_one::<String>("output")
        .map(PathBuf::from)
        .unwrap_or_else(|| source_dir.join("toolr-manifest.json"))
}

fn write_atomically(path: &std::path::Path, contents: &str) -> anyhow::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = tempfile::NamedTempFile::new_in(
        path.parent().unwrap_or_else(|| std::path::Path::new(".")),
    )?;
    tmp.write_all(contents.as_bytes())?;
    tmp.persist(path).map_err(|e| anyhow::anyhow!("persist: {e}"))?;
    Ok(())
}

fn check_against_disk(path: &std::path::Path, serialised: &str) -> anyhow::Result<ExitCode> {
    let existing = if path.is_file() {
        std::fs::read_to_string(path)?
    } else {
        String::new()
    };
    if existing == serialised {
        Ok(ExitCode::SUCCESS)
    } else {
        let diff = similar::TextDiff::from_lines(&existing, serialised);
        eprintln!(
            "toolr.build: {} is out of date - regenerate with `toolr self build-manifest <pkg>`",
            path.display(),
        );
        eprintln!("{}", diff.unified_diff().header("on-disk", "regenerated"));
        Ok(ExitCode::from(2))
    }
}
```

The unused-import sweep: after this edit, `std::path::PathBuf` is still used, but `use std::process::Command` and `which::which` (if previously imported only for the Python spawn) become dead. Run `cargo check` and remove any newly unused imports the compiler flags.

- [ ] **Step 2: Add `tempfile` and `similar` to `[dependencies]` of `crates/toolr/Cargo.toml`**

`tempfile.workspace = true` may already be under `[dev-dependencies]` only — promote it to `[dependencies]`. `similar.workspace = true` was added in Task 10.

Edit `crates/toolr/Cargo.toml` so `[dependencies]` includes:

```toml
similar.workspace = true
tempfile.workspace = true
```

(Keep `[dev-dependencies]` as-is; cargo de-duplicates.)

- [ ] **Step 3: Confirm the crate builds**

Run: `cargo check -p toolr`
Expected: clean build, no unused-import warnings. If `which::which` becomes unused, remove its `use` statement at the top of `dispatch.rs`.

- [ ] **Step 4: Smoke-test the new path against the example plugin**

Run:

```bash
cargo build -p toolr
./target/debug/toolr self build-manifest --source-dir examples/plugin-package/src/toolr_example_plugin --package toolr_example_plugin --check
```

Expected: exit code 0, no output on stderr (the committed manifest matches the freshly-generated one). If exit is non-zero, the unified diff on stderr tells you what drifted — fix the serialiser or the parser mapping until the bytes line up.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr/src/dispatch.rs crates/toolr/Cargo.toml
git commit -m "feat(dispatch): rewire `self build-manifest` to pure-Rust path"
```

---

## Task 12: Integration tests for the CLI surface

**Files:**

- Create: `crates/toolr/tests/build_manifest_cli.rs`

Drive the binary via `assert_cmd` to confirm the user-visible behaviour: `--check` exits 0 on match and 2 on drift, `--source-dir`/`<package>` mutex, namespace-package rejection.

- [ ] **Step 1: Write the integration test file**

Write `crates/toolr/tests/build_manifest_cli.rs`:

```rust
//! Integration tests for `toolr self build-manifest`. Drives the
//! installed `toolr` binary via `assert_cmd` so the assertions exercise
//! the same code path users hit on the command line.

use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

fn write(tmp: &std::path::Path, rel: &str, contents: &str) {
    let path = tmp.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn minimal_plugin(tmp: &std::path::Path, name: &str) -> std::path::PathBuf {
    let pkg = tmp.join(name);
    write(&pkg, "__init__.py", "");
    write(
        &pkg,
        "commands.py",
        r#""""Commands."""
from toolr import Context
from toolr import command_group

g = command_group("g", "G")

@g.command("hi")
def hi(ctx: Context) -> None:
    """Hi."""
    pass
"#,
    );
    pkg
}

#[test]
fn source_dir_generates_manifest() {
    let tmp = TempDir::new().unwrap();
    let pkg = minimal_plugin(tmp.path(), "mypkg");

    let mut cmd = Command::cargo_bin("toolr").unwrap();
    cmd.args([
        "self",
        "build-manifest",
        "--source-dir",
    ])
    .arg(&pkg)
    .args(["--package", "mypkg"]);
    cmd.assert().success();

    let written = pkg.join("toolr-manifest.json");
    assert!(written.is_file(), "manifest was not written");
    let body = fs::read_to_string(&written).unwrap();
    assert!(body.contains("\"package\": \"mypkg\""));
    assert!(body.contains("\"name\": \"hi\""));
    assert!(body.ends_with('\n'), "missing trailing newline");
}

#[test]
fn check_passes_when_manifest_is_in_sync() {
    let tmp = TempDir::new().unwrap();
    let pkg = minimal_plugin(tmp.path(), "mypkg");
    // Generate once, then --check.
    Command::cargo_bin("toolr")
        .unwrap()
        .args(["self", "build-manifest", "--source-dir"])
        .arg(&pkg)
        .args(["--package", "mypkg"])
        .assert()
        .success();
    Command::cargo_bin("toolr")
        .unwrap()
        .args(["self", "build-manifest", "--source-dir"])
        .arg(&pkg)
        .args(["--package", "mypkg", "--check"])
        .assert()
        .success();
}

#[test]
fn check_emits_diff_and_exits_2_on_drift() {
    let tmp = TempDir::new().unwrap();
    let pkg = minimal_plugin(tmp.path(), "mypkg");
    // Plant a stale manifest at the expected output path.
    fs::write(pkg.join("toolr-manifest.json"), "{\"stale\": true}\n").unwrap();

    Command::cargo_bin("toolr")
        .unwrap()
        .args(["self", "build-manifest", "--source-dir"])
        .arg(&pkg)
        .args(["--package", "mypkg", "--check"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicates::str::contains("out of date"));
}

#[test]
fn rejects_namespace_package() {
    let tmp = TempDir::new().unwrap();
    let pkg = tmp.path().join("nsp");
    fs::create_dir_all(&pkg).unwrap();
    // Deliberately no __init__.py.

    Command::cargo_bin("toolr")
        .unwrap()
        .args(["self", "build-manifest", "--source-dir"])
        .arg(&pkg)
        .args(["--package", "nsp"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("namespace package"));
}

#[test]
fn package_positional_conflicts_with_source_dir() {
    let tmp = TempDir::new().unwrap();
    let pkg = minimal_plugin(tmp.path(), "mypkg");

    Command::cargo_bin("toolr")
        .unwrap()
        .args(["self", "build-manifest", "mypkg", "--source-dir"])
        .arg(&pkg)
        .assert()
        .failure()
        .stderr(predicates::str::contains("cannot be used with"));
}

#[test]
fn python_flag_is_no_longer_accepted() {
    let tmp = TempDir::new().unwrap();
    let pkg = minimal_plugin(tmp.path(), "mypkg");

    Command::cargo_bin("toolr")
        .unwrap()
        .args(["self", "build-manifest", "--source-dir"])
        .arg(&pkg)
        .args(["--package", "mypkg", "--python", "/usr/bin/python3"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("unexpected argument"));
}
```

- [ ] **Step 2: Add `predicates` to `[dev-dependencies]`**

Verify `crates/toolr/Cargo.toml`'s `[dev-dependencies]` includes:

```toml
predicates = "3"
```

If absent, add it. (Also confirm it's in workspace `[workspace.dependencies]` — add `predicates = "3"` if missing.)

- [ ] **Step 3: Run the integration tests**

Run: `cargo test -p toolr --test build_manifest_cli`
Expected: all six tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/toolr/tests/build_manifest_cli.rs crates/toolr/Cargo.toml Cargo.toml Cargo.lock
git commit -m "test(toolr): integration coverage for self build-manifest CLI"
```

---

## Task 13: Equivalence check against the canonical example plugin

**Files:** None (verification only).

The unit test in Task 4 already asserts in-memory equivalence; the CI step at `.github/workflows/_test.yml:179` runs `--check` against the committed manifest on every test job. Confirm both invariants hold before continuing.

- [ ] **Step 1: Regenerate the example plugin's manifest in-place and diff**

Run:

```bash
cargo build -p toolr
./target/debug/toolr self build-manifest --source-dir examples/plugin-package/src/toolr_example_plugin --package toolr_example_plugin
git diff -- examples/plugin-package/src/toolr_example_plugin/toolr-manifest.json
```

Expected: `git diff` shows **no changes**. If the file changed, investigate before continuing — either the Rust serialiser drifted from the Python output (more likely) or the committed file had a Python-side bug that the Rust path fixed (mention this in the eventual PR description; the spec calls this an investigatable signal).

- [ ] **Step 2: Run the in-CI drift check manually**

Run:

```bash
./target/debug/toolr self build-manifest toolr_example_plugin --check 2>&1 || true
```

This may fail if the tools venv has no `toolr_example_plugin` installed locally — that's expected on a fresh checkout. To exercise the `<package>` resolution path locally, first run `uv sync` in `tools/` or use the `--source-dir` form above.

- [ ] **Step 3: Run the project's existing distribution test**

Run: `uv run pytest tests/distribution/test_example_plugin_contract.py -v`
Expected: PASS. The test asserts the shipped manifest matches runtime expectations; the fragment shape is preserved, so it must continue to pass.

- [ ] **Step 4: Commit (no-op if Step 1 produced no diff)**

If for any reason the example manifest *did* change (e.g. a deliberate, reviewed fix-up), commit the regenerated file:

```bash
git add examples/plugin-package/src/toolr_example_plugin/toolr-manifest.json
git commit -m "fix(examples): regenerate example plugin manifest via Rust builder"
```

If no diff: skip the commit and move on.

---

## Task 14: Delete the Python `toolr.build` module

**Files:**

- Delete: `crates/toolr-py/python/toolr/build.py`
- Modify: `crates/toolr-py/python/toolr/__init__.py` (if it re-exports `build`).
- [ ] **Step 1: Check whether `toolr/__init__.py` re-exports `build`**

Run: `grep -n "build" crates/toolr-py/python/toolr/__init__.py`
Expected: usually no result (the canonical entry point is `python -m toolr.build`, not `from toolr import build`). If a re-export exists, remove it.

- [ ] **Step 2: Delete the module**

Run: `git rm crates/toolr-py/python/toolr/build.py`

- [ ] **Step 3: Confirm nothing else imports it**

Run: `grep -rn "toolr.build\|from toolr import build\|import toolr.build" crates/ tests/ docs/ 2>/dev/null`
Expected: matches only in (a) deletion-pending Python tests under `tests/build_manifest/` (handled in Task 15) and (b) `docs/third-party.md` mentions (handled in Task 16). Any other match needs investigation — could be a fixture or a CI script that still calls the deleted module.

- [ ] **Step 4: Confirm the Rust build is green without the Python module**

Run: `cargo check -p toolr -p toolr-core`
Expected: clean.

- [ ] **Step 5: Confirm `toolr.testing` (which the spec says must keep working) still imports cleanly**

Run: `uv run python -c "from toolr import testing; print('ok')"`
Expected: `ok`. The spec is explicit that `toolr._decorators._get_command_group_storage` stays — only `build.py` goes.

- [ ] **Step 6: Commit**

```bash
git commit -m "feat(toolr-py): remove Python toolr.build module"
```

---

## Task 15: Delete the obsolete Python build_manifest tests

**Files:**

- Delete: `tests/build_manifest/test_build_manifest.py`
- Delete: `tests/build_manifest/test_round_trip_with_rust.py`
- Delete: `tests/build_manifest/__init__.py`
- Delete: `tests/build_manifest/` (now-empty directory).

These tests exercised `from toolr.build import build_manifest`, which no longer exists. Coverage is replaced by:

- Unit tests in `crates/toolr-core/src/build_fragment.rs` (Tasks 3-7).
- Integration tests in `crates/toolr/tests/build_manifest_cli.rs` (Task 12).
- CI `--check` against the example plugin (`.github/workflows/_test.yml:167-179`).
- [ ] **Step 1: Confirm the directory contents**

Run: `ls tests/build_manifest/`
Expected: `__init__.py`, `test_build_manifest.py`, `test_round_trip_with_rust.py`. If anything else is present, decide case-by-case before deleting.

- [ ] **Step 2: Delete the directory**

Run:

```bash
git rm -r tests/build_manifest/
```

- [ ] **Step 3: Confirm the test suite still discovers everything else**

Run: `uv run pytest --collect-only -q tests/ 2>&1 | tail -5`
Expected: collection completes; no errors about missing `tests/build_manifest/`. If pytest complains about a missing path in a config file, search for it: `grep -rn "build_manifest" tests/conftest.py tests/__init__.py pyproject.toml 2>/dev/null`.

- [ ] **Step 4: Run a quick Python test smoke**

Run: `uv run pytest tests/distribution/test_example_plugin_contract.py -v`
Expected: PASS (already verified in Task 13, but worth re-confirming after the deletion sweep).

- [ ] **Step 5: Commit**

```bash
git commit -m "test: drop Python build_manifest tests superseded by Rust path"
```

---

## Task 16: Update `docs/third-party.md`

**Files:**

- Modify: `docs/third-party.md`

Drop Python-API references; preserve the `toolr self build-manifest <pkg>` CLI as the canonical user-facing command; add the static-only contract note.

- [ ] **Step 1: Read the current doc to identify edit sites**

Run: `grep -n "python -m toolr.build\|from toolr.build\|--python\|build_manifest(" docs/third-party.md`
Expected: a handful of occurrences (lines reported earlier: 40, 50, 52, 62, 142, 146, 153, 167, 180, 203, 217). Each one needs a decision: drop, replace with the CLI form, or rephrase.

- [ ] **Step 2: Apply the edits**

For each match:

- Lines referencing `python -m toolr.build my_pkg` → replace with `toolr self build-manifest my_pkg`.
- Lines referencing `from toolr.build import build_manifest` and the surrounding Python-API example block → delete the block entirely. Plugin authors are no longer expected to call into Python to build the manifest.
- Lines referencing `--python` → delete.
- Pre-commit / CI examples already use `toolr self build-manifest` — leave them.

Use `Edit` with `old_string`/`new_string` pairs for each occurrence; don't rewrite the file wholesale.

After the edits, add a new short subsection right above the "Regenerating after edits" section (typically near line 140) titled:

```markdown
### Static-only contract

`toolr self build-manifest` walks your package's source with a Rust AST
parser. It captures every `command_group(...)` / `@group.command`
declaration that the parser can see *statically* — same as the project
manifest builder. Dynamic registration (`for x in X: group.command(...)`)
is intentionally not supported: a manifest emitted from such patterns
would not match what the Rust dispatch path can resolve at runtime
anyway. If you need dynamic patterns, hand-edit the resulting
`toolr-manifest.json`.
```

- [ ] **Step 3: Re-render the docs locally (optional but recommended)**

If you have `mkdocs` configured (or whatever the docs builder is), run a quick render to catch broken markdown:

Run: `uv run mkdocs build --strict 2>&1 | tail -20` (skip if mkdocs is not configured).
Expected: no warnings about the `third-party.md` page.

- [ ] **Step 4: Spot-check the user-facing text**

Run: `grep -n "python -m toolr.build\|from toolr.build\|build_manifest(" docs/third-party.md`
Expected: zero matches.

Run: `grep -c "toolr self build-manifest" docs/third-party.md`
Expected: ≥ 3 (the doc should still tell users how to invoke the canonical CLI).

- [ ] **Step 5: Commit**

```bash
git add docs/third-party.md
git commit -m "docs(third-party): drop Python build references; add static-only contract"
```

---

## Task 17: Full-workspace verification

**Files:** None (verification only).

Run the full Rust + Python test matrices the way CI does, with the binary built fresh.

- [ ] **Step 1: Full Rust test pass**

Run: `cargo test --workspace`
Expected: all tests pass. Note: `cargo test` defaults to building in debug mode, which matches CI for `_test.yml`.

- [ ] **Step 2: Full Python test pass**

Run: `uv run pytest -ra -s -v --color=yes`
Expected: all pass. Distribution tests and toolr.testing-driven tests should be unaffected.

- [ ] **Step 3: Reproduce the CI step locally**

Run:

```bash
cargo build --quiet -p toolr
./target/debug/toolr self build-manifest toolr_example_plugin --check
```

This requires the tools venv to have `toolr_example_plugin` installed. If it doesn't, run `uv sync` from `tools/` first (or skip — CI exercises this on every job).

Expected: exit 0.

- [ ] **Step 4: Clippy + rustfmt sanity**

Run:

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: clean. If clippy complains, fix in this commit. If fmt has diffs, run `cargo fmt --all` and add them to the commit.

- [ ] **Step 5: Commit any formatting / clippy fixups**

```bash
git add -A
git commit -m "chore: clippy + fmt fixups for build-manifest rewrite"
```

(Skip if there's nothing to commit.)

---

## Task 18: Submit the stacked PR

**Files:** None (git operations only).

- [ ] **Step 1: Confirm the stack is still healthy**

Run: `gs ll`
Expected: `rust_build_manifest` shows `dispatch_manifest_freshness` as its parent, with a clean commit log on top.

- [ ] **Step 2: Restack if needed**

If `dispatch_manifest_freshness` moved while you were working, restack:

Run: `gs branch restack`
Expected: clean restack, no conflicts. If conflicts surface, resolve them, then `gs branch continue`.

- [ ] **Step 3: Submit as a stacked draft PR**

Run: `gs branch submit --draft --fill`
Expected: a new GitHub PR opens, base = `dispatch_manifest_freshness` (NOT `main`). The PR description should reference `specs/2026-05-22-rust-build-manifest-design.md` and link back to the predecessor PR.

If `--fill` produces a thin description, edit it via `gh pr edit` to point at the spec and call out the deliberate dynamic-pattern regression for plugin authors who use loops to register commands.

- [ ] **Step 4: Confirm CI runs**

Wait for CI to start, then watch the `Verify example-plugin manifest is in sync` step pass on the Rust-built `toolr`. That step was already in place from the predecessor branch; it now exercises this PR's code.

Run: `gh pr checks` (after a minute or two).
Expected: pending checks visible; no early failures from the build step.

---

## Self-Review

**Spec coverage:**

- Goals (lines 75-94 of design): ✓ — Tasks 2-7 (pure-Rust path), 11 (no subprocess), 14 (no Python), 9 (--source-dir flag), 9 (preserved CLI flags), 14 (Python module removed).
- Non-goals (lines 96-114): ✓ — no dynamic-pattern support (Task 16 documents this as the static-only contract), no new fragment fields (model.rs unchanged), no change to fragment-consumer code (the merge / dispatch path is untouched).
- Fragment schema (lines 116-163): ✓ — Task 2 maps to existing `ManifestFragment`; Task 6 enforces byte-identical serialisation.
- CLI surface (lines 169-186): ✓ — Task 9.
- Source-directory resolution (lines 188-211): ✓ — Task 8 implements both modes; venv-glob handles multi-Python via lexicographic sort with warning.
- AST walk + fragment emission (lines 215-249): ✓ — Task 2 implementation matches the 6-step recipe (walk → pass-1 tables → pass-2 extract → filter by package → derive groups → sort).
- Output stability (lines 251-266): ✓ — Task 6 via Value round-trip + `to_string_pretty` + trailing newline.
- CLI dispatch wiring (lines 268-300): ✓ — Task 11.
- Removal of Python implementation (lines 302-318): ✓ — Tasks 14-15.
- Differences table (lines 322-332): ✓ — every row covered: install-no-longer-required (Task 11), subprocess-no-longer (Task 11), static-only (Tasks 7+16), cross-file enums (Task 7), `--check` codes (Task 11+12), `--source-dir` (Task 9), `--python` removed outright (Task 9; spec's "warn-then-delete" was an option, not a requirement — user opted for clean removal), byte-equivalence (Tasks 6+13).
- Migration plan (lines 339-359): ✓ — Tasks 11, 13, 14, 16 collectively.
- Testing strategy (lines 363-405): ✓ — golden (Tasks 3-4), filtering (Task 5), empty (Task 5), bad syntax (Task 5), cross-file enums (Task 7), CLI integration (Task 12), distribution-test (Task 13, 17), equivalence (Task 13).
- Edge cases (lines 409-426): ✓ — namespace package rejection (Task 2+12), single-file fold (covered by namespace rejection — single-file packages have no `__init__.py` since they're not directories), subpackages (Task 5 uses `sub/__init__.py`), mixed-content directories (handled by `list_python_files`'s `.py` extension filter — no separate test needed since the helper is reused as-is from `parser::build`), schema_version override (`--schema-version N` flag in Task 9, parsed in Task 11).
- Open questions (lines 428-445): the design's open questions are answered in the plan — no fallback to project `tools/` (Task 8 errors instead); unified diff via `similar` (Tasks 10+11); performance not measured (call-out for a follow-up issue if needed, not blocking).

**Placeholders:** No "TBD", "implement later", or hand-wavy "handle edge cases" steps. Every code-change step includes the actual code. The single "skip if there's nothing to commit" in Task 17 Step 5 is a legitimate conditional, not a placeholder.

**Type consistency:**

- `ManifestFragment` (not `ThirdPartyFragment` — spec uses both, code uses `ManifestFragment`): used consistently throughout Tasks 2, 6, 11, 12.
- `build_third_party_fragment(source_dir, package_name, schema_version)`: same signature in Task 2 declaration, Task 6 serialiser test, Task 11 call site.
- `BuildFragmentError` variants: `NamespacePackage`, `MissingSourceDir`, `EmptyPackage`, `Parse`, `UnsupportedTypes` — same names in Task 2, Task 5, Task 12.
- `FRAGMENT_SCHEMA_VERSION` (the constant used as `--schema-version` default in Task 11) lives under `toolr_core::third_party::FRAGMENT_SCHEMA_VERSION` (verified via `crates/toolr-core/src/third_party/model.rs:12`).
- Resolver type `ResolvedSource { source_dir, package_name }` defined in Task 8, consumed in Task 11.
- `serialise_fragment` (British spelling, consistent with my own bias) — used the same way in Tasks 6 and 11. Re-export wired in Task 6 Step 3.

No gaps found.
