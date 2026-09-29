# Path-State Types Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `arg(must_exist=, must_be_file=, must_be_dir=)` with seven `toolr.types` path
types. The Rust binary checks each one at CLI parse time, and the command receives a
`pathlib.Path`.

**Architecture:**

- **Types.** Each type is a new `SupportedType` variant. The static parser resolves it by name,
  and `crates/toolr/src/value_parsers.rs` gives it a clap value parser built from a
  `(PathForm, PathCheck)` rule.
- **Python side.** The types are chained `typing.NewType`s, so they are distinct for type checkers
  and plain `pathlib.Path` at run time.
- **Removal.** `PathConstraints` is deleted end to end. The local manifest schema goes to 2, and
  freshness rebuilds any cache with a different schema version.

**Tech Stack:** Rust (clap 4, serde, libc, tempfile, ruff_python_ast), Python 3.11+ (msgspec,
mypy), pytest, assert_cmd, mkdocs.

**Spec:** `specs/2026-09-29-path-state-types-design.md`

## Global Constraints

- **The seven types, with these exact names and `NewType` parents:**
    - `AbsolutePath(Path)` and `NewPath(AbsolutePath)`;
    - `ResolvedPath(Path)`, `FilePath(ResolvedPath)` and `DirectoryPath(ResolvedPath)`;
    - `ExecutablePath(FilePath)` and `WritableDirectoryPath(DirectoryPath)`.
- **Serde names** (`rename_all = "snake_case"`): `new_path`, `file_path`, `directory_path`,
  `executable_path` and `writable_directory_path`.
- **Error strings are exact.** `<typed>` means the value exactly as the user typed it.
    - `path does not exist: <typed>`
    - `path is not a regular file: <typed>`
    - `path is not a directory: <typed>`
    - `path is not executable: <typed>`
    - `directory is not writable: <typed>`
    - `path already exists: <typed>`
    - `parent directory does not exist: <absolute parent>`
- **Unknown-keyword hints are exact.** `must_exist` and `path_must_exist` give
  `` use `toolr.types.ResolvedPath` instead ``. `must_be_file` and `path_must_be_file` give
  `` use `toolr.types.FilePath` instead ``. `must_be_dir` and `path_must_be_dir` give
  `` use `toolr.types.DirectoryPath` instead ``.
- **Schema versions.** `manifest::SCHEMA_VERSION` goes 1 → 2. `FRAGMENT_SCHEMA_VERSION` stays 1.
  `RUNNER_SCHEMA_VERSION` and the Python `SCHEMA_VERSION` stay 3.
- **Python floor:** `requires-python = ">=3.11"`. No `pathlib.Path` subclasses.
- **Before every commit:** run `git-spice repo sync` and then `git-spice upstack restack`.
- **Staging:** stage files explicitly by path. Never run `git add -A` or `git add .`, because
  `audit/` is local-only. Check `git diff --cached --name-only` before committing.
- **Commit messages:** Conventional Commits. No `Co-Authored-By` trailer, and no `--no-verify`.
- **Repo hygiene:** no employer name and no absolute `/Users/...` paths in the repo.
- **Language:** British English in prose, comments and commit messages.

## Review Focus

1. **A path variant missing from `execute_build.rs` extraction.** clap stores path values as
   `PathBuf`. If a new variant fell through to `get_one::<String>`, clap would panic with
   "Mismatch between definition and access". Pinned by Task 2's per-kind extraction test.
2. **Containers.** `list[FilePath]`, `DirectoryPath | None` and `*rest: ExecutablePath` must
   resolve, be checked, and be extracted as paths. Pinned by Task 1 (build test), Task 2
   (`extract_many`) and Task 5 (runner coercion).
3. **Symlinks and `..`.** `FilePath` given `sub/../f.txt` returns the canonical file. A symlink to a
   directory is accepted as a `DirectoryPath`, and the canonical target comes back. Pinned by
   Task 1.
4. **A dangling symlink handed to `NewPath`.** It counts as existing (`path already exists`),
   because writing through it would create its target. Pinned by Task 1.
5. **A literal default that names a missing file.** `config: FilePath = "missing.toml"` with the
   flag omitted must be a clap error, not a silent pass. Pinned by Task 1. If clap turns out not
   to validate defaults, record the finding in the spec's "Known limit" section; don't weaken the
   test.

---

### Task 1: Path-state types in the Rust front end

**Files:**

- Modify: `crates/toolr-core/src/parser/types/supported.rs` (variants, `doc()`, `kind()`,
  `supported_type_kinds!`, `representative()`, new `is_path()`, the test name list)
- Modify: `crates/toolr-core/src/parser/types/resolve.rs:382-396` (`resolve_toolr_types_name`)
- Modify: `crates/toolr-core/src/parser/types/mod.rs:490-504` (the
  `toolr_types_names_match_python_surface` list)
- Modify: `crates/toolr-core/src/parser/build.rs` (tests module: a resolution test for the new
  shapes)
- Modify: `crates/toolr/src/value_parsers.rs` (path rule, parser, hints, tests)
- Modify: `crates/toolr/Cargo.toml` (add `libc.workspace = true` under `[dependencies]`)
- Regenerate: `docs/writing-commands/files/supported-types.md`,
  `skills/toolr-command-authoring/references/types.md`

**Interfaces:**

- Produces: `SupportedType::{NewPath, FilePath, DirectoryPath, ExecutablePath, WritableDirectoryPath}`;
  `SupportedTypeKind::` variants of the same names; `pub fn SupportedType::is_path(&self) -> bool`.
- Produces (crate-private, `value_parsers.rs`):
    - `enum PathForm { AsTyped, Absolute, Canonical }`
    - `enum PathCheck { None, Exists, File, Dir, Executable, WritableDir, New }`
    - `fn path_rule(ty: &SupportedType) -> Option<(PathForm, PathCheck)>`
- `apply_value_parser(arg, ty, path_constraints)` keeps its signature in this task. Task 3 removes
  the third parameter.
- [ ] **Step 1: Write the failing core tests**

In `crates/toolr-core/src/parser/types/supported.rs`, replace the `names` array in
`catalogue_covers_every_toolr_types_name` with:

```rust
        let names = [
            "AbsolutePath", "Count", "Date", "DateTime", "DirectoryPath", "Email",
            "ExecutablePath", "FilePath", "IPv4", "IPv6", "NewPath", "ResolvedPath",
            "Time", "UUID", "Version", "WritableDirectoryPath",
        ];
```

Then add this test to the same `tests` module:

```rust
    #[test]
    fn is_path_is_true_exactly_for_the_path_kinds() {
        let paths: Vec<SupportedTypeKind> = SupportedTypeKind::ALL
            .iter()
            .copied()
            .filter(|k| k.representative().is_path())
            .collect();
        assert_eq!(
            paths,
            [
                SupportedTypeKind::Path,
                SupportedTypeKind::AbsolutePath,
                SupportedTypeKind::NewPath,
                SupportedTypeKind::ResolvedPath,
                SupportedTypeKind::FilePath,
                SupportedTypeKind::DirectoryPath,
                SupportedTypeKind::ExecutablePath,
                SupportedTypeKind::WritableDirectoryPath,
            ]
        );
    }
```

In `crates/toolr-core/src/parser/types/mod.rs`, replace the `names` array in
`toolr_types_names_match_python_surface` with the same 16 names as above, one per line, in the
existing style.

In `crates/toolr-core/src/parser/build.rs`, add to the `tests` module, next to `assert_builds`:

```rust
    #[test]
    fn path_state_types_resolve_bare_in_containers_and_as_varargs() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "tools/paths.py",
            r#"from toolr import Context, command_group
from toolr.types import DirectoryPath, ExecutablePath, FilePath, NewPath, WritableDirectoryPath

group = command_group("paths", "Paths", description="Paths.")


@group.command
def run(
    ctx: Context,
    config: FilePath,
    *rest: ExecutablePath,
    inputs: list[DirectoryPath],
    output: NewPath | None = None,
    scratch: WritableDirectoryPath | None = None,
) -> None:
    """Run.

    Args:
        config: Config file.
        rest: Tools to run.
        inputs: Input dirs.
        output: Output file.
        scratch: Scratch dir.
    """
"#,
        );
        let manifest = build_static_manifest(&tmp.path().join("tools")).unwrap();
        let cmd = manifest.commands.iter().find(|c| c.name == "run").unwrap();
        let ty = |name: &str| {
            cmd.arguments
                .iter()
                .find(|a| a.name == name)
                .unwrap_or_else(|| panic!("no argument {name}"))
                .resolved_type
                .clone()
        };
        assert_eq!(ty("config"), Some(SupportedType::FilePath));
        assert_eq!(ty("rest"), Some(SupportedType::ExecutablePath));
        assert_eq!(
            ty("inputs"),
            Some(SupportedType::List(Box::new(SupportedType::DirectoryPath)))
        );
        assert_eq!(
            ty("output"),
            Some(SupportedType::Optional(Box::new(SupportedType::NewPath)))
        );
        assert_eq!(
            ty("scratch"),
            Some(SupportedType::Optional(Box::new(SupportedType::WritableDirectoryPath)))
        );
    }
```

Add `use crate::parser::SupportedType;` to that `tests` module's imports if it's not already in
scope. Also check that `write`, `TempDir` and `build_static_manifest` are in scope; the
existing `assert_builds` uses all three.

- [ ] **Step 2: Run the core tests to verify they fail**

Run: `cargo test -p toolr-core -- supported:: toolr_types_names_match path_state_types_resolve`

Expected: FAIL to compile: `no variant or associated item named FilePath found for enum
SupportedType` (and the same for the other new names).

- [ ] **Step 3: Add the variants and their catalogue entries**

In `supported.rs`, replace the three path variants in `enum SupportedType` with:

```rust
    /// `pathlib.Path`: string passes through unchanged.
    Path,
    /// `toolr.types.AbsolutePath`: absolutised against cwd, no fs check.
    AbsolutePath,
    /// `toolr.types.NewPath`: absolutised; must not exist, parent dir must.
    NewPath,
    /// `toolr.types.ResolvedPath`: canonicalised, must exist.
    ResolvedPath,
    /// `toolr.types.FilePath`: canonicalised, must be a regular file.
    FilePath,
    /// `toolr.types.DirectoryPath`: canonicalised, must be a directory.
    DirectoryPath,
    /// `toolr.types.ExecutablePath`: canonicalised, executable regular file.
    ExecutablePath,
    /// `toolr.types.WritableDirectoryPath`: canonicalised, writable directory.
    WritableDirectoryPath,
```

In `doc()`, after the `SupportedType::AbsolutePath` arm, add:

```rust
            SupportedType::NewPath => TypeDoc {
                annotation: "toolr.types.NewPath",
                validated_by: "clap (must not exist; parent dir must)",
                wire_format: "absolute string",
                python_receives: "`pathlib.Path`",
                note: "",
            },
```

After the `SupportedType::ResolvedPath` arm, add:

```rust
            SupportedType::FilePath => TypeDoc {
                annotation: "toolr.types.FilePath",
                validated_by: "clap (`canonicalize()`, regular file)",
                wire_format: "resolved string",
                python_receives: "`pathlib.Path`",
                note: "",
            },
            SupportedType::DirectoryPath => TypeDoc {
                annotation: "toolr.types.DirectoryPath",
                validated_by: "clap (`canonicalize()`, directory)",
                wire_format: "resolved string",
                python_receives: "`pathlib.Path`",
                note: "",
            },
            SupportedType::ExecutablePath => TypeDoc {
                annotation: "toolr.types.ExecutablePath",
                validated_by: "clap (`canonicalize()`, executable file)",
                wire_format: "resolved string",
                python_receives: "`pathlib.Path`",
                note: "",
            },
            SupportedType::WritableDirectoryPath => TypeDoc {
                annotation: "toolr.types.WritableDirectoryPath",
                validated_by: "clap (`canonicalize()`, writable directory)",
                wire_format: "resolved string",
                python_receives: "`pathlib.Path`",
                note: "",
            },
```

In `kind()`, add five arms next to the existing path arms:

```rust
            SupportedType::NewPath => SupportedTypeKind::NewPath,
            SupportedType::FilePath => SupportedTypeKind::FilePath,
            SupportedType::DirectoryPath => SupportedTypeKind::DirectoryPath,
            SupportedType::ExecutablePath => SupportedTypeKind::ExecutablePath,
            SupportedType::WritableDirectoryPath => SupportedTypeKind::WritableDirectoryPath,
```

In `supported_type_kinds!(...)`, replace `Path, AbsolutePath, ResolvedPath,` with:

```rust
    Path,
    AbsolutePath,
    NewPath,
    ResolvedPath,
    FilePath,
    DirectoryPath,
    ExecutablePath,
    WritableDirectoryPath,
```

In `representative()`, add:

```rust
            SupportedTypeKind::NewPath => SupportedType::NewPath,
            SupportedTypeKind::FilePath => SupportedType::FilePath,
            SupportedTypeKind::DirectoryPath => SupportedType::DirectoryPath,
            SupportedTypeKind::ExecutablePath => SupportedType::ExecutablePath,
            SupportedTypeKind::WritableDirectoryPath => SupportedType::WritableDirectoryPath,
```

After `unwrap_optional`, add `is_path()`. It is exhaustive with no `_` arm, so a future variant
has to decide:

```rust
    /// Whether clap stores this type's value as a `PathBuf`. Exhaustive, so
    /// a new variant must decide; `execute_build.rs` relies on this to read
    /// path values back with the right type.
    pub fn is_path(&self) -> bool {
        match self {
            SupportedType::Path
            | SupportedType::AbsolutePath
            | SupportedType::NewPath
            | SupportedType::ResolvedPath
            | SupportedType::FilePath
            | SupportedType::DirectoryPath
            | SupportedType::ExecutablePath
            | SupportedType::WritableDirectoryPath => true,
            SupportedType::Str
            | SupportedType::Int
            | SupportedType::Float
            | SupportedType::Bool
            | SupportedType::DateTime
            | SupportedType::Date
            | SupportedType::Time
            | SupportedType::Uuid
            | SupportedType::Ipv4
            | SupportedType::Ipv6
            | SupportedType::Email
            | SupportedType::Version
            | SupportedType::Count
            | SupportedType::Literal(_)
            | SupportedType::Enum { .. }
            | SupportedType::List(_)
            | SupportedType::Tuple(_)
            | SupportedType::Optional(_) => false,
        }
    }
```

In `resolve.rs::resolve_toolr_types_name`, after the `"ResolvedPath"` arm, add:

```rust
        "NewPath" => Ok(SupportedType::NewPath),
        "FilePath" => Ok(SupportedType::FilePath),
        "DirectoryPath" => Ok(SupportedType::DirectoryPath),
        "ExecutablePath" => Ok(SupportedType::ExecutablePath),
        "WritableDirectoryPath" => Ok(SupportedType::WritableDirectoryPath),
```

- [ ] **Step 4: Run the core tests to verify they pass**

Run: `cargo test -p toolr-core -- supported:: toolr_types_names_match path_state_types_resolve`

Expected: PASS. The workspace doesn't compile yet: `crates/toolr` fails with
`non-exhaustive patterns` in `apply_value_parser`. Steps 5–8 fix that.

- [ ] **Step 5: Write the failing value-parser tests**

In `crates/toolr/src/value_parsers.rs`, add to the `tests` module (after the existing helpers):

```rust
    use std::fs;
    use tempfile::TempDir;
    use toolr_core::parser::types::SupportedTypeKind;

    fn parse(ty: &SupportedType, value: &str) -> Result<PathBuf, String> {
        build_command_with(ty)
            .try_get_matches_from(["test", "--v", value])
            .map(|m| m.get_one::<PathBuf>("v").unwrap().clone())
            .map_err(|e| e.to_string())
    }

    fn s(p: &std::path::Path) -> &str {
        p.to_str().unwrap()
    }

    #[test]
    fn every_path_kind_has_a_path_rule_and_no_other_kind_does() {
        for kind in SupportedTypeKind::ALL {
            let ty = kind.representative();
            assert_eq!(ty.is_path(), path_rule(&ty).is_some(), "{kind:?}");
        }
    }

    #[test]
    fn file_path_canonicalises_dot_dot_segments() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("sub")).unwrap();
        fs::write(tmp.path().join("f.txt"), "x").unwrap();
        let typed = tmp.path().join("sub").join("..").join("f.txt");
        let got = parse(&SupportedType::FilePath, s(&typed)).unwrap();
        assert_eq!(got, tmp.path().join("f.txt").canonicalize().unwrap());
    }

    #[test]
    fn file_path_rejects_a_directory_naming_the_typed_path() {
        let tmp = TempDir::new().unwrap();
        let err = parse(&SupportedType::FilePath, s(tmp.path())).unwrap_err();
        assert!(
            err.contains(&format!("path is not a regular file: {}", s(tmp.path()))),
            "got: {err}"
        );
    }

    #[test]
    fn canonical_types_reject_a_missing_path_as_does_not_exist() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("missing");
        for ty in [
            SupportedType::ResolvedPath,
            SupportedType::FilePath,
            SupportedType::DirectoryPath,
            SupportedType::ExecutablePath,
            SupportedType::WritableDirectoryPath,
        ] {
            let err = parse(&ty, s(&missing)).unwrap_err();
            assert!(
                err.contains(&format!("path does not exist: {}", s(&missing))),
                "{ty:?} got: {err}"
            );
        }
    }

    #[test]
    fn directory_path_rejects_a_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("f.txt");
        fs::write(&file, "x").unwrap();
        let err = parse(&SupportedType::DirectoryPath, s(&file)).unwrap_err();
        assert!(
            err.contains(&format!("path is not a directory: {}", s(&file))),
            "got: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn directory_path_accepts_a_symlink_to_a_directory_and_returns_the_target() {
        let tmp = TempDir::new().unwrap();
        let real = tmp.path().join("real");
        fs::create_dir(&real).unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let got = parse(&SupportedType::DirectoryPath, s(&link)).unwrap();
        assert_eq!(got, real.canonicalize().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn executable_path_follows_the_mode_bits() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let tool = tmp.path().join("tool");
        fs::write(&tool, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o644)).unwrap();
        let err = parse(&SupportedType::ExecutablePath, s(&tool)).unwrap_err();
        assert!(
            err.contains(&format!("path is not executable: {}", s(&tool))),
            "got: {err}"
        );
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(parse(&SupportedType::ExecutablePath, s(&tool)).is_ok());
    }

    #[test]
    fn executable_path_rejects_a_directory_as_not_a_file() {
        let tmp = TempDir::new().unwrap();
        let err = parse(&SupportedType::ExecutablePath, s(tmp.path())).unwrap_err();
        assert!(err.contains("path is not a regular file"), "got: {err}");
    }

    #[test]
    fn writable_directory_path_accepts_a_fresh_temp_dir() {
        let tmp = TempDir::new().unwrap();
        let got = parse(&SupportedType::WritableDirectoryPath, s(tmp.path())).unwrap();
        assert_eq!(got, tmp.path().canonicalize().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn writable_directory_path_rejects_a_read_only_directory() {
        use std::os::unix::fs::PermissionsExt;
        // root passes access(W_OK) regardless of mode bits.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let tmp = TempDir::new().unwrap();
        let ro = tmp.path().join("ro");
        fs::create_dir(&ro).unwrap();
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o555)).unwrap();
        let err = parse(&SupportedType::WritableDirectoryPath, s(&ro)).unwrap_err();
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            err.contains(&format!("directory is not writable: {}", s(&ro))),
            "got: {err}"
        );
    }

    #[test]
    fn new_path_accepts_a_missing_file_in_an_existing_dir() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("out.txt");
        assert_eq!(parse(&SupportedType::NewPath, s(&target)).unwrap(), target);
    }

    #[test]
    fn new_path_absolutises_a_relative_path() {
        let got = parse(&SupportedType::NewPath, "not-here-4f1c2a.txt").unwrap();
        assert!(got.is_absolute(), "got: {}", got.display());
        assert!(got.ends_with("not-here-4f1c2a.txt"));
    }

    #[test]
    fn new_path_rejects_an_existing_path() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("f.txt");
        fs::write(&file, "x").unwrap();
        let err = parse(&SupportedType::NewPath, s(&file)).unwrap_err();
        assert!(
            err.contains(&format!("path already exists: {}", s(&file))),
            "got: {err}"
        );
    }

    #[test]
    fn new_path_rejects_a_missing_parent() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("missing").join("out.txt");
        let err = parse(&SupportedType::NewPath, s(&target)).unwrap_err();
        assert!(
            err.contains(&format!(
                "parent directory does not exist: {}",
                tmp.path().join("missing").display()
            )),
            "got: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn new_path_treats_a_dangling_symlink_as_existing() {
        let tmp = TempDir::new().unwrap();
        let link = tmp.path().join("dangling");
        std::os::unix::fs::symlink(tmp.path().join("nowhere"), &link).unwrap();
        let err = parse(&SupportedType::NewPath, s(&link)).unwrap_err();
        assert!(err.contains("path already exists"), "got: {err}");
    }

    #[test]
    fn path_hints_follow_the_type() {
        use clap::ValueHint;
        for (ty, hint) in [
            (SupportedType::Path, ValueHint::AnyPath),
            (SupportedType::NewPath, ValueHint::AnyPath),
            (SupportedType::ResolvedPath, ValueHint::AnyPath),
            (SupportedType::FilePath, ValueHint::FilePath),
            (SupportedType::DirectoryPath, ValueHint::DirPath),
            (SupportedType::ExecutablePath, ValueHint::ExecutablePath),
            (SupportedType::WritableDirectoryPath, ValueHint::DirPath),
        ] {
            let arg = apply_value_parser(Arg::new("v").long("v"), &ty, None);
            assert_eq!(arg.get_value_hint(), hint, "{ty:?}");
        }
    }

    #[test]
    fn a_literal_default_naming_a_missing_file_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("missing.toml");
        let cmd = Command::new("test").arg(
            apply_value_parser(Arg::new("v").long("v"), &SupportedType::FilePath, None)
                .default_value(s(&missing).to_string()),
        );
        let err = cmd.try_get_matches_from(["test"]).unwrap_err();
        assert!(err.to_string().contains("path does not exist"), "got: {err}");
    }

    #[test]
    fn list_of_file_paths_checks_every_element() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("f.txt");
        fs::write(&file, "x").unwrap();
        let ty = SupportedType::List(Box::new(SupportedType::FilePath));
        let cmd = Command::new("test").arg(
            apply_value_parser(Arg::new("v").long("v"), &ty, None).action(clap::ArgAction::Append),
        );
        let err = cmd
            .try_get_matches_from(["test", "--v", s(&file), "--v", s(tmp.path())])
            .unwrap_err();
        assert!(err.to_string().contains("path is not a regular file"), "got: {err}");
    }
```

- [ ] **Step 6: Run the value-parser tests to verify they fail**

Run: `cargo test -p toolr value_parsers`

Expected: FAIL to compile: `cannot find function path_rule` and `non-exhaustive patterns`.

- [ ] **Step 7: Implement the path rule and parser**

In `crates/toolr/Cargo.toml`, under `[dependencies]` and after `humansize.workspace = true`, add
`libc.workspace = true`.

In `crates/toolr/src/value_parsers.rs`, replace the hint `match` (the one covering
`SupportedType::Path | SupportedType::AbsolutePath | SupportedType::ResolvedPath`) with:

```rust
    let arg = match path_rule(inner) {
        Some((_, check)) => arg.value_hint(path_hint(with_legacy_constraints(check, pc))),
        None if matches!(inner, SupportedType::Email) => arg.value_hint(ValueHint::EmailAddress),
        None => arg,
    };
```

In the value-parser `match inner`, replace the three path arms with one arm for every path
variant:

```rust
        SupportedType::Path
        | SupportedType::AbsolutePath
        | SupportedType::NewPath
        | SupportedType::ResolvedPath
        | SupportedType::FilePath
        | SupportedType::DirectoryPath
        | SupportedType::ExecutablePath
        | SupportedType::WritableDirectoryPath => {
            let (form, check) = path_rule(inner).expect("path variant has a rule");
            arg.value_parser(path_parser(form, with_legacy_constraints(check, pc)))
        }
```

Replace the old `path_parser` function and its doc comment with:

```rust
/// How a path type shapes the value it hands to Python.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathForm {
    AsTyped,
    Absolute,
    Canonical,
}

/// What a path type checks on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathCheck {
    None,
    Exists,
    File,
    Dir,
    Executable,
    WritableDir,
    New,
}

/// The form and check for a path type; `None` for non-path types. The
/// `every_path_kind_has_a_path_rule_and_no_other_kind_does` test keeps
/// this in step with `SupportedType::is_path`.
fn path_rule(ty: &SupportedType) -> Option<(PathForm, PathCheck)> {
    Some(match ty {
        SupportedType::Path => (PathForm::AsTyped, PathCheck::None),
        SupportedType::AbsolutePath => (PathForm::Absolute, PathCheck::None),
        SupportedType::NewPath => (PathForm::Absolute, PathCheck::New),
        SupportedType::ResolvedPath => (PathForm::Canonical, PathCheck::Exists),
        SupportedType::FilePath => (PathForm::Canonical, PathCheck::File),
        SupportedType::DirectoryPath => (PathForm::Canonical, PathCheck::Dir),
        SupportedType::ExecutablePath => (PathForm::Canonical, PathCheck::Executable),
        SupportedType::WritableDirectoryPath => (PathForm::Canonical, PathCheck::WritableDir),
        _ => return None,
    })
}

// Removed with `PathConstraints` in the next task.
fn with_legacy_constraints(check: PathCheck, pc: PathConstraints) -> PathCheck {
    if check != PathCheck::None {
        check
    } else if pc.must_be_dir {
        PathCheck::Dir
    } else if pc.must_be_file {
        PathCheck::File
    } else if pc.must_exist {
        PathCheck::Exists
    } else {
        PathCheck::None
    }
}

fn path_hint(check: PathCheck) -> ValueHint {
    match check {
        PathCheck::File => ValueHint::FilePath,
        PathCheck::Dir | PathCheck::WritableDir => ValueHint::DirPath,
        PathCheck::Executable => ValueHint::ExecutablePath,
        PathCheck::None | PathCheck::Exists | PathCheck::New => ValueHint::AnyPath,
    }
}

/// Error messages name the path as the user typed it.
fn path_parser(form: PathForm, check: PathCheck) -> ValueParser {
    ValueParser::new(move |s: &str| -> Result<PathBuf, String> {
        let typed = std::path::Path::new(s);
        let path = match form {
            PathForm::AsTyped => typed.to_path_buf(),
            PathForm::Absolute => absolutise(typed)?,
            PathForm::Canonical => {
                if !typed.exists() {
                    return Err(format!("path does not exist: {s}"));
                }
                typed
                    .canonicalize()
                    .map_err(|e| format!("invalid path `{s}`: {e}"))?
            }
        };
        check_path(&path, check, s)?;
        Ok(path)
    })
}

fn absolutise(path: &std::path::Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let cwd = std::env::current_dir().map_err(|e| format!("could not resolve cwd: {e}"))?;
    Ok(cwd.join(path))
}

fn check_path(path: &std::path::Path, check: PathCheck, typed: &str) -> Result<(), String> {
    let require = |ok: bool, msg: &str| if ok { Ok(()) } else { Err(format!("{msg}: {typed}")) };
    match check {
        PathCheck::None => Ok(()),
        PathCheck::Exists => require(path.exists(), "path does not exist"),
        PathCheck::File => require(path.is_file(), "path is not a regular file"),
        PathCheck::Dir => require(path.is_dir(), "path is not a directory"),
        PathCheck::Executable => {
            require(path.is_file(), "path is not a regular file")?;
            require(is_executable(path), "path is not executable")
        }
        PathCheck::WritableDir => {
            require(path.is_dir(), "path is not a directory")?;
            require(is_writable_dir(path), "directory is not writable")
        }
        PathCheck::New => {
            // `symlink_metadata` so a dangling symlink counts as existing:
            // writing through it would create its target.
            require(path.symlink_metadata().is_err(), "path already exists")?;
            match path.parent() {
                Some(parent) if !parent.is_dir() => Err(format!(
                    "parent directory does not exist: {}",
                    parent.display()
                )),
                _ => Ok(()),
            }
        }
    }
}

#[cfg(unix)]
fn access_ok(path: &std::path::Path, mode: libc::c_int) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `c_path` is NUL-terminated and outlives the call.
    unsafe { libc::access(c_path.as_ptr(), mode) == 0 }
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    access_ok(path, libc::X_OK)
}

#[cfg(unix)]
fn is_writable_dir(path: &std::path::Path) -> bool {
    access_ok(path, libc::W_OK)
}

#[cfg(windows)]
fn is_executable(path: &std::path::Path) -> bool {
    let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    exts.split(';')
        .any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(ext))
}

// Directory ACLs, not the read-only attribute, decide writability on Windows.
#[cfg(windows)]
fn is_writable_dir(path: &std::path::Path) -> bool {
    tempfile::tempfile_in(path).is_ok()
}
```

If clippy flags `_ => return None` in `path_rule` (wildcard on an enum), keep it. The test in
Step 5 is what enforces completeness here.

- [ ] **Step 8: Run the value-parser tests and the workspace build**

Run: `cargo test -p toolr value_parsers && cargo test -p toolr-core`

Expected: PASS, including the existing `path_with_must_*`, `absolute_path_parser_*` and
`resolved_path_parser_requires_existence` tests.

If `a_literal_default_naming_a_missing_file_is_rejected` fails because clap doesn't validate
defaults, stop. Report it as a finding, and update the spec's "Known limit: non-literal
defaults" section to cover literal defaults too. Don't delete the test; flip its assertion to
document the actual behaviour, and name the issue in a comment.

- [ ] **Step 9: Regenerate the type tables**

Run: `cargo xtask build-skill-refs`

Expected: `docs/writing-commands/files/supported-types.md` and
`skills/toolr-command-authoring/references/types.md` gain five rows. Then run
`cargo xtask build-skill-refs --check`, which is expected to exit 0.

- [ ] **Step 10: Commit**

```bash
git-spice repo sync && git-spice upstack restack
git add crates/toolr-core/src/parser/types/supported.rs crates/toolr-core/src/parser/types/resolve.rs \
  crates/toolr-core/src/parser/types/mod.rs crates/toolr-core/src/parser/build.rs \
  crates/toolr/src/value_parsers.rs crates/toolr/Cargo.toml Cargo.lock \
  docs/writing-commands/files/supported-types.md skills/toolr-command-authoring/references/types.md
git diff --cached --name-only
git commit -m "feat(types): add path-state types with clap-side checks (#502)"
```

---

### Task 2: Read every path type back as a path

**Files:**

- Modify: `crates/toolr/src/execute_build.rs:403-490` (`extract_scalar`, `extract_many`,
  `arg_has_relative_cli_path`) and its `tests` module

**Interfaces:**

- Consumes: `SupportedType::is_path()` (Task 1), and `crate::value_parsers::apply_value_parser`.
- Produces: no new names.
- [ ] **Step 1: Write the failing extraction tests**

Add to the `tests` module in `crates/toolr/src/execute_build.rs`:

```rust
    use toolr_core::parser::SupportedType;
    use toolr_core::parser::types::SupportedTypeKind;

    fn path_arg(ty: SupportedType, kind: ArgumentKind) -> Argument {
        Argument {
            name: "p".into(),
            kind,
            help: String::new(),
            default: None,
            type_annotation: None,
            resolved_type: Some(ty),
            allowed_values: vec![],
            path_constraints: None,
            metadata: Default::default(),
            long_flag: None,
        }
    }

    #[test]
    fn every_path_kind_is_read_back_as_a_string_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().to_str().unwrap().to_string();
        // `.exe` so the Windows PATHEXT check accepts it too.
        let file = tmp.path().join("f.exe");
        std::fs::write(&file, "x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let file = file.to_str().unwrap().to_string();
        let new = tmp.path().join("new").to_str().unwrap().to_string();
        for kind in SupportedTypeKind::ALL {
            let ty = kind.representative();
            if !ty.is_path() {
                continue;
            }
            let value = match ty {
                SupportedType::NewPath => new.clone(),
                SupportedType::FilePath | SupportedType::ExecutablePath => file.clone(),
                _ => dir.clone(),
            };
            let arg = path_arg(ty.clone(), ArgumentKind::Optional);
            let clap_cmd = clap::Command::new("t").arg(
                crate::value_parsers::apply_value_parser(Arg::new("p").long("p"), &ty, None),
            );
            let matches = clap_cmd.try_get_matches_from(["t", "--p", &value]).unwrap();
            let got = extract_scalar(&arg, &matches);
            assert!(matches!(got, Some(Value::String(_))), "{kind:?} got {got:?}");
        }
    }

    #[test]
    fn list_of_directory_paths_is_read_back_as_strings() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().to_str().unwrap();
        let ty = SupportedType::List(Box::new(SupportedType::DirectoryPath));
        let arg = path_arg(ty.clone(), ArgumentKind::Repeated);
        let clap_cmd = clap::Command::new("t").arg(
            crate::value_parsers::apply_value_parser(Arg::new("p").long("p"), &ty, None)
                .action(ArgAction::Append),
        );
        let matches = clap_cmd
            .try_get_matches_from(["t", "--p", dir, "--p", dir])
            .unwrap();
        assert_eq!(extract_many(&arg, &matches).len(), 2);
    }
```

If `extract_scalar` or `extract_many` aren't visible from `tests` (they are private `fn`s in the
same module, so `use super::*` covers them), nothing more is needed.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p toolr -- execute_build::tests::every_path_kind execute_build::tests::list_of_directory`

Expected: FAIL. Task 1 left extraction unchanged, so the first new variant it hits
(`NewPath`) falls to the `_` arm and panics:
`Mismatch between definition and access of p. Could not downcast to alloc::string::String, need
to downcast to std::path::PathBuf`. The list test fails the same way for `DirectoryPath`.

- [ ] **Step 3: Use `is_path()` in all three places**

In `extract_scalar`, replace the arm

```rust
        Some(
            SupportedType::Path
            | SupportedType::AbsolutePath
            | SupportedType::ResolvedPath,
        ) => matches
```

with

```rust
        Some(ty) if ty.is_path() => matches
```

Make the same replacement in `extract_many`. In `arg_has_relative_cli_path`, replace the
`let is_path = matches!(...)` statement with:

```rust
    let is_path = scalar_element_type(arg).is_some_and(SupportedType::is_path);
```

Update that function's doc comment line "only `SupportedType::Path` / `AbsolutePath` /
`ResolvedPath` count" to "only path types (`SupportedType::is_path`) count".

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p toolr execute_build`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git-spice repo sync && git-spice upstack restack
git add crates/toolr/src/execute_build.rs
git diff --cached --name-only
git commit -m "fix(execute): read every path type back as a path (#502)"
```

---

### Task 3: Remove `arg(must_*)` and `PathConstraints`

**Files:**

- Delete: `crates/toolr-core/src/parser/types/path_constraints.rs`,
  `docs/writing-commands/files/path-constraints.md`
- Modify: `crates/toolr-core/src/parser/types/mod.rs` (module, re-export, tests),
  `crates/toolr-core/src/parser/mod.rs:32-35`, `crates/toolr-core/src/parser/types/resolve.rs`
  (imports, `resolve_one`, `follow_alias_for_path_constraints`),
  `crates/toolr-core/src/parser/types/arg_keywords.rs`,
  `crates/toolr-core/src/parser/types/supported.rs` (`Display` for `UnknownArgKeyword`),
  `crates/toolr-core/src/parser/types/arg_metadata.rs:7` (doc comment),
  `crates/toolr-core/src/manifest/model.rs`, `crates/toolr-core/src/parser/build.rs` (tests),
  `crates/toolr/src/value_parsers.rs`, `crates/toolr/src/cli.rs:772`
- Modify, removing the `path_constraints: None,` line only: `argparse/scan.rs`,
  `complete/tests.rs`, `execute/spec.rs`, `parser/build.rs`, `parser/signatures.rs`,
  `third_party/merge.rs` (all under `crates/toolr-core/src/`); `builtin_completions.rs`,
  `cli.rs` and `execute_build.rs` (under `crates/toolr/src/`)
- Modify: `crates/xtask/src/build_skill_refs/types.rs`, `crates/xtask/src/build_skill_refs/mod.rs`,
  `crates/xtask/tests/coverage.rs`
- Modify: `crates/toolr-py/python/toolr/utils/_signature.py`,
  `tests/utils/signature/test_argument_annotation.py`
- Modify: `docs/writing-commands/arguments.md:179-205`, `docs/writing-commands/annotations.md`
- Regenerate: `skills/toolr-command-authoring/references/{types,arguments,commands}.md`

**Interfaces:**

- Consumes: `path_rule`, `path_parser`, `path_hint` (Task 1).
- Produces: `pub fn apply_value_parser(arg: Arg, ty: &SupportedType) -> Arg`, which drops the
  third parameter, and `pub(super) fn keyword_hint(keyword: &str) -> Option<&'static str>` in
  `arg_keywords.rs`, which replaces `argparse_hint`.
- [ ] **Step 1: Write the failing tests**

In `arg_keywords.rs` tests, replace `suggestion_strips_path_prefix` with:

```rust
    #[test]
    fn removed_path_keywords_point_at_the_path_types() {
        for (keyword, hint) in [
            ("must_exist", "use `toolr.types.ResolvedPath` instead"),
            ("path_must_exist", "use `toolr.types.ResolvedPath` instead"),
            ("must_be_file", "use `toolr.types.FilePath` instead"),
            ("path_must_be_file", "use `toolr.types.FilePath` instead"),
            ("must_be_dir", "use `toolr.types.DirectoryPath` instead"),
            ("path_must_be_dir", "use `toolr.types.DirectoryPath` instead"),
        ] {
            assert_eq!(keyword_hint(keyword), Some(hint), "{keyword}");
            assert_eq!(suggest_arg_keyword(keyword), None, "{keyword}");
        }
    }
```

In `suggestion_within_edit_distance_two`, replace the `must_bee_file` assertion with
`assert_eq!(suggest_arg_keyword("conflict_with").as_deref(), Some("conflicts_with"));`. In
`argparse_keywords_get_a_hint_instead_of_a_suggestion`, change `argparse_hint` to
`keyword_hint` (two call sites).

In `crates/toolr-core/src/parser/build.rs` tests, keep every fixture source exactly as it is.
They are the #500 report shapes. Update only the expectations:

- `unknown_arg_keyword_path_must_exist_fails_build_suggesting_must_exist`: rename it to
  `unknown_arg_keyword_path_must_exist_fails_build_pointing_at_resolved_path`. Expect
  `unknown_keyword("path_must_exist", None)`. Replace the `contains` string with
  ``"unknown `arg()` keyword `path_must_exist`; use `toolr.types.ResolvedPath` instead"``.
- `unknown_arg_keyword_via_module_level_alias_is_reported`: change the fixture keyword
  `must_bee_file` to `must_be_file`, so the alias now carries a removed keyword. Expect
  `unknown_keyword("must_be_file", None)`, and add:

  ```rust
        assert!(errs[0]
            .to_string()
            .ends_with("unknown `arg()` keyword `must_be_file`; use `toolr.types.FilePath` instead"));
  ```

- `unknown_keyword_in_toolr_dot_arg_attribute_form_is_reported`: expect
  `unknown_keyword("path_must_be_dir", None)`.
- `every_bad_arg_keyword_in_the_build_is_reported_together`: expect
  `unknown_keyword("path_must_exist", None)`.
- `assert_path_must_exist_flagged`: filter on `unknown_keyword("path_must_exist", None)`.

Then add one build-level test so the "did you mean" branch stays pinned end to end. The other
tests now take the hint branch instead:

```rust
    #[test]
    fn misspelt_active_arg_keyword_suggests_the_real_one() {
        let errs = type_errors_for(&[(
            "tools/kw.py",
            r#"from typing import Annotated

from toolr import Context, arg, command_group

group = command_group("kw", "Kwarg test", description="Kwarg test.")


@group.command
def read(ctx: Context, *, name: Annotated[str, arg(metvar="NAME")] = "x") -> None:
    """Read."""
"#,
        )]);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert_eq!(errs[0].reason, unknown_keyword("metvar", Some("metavar")));
        assert!(errs[0]
            .to_string()
            .ends_with("unknown `arg()` keyword `metvar` (did you mean `metavar`?)"));
    }
```

In `tests/utils/signature/test_argument_annotation.py`:

- Remove `must_be_file=True,` and `assert annotation.must_be_file is True` from
  `test_arg_accepts_help_section_and_other_new_kwargs`.
- Replace `test_path_constraint_kwargs_land_on_annotation` with:

```python
@pytest.mark.parametrize("kwarg", ["must_exist", "must_be_file", "must_be_dir"])
def test_removed_path_constraint_kwargs_are_rejected(kwarg):
    """The path checks moved to `toolr.types`; `arg()` no longer takes them."""
    with pytest.raises(TypeError, match=kwarg):
        arg(**{kwarg: True})
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
cargo test -p toolr-core -- arg_keywords unknown_arg_keyword unknown_keyword every_bad_arg 2>&1 | tail -20
uv run pytest tests/utils/signature/test_argument_annotation.py -q
```

Expected: Rust fails to compile (`cannot find function keyword_hint`). pytest fails
`test_removed_path_constraint_kwargs_are_rejected` (DID NOT RAISE).

- [ ] **Step 3: Rework the keyword hints**

In `arg_keywords.rs`:

- Remove `"must_exist"`, `"must_be_file"` and `"must_be_dir"` from `ACTIVE_ARG_KEYWORDS`.
- In `suggest_arg_keyword`, delete the `strip_prefix("path_")` block, and change its doc comment
  to "The nearest active keyword within the same `(len / 3).max(2)` edit distance the
  unknown-group hint uses."
- Replace `argparse_hint` with:

```rust
/// Where toolr takes what an `arg()` keyword it doesn't accept would have set:
/// the path checks now live in `toolr.types`, and argparse-style keywords
/// come from the signature.
pub(super) fn keyword_hint(keyword: &str) -> Option<&'static str> {
    match keyword.strip_prefix("path_").unwrap_or(keyword) {
        "must_exist" => return Some("use `toolr.types.ResolvedPath` instead"),
        "must_be_file" => return Some("use `toolr.types.FilePath` instead"),
        "must_be_dir" => return Some("use `toolr.types.DirectoryPath` instead"),
        _ => {}
    }
    match keyword {
        "help" => Some("help text comes from the docstring's `Args:` section"),
        "type" => Some("the type comes from the annotation"),
        "default" => Some("the default comes from the parameter's default value"),
        _ => None,
    }
}
```

- In `check_call`, change `argparse_hint(name)` to `keyword_hint(name)`.
- In `supported.rs`'s `Display for UnsupportedType`, change
  `super::arg_keywords::argparse_hint(keyword)` to `super::arg_keywords::keyword_hint(keyword)`.
- [ ] **Step 4: Delete `PathConstraints` from the Rust crates**

1. Run `git rm crates/toolr-core/src/parser/types/path_constraints.rs`.
2. In `crates/toolr-core/src/parser/types/mod.rs`:
   - Remove `mod path_constraints;` and the
     `pub use path_constraints::{extract_path_constraints, PathConstraintDoc, PathConstraints};`
     line.
   - Delete the test `path_constraints_extract_from_must_kwargs`.
   - In `toolr_arg_calls_accepts_qualified_annotated_head`, delete the
     `assert!(extract_path_constraints(&ann)...` line.
   - In `toolr_arg_calls_ignores_non_annotated_subscripts`, delete the
     `assert_eq!(extract_path_constraints(&ann), None, "{src}");` line.
   - In the `is_toolr_arg_call` doc comment, change "the path-constraints and arg-metadata
     extractors" to "the arg-metadata extractor and the `arg()` keyword checks".
3. In `crates/toolr-core/src/parser/mod.rs`, change the `pub use types::{...}` to
   `pub use types::{SourcesImports, SupportedType, TypeImports, UnsupportedType, resolve as resolve_type};`.
4. In `resolve.rs`:
   - Remove `use super::path_constraints::extract_path_constraints;`, and `PathConstraints` from
     the `use super::{...}` line.
   - Delete the "Path constraints come from…" comment and the two-line `arg.path_constraints = …`
     statement in `resolve_one`.
   - Delete `fn follow_alias_for_path_constraints`.
5. In `crates/toolr-core/src/manifest/model.rs`:
   - Change the import to `use crate::parser::SupportedType;`.
   - Delete the `path_constraints` field, together with its doc comment and
     `#[serde(default, skip_serializing_if = "Option::is_none")]` attribute.
6. In `arg_metadata.rs` line 7, reword the doc sentence that mentions `path_constraints.rs` so it
   no longer names it. `arg_keywords.rs` now does the keyword checks.
7. Delete every `path_constraints: None,` line in one go:

```bash
git grep -l 'path_constraints: None,' -- crates | xargs sed -i '' '/^[[:space:]]*path_constraints: None,$/d'
```

   On Linux, use `sed -i` without `''`.

8. In `crates/toolr/src/value_parsers.rs`:
   - Change the import to `use toolr_core::parser::SupportedType;`.
   - Change the signature to `pub fn apply_value_parser(arg: Arg, ty: &SupportedType) -> Arg`.
   - Delete the `let pc = …` line and `fn with_legacy_constraints`.
   - Replace `path_hint(with_legacy_constraints(check, pc))` with `path_hint(check)`, and
     `path_parser(form, with_legacy_constraints(check, pc))` with `path_parser(form, check)`.
   - Change the `List` arm to `apply_value_parser(arg, elem)`.
   - Remove the `path_constraints` sentences from the function's doc comment.
   - In the tests, drop the third argument from every `apply_value_parser(…, None)` call. Delete
     `build_command_with_constraints` and the three `path_with_must_*` tests; the Task 1 tests
     cover those checks per type now.
9. In `crates/toolr/src/cli.rs:772`, change the call to
   `crate::value_parsers::apply_value_parser(a, ty)`.
10. In `crates/toolr/src/execute_build.rs` tests, drop the `, None` third argument from the two
    `apply_value_parser` calls that Task 2 added.
11. Run `git grep -n "path_constraints\|PathConstraint\|must_exist\|must_be_file\|must_be_dir" -- crates`.
    Expected: hits only in `arg_keywords.rs` and `build.rs` tests (the removed-keyword hints and
    the #500 fixtures), and in `xtask` (handled next).

- [ ] **Step 5: Remove the path-constraint table from xtask**

In `crates/xtask/src/build_skill_refs/types.rs`:

- Change the import to `use toolr_core::parser::types::{SupportedType, TypeDoc};`.
- Delete `render_path_table`, `PATH_CONSTRAINT_APPLIES_TO` and `path_constraints_snippet`.
- In `types_reference`, delete the four `body.push_…` lines that write the "## Path constraints"
  section.
- In the tests, delete the assertions on `arg(must_exist=True)`, `arg(must_be_file=True)`,
  `arg(must_be_dir=True)` and `path_must_`, and delete any test left with no assertions.
- Update the module and function doc comments that mention
  `docs/writing-commands/files/path-constraints.md`.

In `crates/xtask/src/build_skill_refs/mod.rs`, delete `types::path_constraints_snippet(&root)?,`.

In `crates/xtask/tests/coverage.rs`:

- Delete `path_constraints_snippet_uses_real_keywords`.
- In `arguments_reference_is_extracted_and_link_free`, replace
  `assert!(body.contains("must_be_file=True"));` with
  `assert!(body.contains("config: FilePath"));`.

Run `git rm docs/writing-commands/files/path-constraints.md`.

- [ ] **Step 6: Remove the keywords from `arg()`**

In `crates/toolr-py/python/toolr/utils/_signature.py`, delete:

- the `# Path constraints …` comment and the three `must_*` fields on `ArgumentAnnotation`;
- the three `must_*` parameters of `arg()`;
- their three `Args:` docstring entries;
- the three `must_*=` lines in the `ArgumentAnnotation(...)` return.
- [ ] **Step 7: Rewrite the docs sections**

In `docs/writing-commands/arguments.md`, replace everything from `## Path constraints` up to (not
including) `## Module-level type aliases` with:

````markdown
## Path types

Pick a `toolr.types` path type to say what a path argument must be. The binary checks the path
while it parses the command line, before any Python starts. Your function always receives a
`pathlib.Path`.

| Type | Rejects the value unless | Value your function receives |
|---|---|---|
| `pathlib.Path` | (no check) | as typed |
| `toolr.types.AbsolutePath` | (no check) | joined to the working directory |
| `toolr.types.NewPath` | the path does not exist, and its parent directory does | absolute |
| `toolr.types.ResolvedPath` | the path exists | canonical |
| `toolr.types.FilePath` | the path is a regular file | canonical |
| `toolr.types.DirectoryPath` | the path is a directory | canonical |
| `toolr.types.ExecutablePath` | the path is a file the process can execute | canonical |
| `toolr.types.WritableDirectoryPath` | the path is a directory the process can write to | canonical |

"Canonical" means absolute, with symlinks and `..` resolved, so a symlink to a directory counts as
a `DirectoryPath`.

```python
from toolr import Context
from toolr.types import FilePath, NewPath


def convert(ctx: Context, config: FilePath, output: NewPath) -> None:
    ...
```

```sh
$ toolr fs convert /tmp/missing.toml out.json
error: invalid value '/tmp/missing.toml' for '<config>': path does not exist: /tmp/missing.toml
```

The types are `typing.NewType`s, so a type checker tells them apart: a `FilePath` can go where a
`Path` is expected, but a bare `Path` can't go where a `FilePath` is. A path derived from one, such
as `config.parent / "x"`, keeps its type for the type checker, but nothing checked it. Treat
derived paths as unchecked.

Three limits:

- The executable and writable checks are advisory. The file can change between the check and the
  moment your command uses it.
- The checks run in the toolr binary only. Calling the function directly, for example in a test
  built with `toolr.testing.make_context`, checks nothing.
- A default written as an expression, such as `config: FilePath = Path("pyproject.toml")`, isn't
  checked. A string default, `config: FilePath = "pyproject.toml"`, is.
````

In `docs/writing-commands/annotations.md`:

- Delete the three `must_*` rows from the kwarg table.
- Replace the paragraph starting "Not accepted: `path_must_exist=`" with:

```markdown
Removed: `must_exist=`, `must_be_file=` and `must_be_dir=` (and the `path_must_*` spellings). The
build fails with a hint naming the replacement: use `toolr.types.ResolvedPath`, `FilePath` or
`DirectoryPath`. See [Path types](arguments.md#path-types).
```

- [ ] **Step 8: Regenerate the skill refs and run the tests**

Run:

```bash
cargo xtask build-skill-refs
cargo xtask build-skill-refs --check
cargo test --workspace 2>&1 | tail -30
uv run pytest tests/utils/signature -q
```

Expected: all PASS. `skills/toolr-command-authoring/references/commands.md` loses the `must_*`
lines (it is generated from `_signature.py`). `references/arguments.md` now carries the "Path
types" section, and `references/types.md` loses "Path constraints". Poll the
`cargo test --workspace` output every 30–60 s; it can stall.

- [ ] **Step 9: Commit**

```bash
git-spice repo sync && git-spice upstack restack
git add crates/toolr-core crates/toolr/src crates/xtask crates/toolr-py/python/toolr/utils/_signature.py \
  tests/utils/signature/test_argument_annotation.py docs/writing-commands/arguments.md \
  docs/writing-commands/annotations.md skills/toolr-command-authoring/references
git diff --cached --name-only
git commit -m "feat(types)!: remove arg(must_*) in favour of path types (#502)"
```

Check that `git diff --cached --name-only` also lists the two `git rm` deletions and nothing
under `audit/`.

---

### Task 4: Manifest schema v2, enforced by freshness

**Files:**

- Modify: `crates/toolr-core/src/manifest/model.rs:9` (`SCHEMA_VERSION`)
- Modify: `crates/toolr-core/src/freshness/compare.rs` (`compare`)
- Test: `crates/toolr-core/src/freshness/tests.rs`
- Modify: manifest JSON fixtures in `crates/toolr/tests/*.rs` whose tests expect a fresh cache

**Interfaces:**

- Consumes: `crate::manifest::SCHEMA_VERSION`.
- Produces: no new names; `compare` returns at least `StaticDrift` on a schema mismatch.
- [ ] **Step 1: Write the failing freshness tests**

Append to `crates/toolr-core/src/freshness/tests.rs`:

```rust
#[test]
fn older_schema_version_forces_a_rebuild_even_when_hashes_and_version_match() {
    // A dev build keeps its version string across commits, so the
    // toolr_version check alone can't catch a schema bump.
    let tmp = TempDir::new().unwrap();
    make_tools(tmp.path(), &[("a.py", "x = 1\n")]);
    let mut cached = manifest_for(tmp.path());
    cached.schema_version = crate::manifest::SCHEMA_VERSION - 1;
    let verdict = compare(Some(&cached), &tmp.path().join("tools"), None).unwrap();
    assert!(matches!(verdict, FreshnessVerdict::StaticDrift));
}

#[test]
fn older_schema_version_with_venv_forces_third_party_drift() {
    let tmp = TempDir::new().unwrap();
    make_tools(tmp.path(), &[("a.py", "x = 1\n")]);
    make_venv(tmp.path(), &[("foo", "{}")]);
    let mut cached = manifest_for(tmp.path());
    cached.schema_version = crate::manifest::SCHEMA_VERSION - 1;
    let venv = tmp.path().join("venv");
    let verdict = compare(Some(&cached), &tmp.path().join("tools"), Some(&venv)).unwrap();
    assert!(matches!(verdict, FreshnessVerdict::ThirdPartyDrift));
}

#[test]
fn manifest_schema_version_is_2() {
    assert_eq!(crate::manifest::SCHEMA_VERSION, 2);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p toolr-core freshness`

Expected: FAIL. `manifest_schema_version_is_2` fails (left 1, right 2), and the two schema tests
get `Fresh`.

- [ ] **Step 3: Bump the version and check it in `compare`**

In `manifest/model.rs`, set `pub const SCHEMA_VERSION: u32 = 2;`.

In `freshness/compare.rs`, change the version check to:

```rust
    if cached.toolr_version != env!("CARGO_PKG_VERSION")
        || cached.schema_version != crate::manifest::SCHEMA_VERSION
    {
```

Extend the doc comment's `toolr_version` paragraph with: "A `schema_version` other than
`SCHEMA_VERSION` is treated the same way. A dev build keeps its version string across commits,
so this catches a schema change that the version check can't."

- [ ] **Step 4: Run the whole workspace to find fixtures that now go stale**

Run: `cargo test --workspace 2>&1 | tail -40` (poll every 30–60 s).

Expected: the new tests pass. Integration tests whose fixtures write `"schema_version": 1` next
to the running `toolr_version` and a `tools/pyproject.toml` now fail, because the cache is no
longer fresh. The expected failures are in `crates/toolr/tests/freshness_dispatch.rs` and
`crates/toolr/tests/cli_smoke.rs`.

- [ ] **Step 5: Move the stale fixtures to schema 2**

In every file that failed in Step 4, change `"schema_version": 1` to `"schema_version": 2`.
Leave these alone:

- `crates/toolr-core/src/manifest/tests.rs`, where `legacy_imports_key_is_tolerated`
  deliberately loads an old manifest;
- fixtures whose tests don't depend on freshness;
- `toolr_schema_version` fragment fixtures, which are a different key and stay at 1.

Re-run `cargo test --workspace 2>&1 | tail -20`. Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git-spice repo sync && git-spice upstack restack
git add crates/toolr-core/src/manifest/model.rs crates/toolr-core/src/freshness/compare.rs \
  crates/toolr-core/src/freshness/tests.rs <each fixture file changed in Step 5>
git diff --cached --name-only
git commit -m "feat(manifest): bump schema to 2 and rebuild caches on a schema mismatch (#502)"
```

---

### Task 5: Python `NewType`s

**Files:**

- Modify: `crates/toolr-py/python/toolr/types/__init__.py`
- Modify: `tests/test_types_module.py`
- Create: `tests/runner/test_path_type_coercion.py`
- Create: `tests/test_types_static.py`

**Interfaces:**

- Consumes: the Rust names from Task 1.
- Produces: `toolr.types.{AbsolutePath, NewPath, ResolvedPath, FilePath, DirectoryPath, ExecutablePath, WritableDirectoryPath}`.
- [ ] **Step 1: Write the failing tests**

In `tests/test_types_module.py`:

- Add `"DirectoryPath"`, `"ExecutablePath"`, `"FilePath"`, `"NewPath"` and
  `"WritableDirectoryPath"` to `EXPECTED_TOOLR_TYPES_NAMES`, keeping it sorted.
- Add `import typing` to the imports.
- Replace `test_path_aliases_resolve_to_pathlib_path` with:

```python
PATH_TYPE_PARENTS = {
    "AbsolutePath": pathlib.Path,
    "NewPath": toolr.types.AbsolutePath,
    "ResolvedPath": pathlib.Path,
    "FilePath": toolr.types.ResolvedPath,
    "DirectoryPath": toolr.types.ResolvedPath,
    "ExecutablePath": toolr.types.FilePath,
    "WritableDirectoryPath": toolr.types.DirectoryPath,
}


@pytest.mark.parametrize(("name", "parent"), PATH_TYPE_PARENTS.items(), ids=PATH_TYPE_PARENTS)
def test_path_types_are_newtypes_over_their_parent(name, parent) -> None:
    path_type = getattr(toolr.types, name)
    assert isinstance(path_type, typing.NewType)
    assert path_type.__supertype__ is parent


def test_path_types_hand_back_the_path_they_are_given() -> None:
    value = pathlib.Path("x")
    for name in PATH_TYPE_PARENTS:
        assert getattr(toolr.types, name)(value) is value
```

Add `import pytest` if it isn't already imported.

Create `tests/runner/test_path_type_coercion.py`:

```python
"""The runner hands every path type to the command as a `pathlib.Path`."""

from __future__ import annotations

import pathlib

from toolr import types as tt
from toolr._runner import _coerce_args


def _all_path_types(  # noqa: PLR0913 — one parameter per path type is the point.
    ctx: object,
    absolute: tt.AbsolutePath,
    new: tt.NewPath,
    resolved: tt.ResolvedPath,
    file: tt.FilePath,
    directory: tt.DirectoryPath,
    executable: tt.ExecutablePath,
    writable: tt.WritableDirectoryPath,
    files: list[tt.FilePath],
    maybe: tt.DirectoryPath | None,
    *rest: tt.ExecutablePath,
) -> None: ...


SCALARS = ("absolute", "new", "resolved", "file", "directory", "executable", "writable")


def test_every_path_type_reaches_the_command_as_a_pathlib_path() -> None:
    raw = {name: "/srv/x" for name in SCALARS}
    raw |= {"files": ["/srv/a", "/srv/b"], "maybe": "/srv/d", "rest": ["/srv/e"]}
    positional, keyword = _coerce_args(_all_path_types, raw)
    path_cls = type(pathlib.Path())
    for name in SCALARS:
        assert type(keyword[name]) is path_cls, name
    assert [type(p) for p in keyword["files"]] == [path_cls, path_cls]
    assert type(keyword["maybe"]) is path_cls
    assert [type(p) for p in positional] == [path_cls]
```

Create `tests/test_types_static.py`:

```python
"""mypy tells the path types apart from each other and from `pathlib.Path`.

It also pins that a derived path (`f / "x"`, `f.parent`) keeps its type,
which the docs warn about.
"""

from __future__ import annotations

import re
import textwrap
from pathlib import Path

import pytest
from mypy import api as mypy_api

SNIPPET = textwrap.dedent(
    """\
    from pathlib import Path

    from toolr.types import DirectoryPath, FilePath, WritableDirectoryPath


    def wants_path(p: Path) -> None: ...
    def wants_file(p: FilePath) -> None: ...
    def wants_dir(p: DirectoryPath) -> None: ...


    def accepted(f: FilePath, w: WritableDirectoryPath) -> None:
        wants_path(f)
        wants_dir(w)
        child: Path = w / "x"


    def rejected(p: Path) -> None:
        wants_file(p)  # E


    # Pinned, not endorsed: derived paths keep the type (typeshed's `Self`),
    # though nothing checked them. If this starts erroring, update the docs.
    def derived(f: FilePath) -> None:
        wants_file(f / "x")
        wants_file(f.parent)
    """
)


@pytest.fixture
def snippet(tmp_path: Path) -> Path:
    (tmp_path / "mypy.ini").write_text("[mypy]\n")
    path = tmp_path / "snippet.py"
    path.write_text(SNIPPET)
    return path


def test_mypy_distinguishes_path_types(snippet: Path) -> None:
    stdout, stderr, _ = mypy_api.run(
        [
            str(snippet),
            "--config-file",
            str(snippet.parent / "mypy.ini"),
            "--strict",
            "--follow-imports=silent",
            "--no-incremental",
            "--no-error-summary",
        ]
    )
    error_lines = sorted(int(m.group(1)) for m in re.finditer(r":(\d+): error:", stdout))
    expected = [n for n, line in enumerate(SNIPPET.splitlines(), start=1) if line.endswith("# E")]
    assert error_lines == expected, stdout + stderr
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `uv run pytest tests/test_types_module.py tests/runner/test_path_type_coercion.py tests/test_types_static.py -q`

Expected: FAIL. The names are missing from `__all__`. The coercion module fails at import with
`AttributeError: module 'toolr.types' has no attribute 'NewPath'`. `isinstance(…, NewType)`
fails for `AbsolutePath`.

- [ ] **Step 3: Define the `NewType`s**

In `crates/toolr-py/python/toolr/types/__init__.py`:

- Add `from typing import NewType` to the imports.
- Replace `AbsolutePath = _pathlib.Path` and `ResolvedPath = _pathlib.Path` with:

```python
# Each path type is a `NewType`: a distinct type to type checkers, the
# plain `pathlib.Path` it wraps at runtime. The toolr binary does every
# check before the command runs; nothing here validates anything.
AbsolutePath = NewType("AbsolutePath", _pathlib.Path)
NewPath = NewType("NewPath", AbsolutePath)
ResolvedPath = NewType("ResolvedPath", _pathlib.Path)
FilePath = NewType("FilePath", ResolvedPath)
DirectoryPath = NewType("DirectoryPath", ResolvedPath)
ExecutablePath = NewType("ExecutablePath", FilePath)
WritableDirectoryPath = NewType("WritableDirectoryPath", DirectoryPath)
```

- Add the five new names to `__all__`, keeping ruff's sort order. Run `uv run ruff check --fix`
  on the file if it complains.
- In the module docstring, replace the `AbsolutePath` and `ResolvedPath` bullets, and the "For
  bare :class:`pathlib.Path`" paragraph, with:

```text
- Path types. Each is a :class:`typing.NewType` over :class:`pathlib.Path`
  (or over another path type), so type checkers tell them apart while the
  runtime value is always a plain ``pathlib.Path``. The toolr binary
  checks the path before the command runs:

  - :data:`AbsolutePath` — joined to the working directory; no check.
  - :data:`NewPath` — absolute; must not exist, parent directory must.
  - :data:`ResolvedPath` — canonicalised; must exist.
  - :data:`FilePath` — canonicalised; must be a regular file.
  - :data:`DirectoryPath` — canonicalised; must be a directory.
  - :data:`ExecutablePath` — canonicalised; must be an executable file.
  - :data:`WritableDirectoryPath` — canonicalised; must be a writable
    directory.

  A bare :class:`pathlib.Path` gets no processing: the value is what the
  user typed.
```

- Change the docstring's opening claim "Each name in this module is a deliberate alias for a
  stdlib type" to "Each name in this module stands for a stdlib type".

- [ ] **Step 4: Run the tests to verify they pass**

Run: `uv run pytest tests/test_types_module.py tests/runner/test_path_type_coercion.py tests/test_types_static.py -q`

Expected: PASS. Then run `cargo xtask build-skill-refs --check`, which is expected to exit 0.
The xtask coverage test checks that every `__all__` name has a row, and Task 1 added the rows.

- [ ] **Step 5: Commit**

```bash
git-spice repo sync && git-spice upstack restack
git add crates/toolr-py/python/toolr/types/__init__.py tests/test_types_module.py \
  tests/runner/test_path_type_coercion.py tests/test_types_static.py
git diff --cached --name-only
git commit -m "feat(types): expose path types as NewTypes in toolr.types (#502)"
```

---

### Task 6: End-to-end rejection through the binary

**Files:**

- Create: `crates/toolr/tests/path_types_dispatch.rs`

**Interfaces:**

- Consumes: the serde names from the Global Constraints, and manifest schema 2 (Task 4).

- [ ] **Step 1: Write the test**

Create `crates/toolr/tests/path_types_dispatch.rs`:

```rust
//! The path types reject bad input at clap-parse time, before any Python
//! is spawned. The fixture manifest has no `tools/pyproject.toml`, so the
//! binary uses it as written without a freshness rebuild.

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

fn fixture(kind: &str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir(&tools).unwrap();
    let manifest = format!(
        r#"{{
    "schema_version": 2,
    "static_hash": "h",
    "third_party_hash": "",
    "groups": [{{"name": "probe", "title": "Probe", "description": "", "origin": "static"}}],
    "commands": [{{
        "name": "read", "group": "probe", "module": "tools.probe", "function": "read",
        "summary": "Path probe.", "description": "",
        "arguments": [{{
            "name": "target", "kind": "optional", "help": "a path.", "default": null,
            "type_annotation": "toolr.types.X", "resolved_type": {{"kind": "{kind}"}},
            "allowed_values": []
        }}],
        "origin": "static"
    }}]
}}"#
    );
    fs::write(tools.join(".toolr-manifest.json"), manifest).unwrap();
    tmp
}

fn assert_rejected(kind: &str, value: &str, message: &str) {
    let tmp = fixture(kind);
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["probe", "read", "--target", value])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{kind}: stderr:\n{stderr}");
    assert!(stderr.contains(message), "{kind}: stderr:\n{stderr}");
}

#[test]
fn each_path_type_rejects_its_bad_input() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("f.txt");
    fs::write(&file, "x").unwrap();
    let dir = tmp.path().to_str().unwrap();
    let file = file.to_str().unwrap();
    let missing = tmp.path().join("missing");
    let missing = missing.to_str().unwrap();
    let orphan = tmp.path().join("missing").join("out.txt");
    let orphan = orphan.to_str().unwrap();

    assert_rejected("resolved_path", missing, &format!("path does not exist: {missing}"));
    assert_rejected("file_path", dir, &format!("path is not a regular file: {dir}"));
    assert_rejected("directory_path", file, &format!("path is not a directory: {file}"));
    assert_rejected("new_path", file, &format!("path already exists: {file}"));
    assert_rejected("new_path", orphan, "parent directory does not exist:");
    assert_rejected("writable_directory_path", file, &format!("path is not a directory: {file}"));
    assert_rejected("executable_path", dir, &format!("path is not a regular file: {dir}"));
}

#[cfg(unix)]
#[test]
fn executable_path_rejects_a_non_executable_file() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let tool = tmp.path().join("tool");
    fs::write(&tool, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o644)).unwrap();
    let tool = tool.to_str().unwrap();
    assert_rejected("executable_path", tool, &format!("path is not executable: {tool}"));
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p toolr --test path_types_dispatch`

Expected: PASS. The code shipped in Tasks 1–4, so this test is a regression guard. If it fails,
debug the failure; don't loosen the assertions.

If the binary's first-run path needs a `pyproject.toml` or prints a cache hint on stderr before
the clap error, `contains` still matches. If the exit code isn't 2, compare against
`conflicts_with_dispatch.rs`, which pins the same clap-rejection path.

- [ ] **Step 3: Commit**

```bash
git-spice repo sync && git-spice upstack restack
git add crates/toolr/tests/path_types_dispatch.rs
git diff --cached --name-only
git commit -m "test(cli): pin path-type rejections end to end (#502)"
```

---

### Task 7: Docs, skill prose and release notes

**Files:**

- Modify: `docs/writing-commands/known-bugs.md`, `docs/internals/manifest.md:95-135`,
  `skills/toolr-command-authoring/SKILL.md:50-56,236-240`, `CONTRIBUTING.md:71-96`,
  `UNRELEASED.md`

- [ ] **Step 1: Update the docs**

In `docs/writing-commands/known-bugs.md`, change the "**Correction:**" sentence to:

```markdown
  **Correction:** the `path_must_*` rename was later reverted, and the
  `must_*` keywords were then removed in favour of the `toolr.types` path
  types (see [Path types](arguments.md#path-types)).
```

In `docs/internals/manifest.md`:

- Delete the `"path_constraints": null,` line from the JSON example.
- Change any `"schema_version": 1` in that page to `2`.
- Add `{ "kind": "file_path" }` to the `resolved_type` shape list.
- Delete the `**`path_constraints`**` bullet.
- Add a sentence after the `resolved_type` bullet: "Path types (`path`, `absolute_path`,
  `new_path`, `resolved_path`, `file_path`, `directory_path`, `executable_path`,
  `writable_directory_path`) carry their filesystem check in the kind itself."
- Where the page describes `schema_version`, add: "Version 2 removed `path_constraints`. A
  cache with any other schema version is rebuilt on the next run."

In `skills/toolr-command-authoring/SKILL.md`:

- Read lines 50–56 and 236–240.
- At line 54, remove `` `must_exist`, `` from the keyword list.
- At line 239, change "path constraints" to "path types".
- Add one bullet next to the other type guidance: "For a path argument, pick a `toolr.types`
  path type (`FilePath`, `DirectoryPath`, `NewPath`, `ExecutablePath`,
  `WritableDirectoryPath`, `ResolvedPath`, `AbsolutePath`); `arg()` no longer takes `must_*`."

In `CONTRIBUTING.md`, under "Adding a supported type", add to the compiler-checked list:

```markdown
6. `SupportedType::is_path()`: say whether clap stores the value as a `PathBuf`. For a path type,
   also add its `(PathForm, PathCheck)` to `path_rule` in `crates/toolr/src/value_parsers.rs`.
   A test fails if the two disagree.
```

- [ ] **Step 2: Queue the release notes**

Append to `UNRELEASED.md`:

````markdown
### Path types replace `arg(must_*)`

`toolr.types` gains path types that say what a path argument must be. The toolr binary checks each
one while it parses the command line, and your command always receives a `pathlib.Path`:

- `NewPath`: must not exist; its parent directory must.
- `FilePath`, `DirectoryPath`: must exist as that kind; canonicalised.
- `ExecutablePath`, `WritableDirectoryPath`: as above, plus executable or writable.

All seven path types, including the existing `AbsolutePath` and `ResolvedPath`, are now
`typing.NewType`s rather than plain aliases, so type checkers tell them apart. **Typing-level
break:** passing a bare `Path` where one of them is expected is now a type error.

**Breaking:** `arg(must_exist=…)`, `arg(must_be_file=…)` and `arg(must_be_dir=…)` are removed. The
manifest build fails and names the replacement:

| Before | After |
|---|---|
| `Annotated[Path, arg(must_exist=True)]` | `ResolvedPath` |
| `Annotated[Path, arg(must_be_file=True)]` | `FilePath` |
| `Annotated[Path, arg(must_be_dir=True)]` | `DirectoryPath` |

The local manifest schema is now version 2. An existing cache is rebuilt on the next run; there is
nothing to do. Plugin commands don't run the path checks yet (#520).
````

- [ ] **Step 3: Verify the docs build and the whole suite**

Run:

```bash
prek run --all-files
uv run mkdocs build --strict
mise run test
```

Expected: all pass. `mise run test` runs the skill-refs drift gate, `cargo test --workspace` and
pytest; poll it every 30–60 s. Run `git grep -in "must_exist\|must_be_file\|must_be_dir" -- docs skills`.
Expected hits: only the removal notes in `annotations.md`, `known-bugs.md` and `UNRELEASED.md`.

- [ ] **Step 4: Commit**

```bash
git-spice repo sync && git-spice upstack restack
git add docs/writing-commands/known-bugs.md docs/internals/manifest.md \
  skills/toolr-command-authoring/SKILL.md CONTRIBUTING.md UNRELEASED.md
git diff --cached --name-only
git commit -m "docs(types): document path types and the arg(must_*) removal (#502)"
```

---

### Task 8: Archive the spec

**Files:**

- Move: `specs/2026-09-29-path-state-types-design.md` and
  `specs/2026-09-29-path-state-types-plan.md` to `specs/archive/2026/`

- [ ] **Step 1: Move and commit.** This is the final commit before opening the PR.

```bash
git-spice repo sync && git-spice upstack restack
git mv specs/2026-09-29-path-state-types-design.md specs/archive/2026/
git mv specs/2026-09-29-path-state-types-plan.md specs/archive/2026/
git diff --cached --name-only
git commit -m "docs(specs): archive the path-state types design and plan (#502)"
```
