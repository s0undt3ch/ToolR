<!-- rumdl-disable MD046 MD076 -->

# Plan 7: Missing-Dependency Diagnostics

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Lint:** Plan docs nest fenced code inside list items for step-by-step
> structure. The `<!-- rumdl-disable MD046 MD076 -->` directive above turns
> off the code-block-style and list-item-spacing rules for this file only.

**Goal:** Make missing-dependency failures friendly without lying. Toolr never
guesses which package provides which import (no module-to-package lookup
table). Instead it surfaces two clear signals:

- **Pre-flight check.** Before spawning Python for a user command, walk the
  command's recorded top-level imports and probe the tools venv's
  `site-packages`. Any missing module → fail fast with a concrete,
  copy-pasteable next step.
- **Post-mortem interception.** When the Python subprocess does exit with an
  `ImportError` (inline imports, conditional imports, lazy submodules), parse
  the traceback off stderr, preserve it verbatim in the user-visible output,
  and append the same `toolr project deps sync` suggestion.

User contract: "common missing-deps surface a clear pre-flight error;
everything else surfaces a clear post-mortem error." Toolr never claims to
know which PyPI package provides a given import name.

**Architecture:** A new `_rust_utils::deps_check` module hosts both halves —
the cheap filesystem probe and the post-mortem stderr parser. The pre-flight
hook fires inside `dispatch.rs` immediately before Plan 2's subprocess spawn,
keyed on the resolved tools venv from Plan 3. The post-mortem hook wraps
Plan 2's runner spawn: captures stderr, looks for the well-known Python
`ImportError` shape on subprocess exit, prints the original traceback
followed by the toolr suggestion. The original traceback is **never**
rewritten or omitted — toolr only appends.

**Tech Stack:** Rust 2021, anyhow, thiserror. Existing crates only — no new
runtime dependencies. Tests use `tempfile` + `assert_cmd` (already in
`[dev-dependencies]` from Plan 1).

**Reading order in this plan:** Tasks build on each other. Don't skip ahead;
later tasks reference types defined in earlier ones.

**Dependencies:**

- **Plan 2** must be landed: this plan hooks into the subprocess-spawn site
  inside `dispatch.rs` and captures the runner's stderr.
- **Plan 3** must be landed: pre-flight needs the resolved tools venv path
  (and specifically its `site-packages` directory).

If either is not yet merged, this plan's wiring tasks (Task 4, Task 7) will
not compile. The pure module tasks (Task 1, Task 2, Task 3, Task 6) are
landable independently and can be staged ahead of Plans 2 and 3 if needed.

---

## Task 1: Filesystem module probe

Implement the lowest-level primitive: given a venv path and a top-level
import name, report whether the module exists under `site-packages` as
either a package directory (`<module>/__init__.py`) or a single-file module
(`<module>.py`).

**Files:**

- Create: `src/deps_check/mod.rs`
- Create: `src/deps_check/probe.rs`
- Modify: `src/lib.rs`
- [x] **Step 1.1: Expose the new module from `src/lib.rs`**

    Add to `src/lib.rs`:

    ```rust
    pub mod deps_check;
    ```

- [x] **Step 1.2: Create `src/deps_check/mod.rs`**

    ```rust
    //! Missing-dependency diagnostics.
    //!
    //! Two halves:
    //!
    //! - [`probe`] — filesystem-only check that a top-level import exists in
    //!   a venv's `site-packages`. Used by pre-flight (Task 2).
    //! - [`post_mortem`] (Task 6) — parse Python `ImportError` tracebacks
    //!   off subprocess stderr and append the standard suggestion.

    pub mod probe;

    pub use probe::{ProbeOutcome, probe_module, site_packages_dir};

    #[cfg(test)]
    mod tests;
    ```

- [x] **Step 1.3: Create `src/deps_check/probe.rs`**

    ```rust
    //! Filesystem-only module probe.

    use std::path::{Path, PathBuf};

    /// Result of probing a single top-level module name against a venv.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ProbeOutcome {
        /// `<site-packages>/<module>/__init__.py` exists.
        Package(PathBuf),
        /// `<site-packages>/<module>.py` exists.
        SingleFile(PathBuf),
        /// Neither was found.
        Missing,
    }

    /// Locate the `site-packages` directory under a venv. Returns the first
    /// match for `<venv>/lib/python*/site-packages/`. On Windows this is
    /// `<venv>/Lib/site-packages/` (no `python*` segment).
    pub fn site_packages_dir(venv: &Path) -> Option<PathBuf> {
        // Windows layout first — short-circuit if it matches.
        let win = venv.join("Lib").join("site-packages");
        if win.is_dir() {
            return Some(win);
        }
        // Unix layout: <venv>/lib/python<X.Y>/site-packages
        let lib = venv.join("lib");
        let entries = std::fs::read_dir(&lib).ok()?;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("python") {
                continue;
            }
            let candidate = entry.path().join("site-packages");
            if candidate.is_dir() {
                return Some(candidate);
            }
        }
        None
    }

    /// Probe a single top-level import name against a `site-packages` dir.
    ///
    /// **Scope.** Only checks for `<module>/__init__.py` or `<module>.py`.
    /// This is the same shape Python's `importlib` finds first. It misses:
    ///
    /// - Namespace packages (`PEP 420`): no `__init__.py`, just a bare
    ///   directory. These will pass at runtime but the probe returns
    ///   `Missing`. Falls through to post-mortem.
    /// - C-extension modules shipped as `.so` / `.pyd` without a `.py`
    ///   sibling. Rare for the modules toolr commands import directly at
    ///   the top level (these are usually re-exported from a Python
    ///   shim package).
    ///
    /// Both gaps are accepted: pre-flight is a fast-path, not a guarantee.
    /// Post-mortem catches whatever pre-flight misses.
    pub fn probe_module(site_packages: &Path, module: &str) -> ProbeOutcome {
        // Defensive: a dotted import name like `a.b.c` always has its
        // top-level segment as `a`. Static parser already records only
        // top-level names, but be safe here too.
        let top = module.split('.').next().unwrap_or(module);
        if top.is_empty() {
            return ProbeOutcome::Missing;
        }

        let pkg = site_packages.join(top).join("__init__.py");
        if pkg.is_file() {
            return ProbeOutcome::Package(pkg);
        }
        let single = site_packages.join(format!("{top}.py"));
        if single.is_file() {
            return ProbeOutcome::SingleFile(single);
        }
        ProbeOutcome::Missing
    }
    ```

- [x] **Step 1.4: Create `src/deps_check/tests.rs` with probe tests**

    ```rust
    use std::fs;

    use tempfile::TempDir;

    use super::probe::{ProbeOutcome, probe_module, site_packages_dir};

    /// Build a fake unix-shaped venv with the requested module shapes.
    /// Each entry is either ("foo", "package") or ("bar", "single").
    fn fake_venv(shapes: &[(&str, &str)]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let sp = tmp
            .path()
            .join("lib")
            .join("python3.13")
            .join("site-packages");
        fs::create_dir_all(&sp).unwrap();
        for (name, kind) in shapes {
            match *kind {
                "package" => {
                    let pkg = sp.join(name);
                    fs::create_dir(&pkg).unwrap();
                    fs::write(pkg.join("__init__.py"), "").unwrap();
                }
                "single" => {
                    fs::write(sp.join(format!("{name}.py")), "").unwrap();
                }
                other => panic!("unknown shape {other}"),
            }
        }
        tmp
    }

    #[test]
    fn site_packages_dir_finds_python_subdir() {
        let venv = fake_venv(&[]);
        let sp = site_packages_dir(venv.path()).expect("should find site-packages");
        assert!(sp.ends_with("site-packages"));
    }

    #[test]
    fn site_packages_dir_returns_none_when_absent() {
        let tmp = TempDir::new().unwrap();
        assert!(site_packages_dir(tmp.path()).is_none());
    }

    #[test]
    fn probe_module_finds_a_package() {
        let venv = fake_venv(&[("packaging", "package")]);
        let sp = site_packages_dir(venv.path()).unwrap();
        let outcome = probe_module(&sp, "packaging");
        assert!(matches!(outcome, ProbeOutcome::Package(_)));
    }

    #[test]
    fn probe_module_finds_a_single_file_module() {
        let venv = fake_venv(&[("six", "single")]);
        let sp = site_packages_dir(venv.path()).unwrap();
        let outcome = probe_module(&sp, "six");
        assert!(matches!(outcome, ProbeOutcome::SingleFile(_)));
    }

    #[test]
    fn probe_module_returns_missing_when_absent() {
        let venv = fake_venv(&[]);
        let sp = site_packages_dir(venv.path()).unwrap();
        assert_eq!(probe_module(&sp, "nope"), ProbeOutcome::Missing);
    }

    #[test]
    fn probe_module_only_checks_top_level_segment() {
        // We pass a dotted name; only `pkg/__init__.py` matters.
        let venv = fake_venv(&[("pkg", "package")]);
        let sp = site_packages_dir(venv.path()).unwrap();
        assert!(matches!(probe_module(&sp, "pkg.sub"), ProbeOutcome::Package(_)));
    }

    #[test]
    fn probe_module_treats_empty_name_as_missing() {
        let venv = fake_venv(&[]);
        let sp = site_packages_dir(venv.path()).unwrap();
        assert_eq!(probe_module(&sp, ""), ProbeOutcome::Missing);
    }
    ```

- [x] **Step 1.5: Run the tests**

    ```bash
    cargo test --lib deps_check::tests::
    ```

    Expected: 7 tests passing.

- [x] **Step 1.6: Commit**

    ```bash
    git add src/lib.rs src/deps_check/
    git commit -m "feat(deps-check): Add filesystem probe for top-level imports"
    ```

---

## Task 2: Pre-flight check with structured error

Wrap the per-module probe in a higher-level `check_imports` that takes the
whole list a command recorded and returns a structured error if any are
missing. The error carries the missing module names so callers can render a
consistent message.

**Files:**

- Create: `src/deps_check/preflight.rs`
- Modify: `src/deps_check/mod.rs`
- Modify: `src/deps_check/tests.rs`
- [x] **Step 2.1: Append the failing tests to `src/deps_check/tests.rs`**

    ```rust
    use super::preflight::{MissingDeps, check_imports};

    #[test]
    fn check_imports_passes_when_all_present() {
        let venv = fake_venv(&[("packaging", "package"), ("six", "single")]);
        let sp = site_packages_dir(venv.path()).unwrap();
        let imports = vec!["packaging".to_string(), "six".to_string()];
        assert!(check_imports(&sp, &imports).is_ok());
    }

    #[test]
    fn check_imports_reports_missing_module() {
        let venv = fake_venv(&[("packaging", "package")]);
        let sp = site_packages_dir(venv.path()).unwrap();
        let imports = vec!["packaging".to_string(), "yaml".to_string()];
        let err = check_imports(&sp, &imports).expect_err("should be missing");
        assert_eq!(err.missing, vec!["yaml".to_string()]);
    }

    #[test]
    fn check_imports_reports_all_missing_in_input_order() {
        let venv = fake_venv(&[]);
        let sp = site_packages_dir(venv.path()).unwrap();
        let imports = vec![
            "yaml".to_string(),
            "cv2".to_string(),
            "sklearn".to_string(),
        ];
        let err = check_imports(&sp, &imports).expect_err("should be missing");
        assert_eq!(err.missing, imports);
    }

    #[test]
    fn check_imports_skips_stdlib_like_names() {
        // The static parser already excludes stdlib modules from
        // `command.imports`, but defensively check_imports must not blow
        // up when given an empty list.
        let venv = fake_venv(&[]);
        let sp = site_packages_dir(venv.path()).unwrap();
        assert!(check_imports(&sp, &[]).is_ok());
    }

    #[test]
    fn missing_deps_message_quotes_module_and_suggests_sync() {
        let err = MissingDeps {
            missing: vec!["yaml".to_string()],
        };
        let rendered = err.to_string();
        assert!(rendered.contains("`yaml`"));
        assert!(rendered.contains("toolr project deps sync"));
        assert!(rendered.contains("tools/pyproject.toml"));
    }

    #[test]
    fn missing_deps_message_pluralizes_when_multiple() {
        let err = MissingDeps {
            missing: vec!["yaml".to_string(), "cv2".to_string()],
        };
        let rendered = err.to_string();
        // Both modules should appear, in order.
        let yaml_idx = rendered.find("yaml").unwrap();
        let cv2_idx = rendered.find("cv2").unwrap();
        assert!(yaml_idx < cv2_idx);
    }
    ```

- [x] **Step 2.2: Run the tests, expect compile failure**

    ```bash
    cargo test --lib deps_check::tests::check_imports_passes_when_all_present
    ```

    Expected: unresolved import `super::preflight`. Good — we're TDD-ing
    the module shape.

- [x] **Step 2.3: Create `src/deps_check/preflight.rs`**

    ```rust
    //! Pre-flight: check that all of a command's top-level imports exist
    //! in the tools venv's `site-packages`.

    use std::fmt;
    use std::path::Path;

    use super::probe::{ProbeOutcome, probe_module};

    /// One or more imports were not found.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct MissingDeps {
        /// Missing module names, preserved in the order they were probed.
        pub missing: Vec<String>,
    }

    impl std::error::Error for MissingDeps {}

    impl fmt::Display for MissingDeps {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self.missing.as_slice() {
                [] => write!(f, "no missing imports"),
                [one] => write!(
                    f,
                    "import `{one}` not found in tools venv. \
                     A dependency may be missing — run \
                     `toolr project deps sync` and check tools/pyproject.toml."
                ),
                many => {
                    let joined = many
                        .iter()
                        .map(|m| format!("`{m}`"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    write!(
                        f,
                        "imports {joined} not found in tools venv. \
                         Dependencies may be missing — run \
                         `toolr project deps sync` and check tools/pyproject.toml."
                    )
                }
            }
        }
    }

    /// Probe each import in order; collect those that come back `Missing`.
    /// Returns `Ok(())` if every import resolves to a package or
    /// single-file module under `site-packages`.
    pub fn check_imports(site_packages: &Path, imports: &[String]) -> Result<(), MissingDeps> {
        let mut missing = Vec::new();
        for name in imports {
            if matches!(probe_module(site_packages, name), ProbeOutcome::Missing) {
                missing.push(name.clone());
            }
        }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(MissingDeps { missing })
        }
    }
    ```

- [x] **Step 2.4: Re-export from `src/deps_check/mod.rs`**

    Append:

    ```rust
    pub mod preflight;

    pub use preflight::{MissingDeps, check_imports};
    ```

- [x] **Step 2.5: Run the tests, expect PASS**

    ```bash
    cargo test --lib deps_check::
    ```

    Expected: all 13 tests passing (7 probe + 6 preflight).

- [x] **Step 2.6: Commit**

    ```bash
    git add src/deps_check/
    git commit -m "feat(deps-check): Add pre-flight check_imports with structured error"
    ```

---

## Task 3: Public API and re-exports

Promote `deps_check`'s small surface to the crate root for ergonomic use
from the `toolr` binary's `dispatch.rs`. Keeps `dispatch.rs` from having to
spell out `_rust_utils::deps_check::preflight::check_imports`.

**Files:**

- Modify: `src/deps_check/mod.rs`
- [x] **Step 3.1: Confirm the re-exports**

    `src/deps_check/mod.rs` should now read:

    ```rust
    //! Missing-dependency diagnostics.

    pub mod post_mortem; // landed in Task 6, declared early so it's discoverable
    pub mod preflight;
    pub mod probe;

    pub use post_mortem::{ImportErrorReport, intercept_import_error};
    pub use preflight::{MissingDeps, check_imports};
    pub use probe::{ProbeOutcome, probe_module, site_packages_dir};

    #[cfg(test)]
    mod tests;
    ```

    The `post_mortem` items don't exist yet — that's fine, this task ends
    with a deliberately broken build to anchor Task 6. Remove the
    `post_mortem` `pub mod` and `pub use` lines for now and re-add them in
    Task 6.

- [x] **Step 3.2: Final shape for this task**

    ```rust
    //! Missing-dependency diagnostics.

    pub mod preflight;
    pub mod probe;

    pub use preflight::{MissingDeps, check_imports};
    pub use probe::{ProbeOutcome, probe_module, site_packages_dir};

    #[cfg(test)]
    mod tests;
    ```

- [x] **Step 3.3: Build and test**

    ```bash
    cargo build
    cargo test --lib deps_check::
    ```

    Expected: clean build, 13 tests still passing.

- [x] **Step 3.4: Commit**

    ```bash
    git add src/deps_check/mod.rs
    git commit -m "refactor(deps-check): Re-export pre-flight surface at module root"
    ```

---

## Task 4: Wire pre-flight into dispatch

Hook `check_imports` into `dispatch.rs` so it runs immediately before the
Plan 2 subprocess spawn. On `MissingDeps`, print the error to stderr and
exit with a dedicated non-zero code (78 — "configuration error", per
`sysexits.h` convention; distinct from the 64 "usage error" Plan 1 uses).

This task touches code added by Plans 2 and 3. If those are not yet
landed, skip to Task 5 and come back here when they are.

**Files:**

- Modify: `src/bin/toolr/dispatch.rs`
- Modify: `tests/cli_smoke.rs`
- [x] **Step 4.1: Locate the spawn site introduced by Plan 2**

    Plan 2 added something resembling:

    ```rust
    let venv = _rust_utils::venv::resolve(&project_root)?;
    let status = _rust_utils::runner::spawn_runner(&venv, &spec)?;
    return Ok(ExitCode::from(status.code().unwrap_or(1) as u8));
    ```

    The pre-flight inserts itself between `resolve` and `spawn_runner`.

- [x] **Step 4.2: Add the pre-flight call**

    In `src/bin/toolr/dispatch.rs`, immediately after the venv is resolved
    and before the runner is spawned:

    ```rust
    // Pre-flight missing-dependency check (Plan 7).
    if let Some(sp) = _rust_utils::deps_check::site_packages_dir(&venv.path) {
        if let Err(err) = _rust_utils::deps_check::check_imports(&sp, &cmd.imports) {
            eprintln!("toolr: {err}");
            return Ok(ExitCode::from(78));
        }
    }
    // If site-packages can't be located the runner spawn will fail
    // loudly anyway — don't pretend pre-flight ran successfully.
    ```

    Note: `venv.path` and the `_rust_utils::venv` module shape come from
    Plan 3. Adapt the field/path name if Plan 3 named things differently;
    the contract is "resolved venv root → `site_packages_dir(&root)`".

- [x] **Step 4.3: Add the integration test fixture helper**

    Append to `tests/cli_smoke.rs` (created by Plan 1):

    ```rust
    /// Build a fixture project at `tmp/` containing:
    ///
    /// - `tools/<module>.py` with the given source
    /// - `tools/.toolr-manifest.json` with one command whose `imports`
    ///   list is whatever was passed
    /// - a fake venv at `<venv_path>` containing only the modules listed
    ///   in `present`
    ///
    /// The fixture's manifest is hand-rolled rather than rebuilt from
    /// source so the test stays focused on the pre-flight branch.
    fn fixture_with_imports(
        module_src: &str,
        imports: &[&str],
        present_in_venv: &[&str],
    ) -> tempfile::TempDir {
        use std::fs;
        let tmp = tempfile::TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        fs::create_dir_all(&tools).unwrap();
        fs::write(tools.join("ci.py"), module_src).unwrap();
        let imports_json: String = imports
            .iter()
            .map(|i| format!("\"{i}\""))
            .collect::<Vec<_>>()
            .join(",");
        let manifest = format!(
            r#"{{
                "schema_version": 1,
                "static_hash": "h",
                "groups": [{{
                    "name": "ci", "title": "CI", "description": "",
                    "origin": "static"
                }}],
                "commands": [{{
                    "name": "hello", "group": "ci", "module": "tools.ci",
                    "function": "hello", "summary": "", "description": "",
                    "arguments": [], "imports": [{imports_json}],
                    "origin": "static"
                }}]
            }}"#
        );
        fs::write(tools.join(".toolr-manifest.json"), manifest).unwrap();

        // Fake venv at the location Plan 3 resolves to. The unit tests in
        // Task 1 already exercise `site_packages_dir`; here we just need a
        // structure it accepts. The default cache location is governed by
        // Plan 3, so for the integration test we point toolr at the
        // fixture via the same env var Plan 3 honors. If Plan 3's env var
        // is named differently, update `TOOLR_VENV_OVERRIDE` here.
        let venv = tmp.path().join("fake-venv");
        let sp = venv.join("lib").join("python3.13").join("site-packages");
        fs::create_dir_all(&sp).unwrap();
        for name in present_in_venv {
            let pkg = sp.join(name);
            fs::create_dir(&pkg).unwrap();
            fs::write(pkg.join("__init__.py"), "").unwrap();
        }
        // Also drop a `toolr/` package so Plan 3's "toolr installed in
        // venv" guard doesn't trip.
        let toolr_pkg = sp.join("toolr");
        fs::create_dir_all(&toolr_pkg).unwrap();
        fs::write(toolr_pkg.join("__init__.py"), "").unwrap();
        tmp
    }
    ```

- [x] **Step 4.4: Add the integration test**

    Append:

    ```rust
    use assert_cmd::Command;

    #[test]
    fn preflight_fails_when_an_import_is_missing_from_venv() {
        let tmp = fixture_with_imports(
            "def hello(ctx): pass\n",
            &["yaml"],
            // venv is empty — `yaml` is missing.
            &[],
        );
        let output = Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .env("TOOLR_VENV_OVERRIDE", tmp.path().join("fake-venv"))
            .args(["ci", "hello"])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(78));
        assert!(stderr.contains("import `yaml` not found"));
        assert!(stderr.contains("toolr project deps sync"));
    }

    #[test]
    fn preflight_passes_when_all_imports_present() {
        let tmp = fixture_with_imports(
            "def hello(ctx): pass\n",
            &["packaging"],
            &["packaging"],
        );
        let output = Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .env("TOOLR_VENV_OVERRIDE", tmp.path().join("fake-venv"))
            .args(["ci", "hello"])
            .output()
            .unwrap();
        // Pre-flight passed → runner spawns → the test fixture has no real
        // Python, so the runner errors out. The pre-flight diagnostic
        // must NOT appear in stderr.
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("not found in tools venv"));
    }
    ```

    `TOOLR_VENV_OVERRIDE` is the env var Plan 3 honors for forcing a venv
    location. If Plan 3 uses a different name, update both the dispatch
    call and the test in lockstep.

- [x] **Step 4.5: Run the tests**

    ```bash
    cargo test --test cli_smoke preflight_
    ```

    Expected: 2 tests passing.

- [x] **Step 4.6: Commit**

    ```bash
    git add src/bin/toolr/dispatch.rs tests/cli_smoke.rs
    git commit -m "feat(cli): Run pre-flight missing-deps check before runner spawn"
    ```

---

## Task 5: Suppression escape hatch

Some users will hit pre-flight false positives (namespace packages,
`__getattr__` plugin systems, optional imports that the static parser
doesn't realize are optional). Provide a single environment variable to
skip the pre-flight entirely. The post-mortem check still runs.

**Files:**

- Modify: `src/bin/toolr/dispatch.rs`
- Modify: `tests/cli_smoke.rs`
- [x] **Step 5.1: Honor `TOOLR_NO_PREFLIGHT_DEPS` in dispatch**

    Replace the Task 4 pre-flight block with:

    ```rust
    let skip_preflight = std::env::var_os("TOOLR_NO_PREFLIGHT_DEPS")
        .is_some_and(|v| !v.is_empty() && v != "0");
    if !skip_preflight {
        if let Some(sp) = _rust_utils::deps_check::site_packages_dir(&venv.path) {
            if let Err(err) = _rust_utils::deps_check::check_imports(&sp, &cmd.imports) {
                eprintln!("toolr: {err}");
                return Ok(ExitCode::from(78));
            }
        }
    }
    ```

- [x] **Step 5.2: Add the integration test**

    Append to `tests/cli_smoke.rs`:

    ```rust
    #[test]
    fn preflight_can_be_disabled_with_env_var() {
        let tmp = fixture_with_imports(
            "def hello(ctx): pass\n",
            &["yaml"],
            // venv is empty — pre-flight would normally fail.
            &[],
        );
        let output = Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .env("TOOLR_VENV_OVERRIDE", tmp.path().join("fake-venv"))
            .env("TOOLR_NO_PREFLIGHT_DEPS", "1")
            .args(["ci", "hello"])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Pre-flight skipped → runner spawn proceeds. The fake venv has no
        // python, so the spawn fails, but NOT with the pre-flight string.
        assert!(!stderr.contains("not found in tools venv"));
    }
    ```

- [x] **Step 5.3: Run the test**

    ```bash
    cargo test --test cli_smoke preflight_can_be_disabled
    ```

    Expected: 1 test passing.

- [x] **Step 5.4: Commit**

    ```bash
    git add src/bin/toolr/dispatch.rs tests/cli_smoke.rs
    git commit -m "feat(cli): Add TOOLR_NO_PREFLIGHT_DEPS escape hatch"
    ```

---

## Task 6: Post-mortem ImportError parser

When pre-flight passes but the Python runner still hits an `ImportError`
(inline imports, `__getattr__`, conditional imports), toolr should preserve
the original Python traceback verbatim and append the same actionable
suggestion. This task builds the stderr parser; Task 7 wires it into the
subprocess flow.

**Files:**

- Create: `src/deps_check/post_mortem.rs`
- Modify: `src/deps_check/mod.rs`
- Modify: `src/deps_check/tests.rs`
- [ ] **Step 6.1: Append the failing tests**

    In `src/deps_check/tests.rs`:

    ```rust
    use super::post_mortem::{ImportErrorReport, intercept_import_error};

    const PY_IMPORT_ERROR: &str = "\
Traceback (most recent call last):
  File \"/x/tools/ci.py\", line 1, in <module>
    import yaml
ModuleNotFoundError: No module named 'yaml'
";

    const PY_NESTED_IMPORT_ERROR: &str = "\
Traceback (most recent call last):
  File \"/x/tools/ci.py\", line 5, in hello
    from pkg.sub import thing
ImportError: cannot import name 'thing' from 'pkg.sub'
";

    const PY_GENERIC_RUNTIME_ERROR: &str = "\
Traceback (most recent call last):
  File \"/x/tools/ci.py\", line 7, in hello
    raise ValueError(\"nope\")
ValueError: nope
";

    #[test]
    fn intercepts_module_not_found_error() {
        let report = intercept_import_error(PY_IMPORT_ERROR)
            .expect("should classify");
        assert_eq!(report.error_class, "ModuleNotFoundError");
        assert_eq!(report.missing_hint.as_deref(), Some("yaml"));
        assert!(report.traceback.contains("ModuleNotFoundError"));
    }

    #[test]
    fn intercepts_plain_import_error() {
        let report = intercept_import_error(PY_NESTED_IMPORT_ERROR)
            .expect("should classify");
        assert_eq!(report.error_class, "ImportError");
        // `missing_hint` may be None — the message form is "cannot import
        // name X from Y". We don't try to extract a top-level module here
        // because that's not actually the missing thing.
        assert!(report.traceback.contains("ImportError"));
    }

    #[test]
    fn returns_none_for_non_import_error() {
        assert!(intercept_import_error(PY_GENERIC_RUNTIME_ERROR).is_none());
    }

    #[test]
    fn returns_none_for_empty_input() {
        assert!(intercept_import_error("").is_none());
    }

    #[test]
    fn rendered_report_includes_traceback_and_suggestion() {
        let report = intercept_import_error(PY_IMPORT_ERROR).unwrap();
        let rendered = report.render();
        // The original traceback is preserved verbatim.
        assert!(rendered.contains(PY_IMPORT_ERROR.trim_end()));
        // The toolr suggestion is appended at the end.
        assert!(rendered.contains("toolr project deps sync"));
        // The suggestion specifically calls out the module name when we
        // were able to extract one.
        assert!(rendered.contains("yaml"));
    }

    #[test]
    fn rendered_report_for_import_error_without_hint_still_suggests_sync() {
        let report = intercept_import_error(PY_NESTED_IMPORT_ERROR).unwrap();
        let rendered = report.render();
        assert!(rendered.contains(PY_NESTED_IMPORT_ERROR.trim_end()));
        assert!(rendered.contains("toolr project deps sync"));
    }
    ```

- [ ] **Step 6.2: Run the tests, expect compile failure**

    ```bash
    cargo test --lib deps_check::tests::intercepts_module_not_found_error
    ```

    Expected: unresolved import `super::post_mortem`. Good.

- [ ] **Step 6.3: Create `src/deps_check/post_mortem.rs`**

    ```rust
    //! Parse Python tracebacks on subprocess stderr looking for
    //! `ImportError` / `ModuleNotFoundError`, and produce a rendered
    //! report with the original traceback plus the toolr suggestion.

    /// One intercepted `ImportError` from a Python subprocess.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ImportErrorReport {
        /// The original, unmodified subprocess stderr captured by the
        /// runner. Always rendered verbatim — toolr only appends.
        pub traceback: String,
        /// `"ImportError"` or `"ModuleNotFoundError"`.
        pub error_class: String,
        /// For `ModuleNotFoundError: No module named 'X'`, the captured
        /// `X`. `None` for the bare `ImportError` form, where the
        /// missing-thing is a name inside an existing module, not a
        /// top-level package.
        pub missing_hint: Option<String>,
    }

    impl ImportErrorReport {
        /// Render the report exactly as toolr should print it to the
        /// user. The original traceback comes first; the toolr suggestion
        /// is appended on its own line(s) after a blank separator.
        pub fn render(&self) -> String {
            let mut out = String::new();
            out.push_str(self.traceback.trim_end());
            out.push_str("\n\n");
            match self.missing_hint.as_deref() {
                Some(module) => {
                    out.push_str(&format!(
                        "toolr: import `{module}` failed at runtime. \
                         A dependency may be missing — run \
                         `toolr project deps sync` and check \
                         tools/pyproject.toml.\n"
                    ));
                }
                None => {
                    out.push_str(
                        "toolr: import failed at runtime. \
                         A dependency may be missing — run \
                         `toolr project deps sync` and check \
                         tools/pyproject.toml.\n",
                    );
                }
            }
            out
        }
    }

    /// Inspect captured stderr from a Python subprocess. If it ends in an
    /// `ImportError` / `ModuleNotFoundError`, return a structured report.
    /// Otherwise return `None` and let normal error handling take over.
    ///
    /// **Heuristic.** Python's `traceback.print_exc()` puts the exception
    /// class and message on the **last non-empty line**. We scan from
    /// the end backwards for the first non-empty line and pattern-match
    /// against `ModuleNotFoundError: No module named '...'` and
    /// `ImportError: ...`. This is the same trick `pytest` uses; it's
    /// resilient to extra warning output written *before* the traceback
    /// (deprecation warnings, etc.).
    pub fn intercept_import_error(stderr: &str) -> Option<ImportErrorReport> {
        let last = stderr.lines().rev().find(|line| !line.trim().is_empty())?;
        if let Some(rest) = last.strip_prefix("ModuleNotFoundError: ") {
            let hint = extract_quoted_module(rest);
            return Some(ImportErrorReport {
                traceback: stderr.to_string(),
                error_class: "ModuleNotFoundError".to_string(),
                missing_hint: hint,
            });
        }
        if last.starts_with("ImportError: ") {
            return Some(ImportErrorReport {
                traceback: stderr.to_string(),
                error_class: "ImportError".to_string(),
                missing_hint: None,
            });
        }
        None
    }

    /// Pull the module name out of `No module named 'X'` or
    /// `No module named "X"`. Tolerant of additional text after the quoted
    /// name (Python sometimes adds `; 'X' is not a package`).
    fn extract_quoted_module(message: &str) -> Option<String> {
        let prefix = "No module named ";
        let rest = message.strip_prefix(prefix)?;
        let bytes = rest.as_bytes();
        let (quote, start) = match bytes.first()? {
            b'\'' => ('\'', 1),
            b'"' => ('"', 1),
            _ => return None,
        };
        let after_open = &rest[start..];
        let end = after_open.find(quote)?;
        Some(after_open[..end].to_string())
    }
    ```

- [ ] **Step 6.4: Re-export from `src/deps_check/mod.rs`**

    Final shape:

    ```rust
    //! Missing-dependency diagnostics.

    pub mod post_mortem;
    pub mod preflight;
    pub mod probe;

    pub use post_mortem::{ImportErrorReport, intercept_import_error};
    pub use preflight::{MissingDeps, check_imports};
    pub use probe::{ProbeOutcome, probe_module, site_packages_dir};

    #[cfg(test)]
    mod tests;
    ```

- [ ] **Step 6.5: Run the tests, expect PASS**

    ```bash
    cargo test --lib deps_check::
    ```

    Expected: 19 tests passing (13 prior + 6 new).

- [ ] **Step 6.6: Commit**

    ```bash
    git add src/deps_check/
    git commit -m "feat(deps-check): Add post-mortem ImportError interceptor"
    ```

---

## Task 7: Wire post-mortem into runner spawn

Capture the runner subprocess's stderr, pass it through
`intercept_import_error`, and on a hit print the rendered report (original
traceback + suggestion) to *toolr's* stderr instead of just propagating the
raw stream. On a miss, pass the captured stderr through verbatim — toolr
must not eat normal Python error output.

Trade-off: this requires switching the spawn from inherited stderr to a
piped stderr that toolr tees back to the terminal. Plan 2's runner spawns
with inherited stdio for transparency; we relax that to "piped, then
forwarded". The user-facing difference for a successful run is zero (stderr
is empty); for an erroring run the stderr arrives at the terminal in one
batch instead of as it streams.

**Files:**

- Modify: `src/bin/toolr/dispatch.rs` (or wherever Plan 2 put `spawn_runner`)
- Modify: `tests/cli_smoke.rs`
- [ ] **Step 7.1: Locate Plan 2's spawn code**

    Plan 2 ships something like (paraphrased):

    ```rust
    pub fn spawn_runner(venv: &Venv, spec_path: &Path) -> Result<ExitStatus> {
        let mut child = std::process::Command::new(venv.python_bin())
            .args(["-m", "toolr._runner"])
            .env("TOOLR_SPEC_FILE", spec_path)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        let status = child.wait()?;
        Ok(status)
    }
    ```

    Replace it (or add a sibling) with the capturing variant below.

- [ ] **Step 7.2: Add the capturing spawn helper**

    In Plan 2's runner module (most likely `src/runner.rs`), add:

    ```rust
    use std::io::{self, Read, Write};
    use std::process::{Command, ExitStatus, Stdio};

    use crate::deps_check::intercept_import_error;

    /// Spawn the runner with stderr piped so we can intercept
    /// `ImportError` tracebacks. Stdout/stdin remain inherited.
    pub fn spawn_runner_with_post_mortem(
        venv: &Venv,
        spec_path: &std::path::Path,
    ) -> std::io::Result<ExitStatus> {
        let mut child = Command::new(venv.python_bin())
            .args(["-m", "toolr._runner"])
            .env("TOOLR_SPEC_FILE", spec_path)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stderr_pipe = child.stderr.take().expect("piped stderr");
        let mut buf = String::new();
        stderr_pipe.read_to_string(&mut buf)?;
        let status = child.wait()?;

        if !status.success() {
            if let Some(report) = intercept_import_error(&buf) {
                io::stderr().write_all(report.render().as_bytes())?;
            } else {
                io::stderr().write_all(buf.as_bytes())?;
            }
        } else {
            // Success path: forward whatever stderr was emitted (warnings,
            // logging) verbatim. ImportError on a successful run would be
            // a Python bug, so we don't try to intercept.
            io::stderr().write_all(buf.as_bytes())?;
        }

        Ok(status)
    }
    ```

    Adjust the `Venv` / `python_bin()` references to the names Plan 3 chose.

- [ ] **Step 7.3: Call the new spawn from `dispatch.rs`**

    Replace the existing `spawn_runner` call (the one Plan 2 added):

    ```rust
    let status = _rust_utils::runner::spawn_runner_with_post_mortem(&venv, &spec_path)?;
    return Ok(ExitCode::from(status.code().unwrap_or(1) as u8));
    ```

- [ ] **Step 7.4: Add a unit test against a captured stderr buffer**

    The end-to-end "real Python subprocess emits an ImportError" test
    needs a real venv, which is too heavy for a unit test. Instead, test
    the rendering path directly. Add to `src/deps_check/tests.rs`:

    ```rust
    #[test]
    fn render_preserves_traceback_byte_for_byte() {
        let stderr = PY_IMPORT_ERROR;
        let report = intercept_import_error(stderr).unwrap();
        let rendered = report.render();
        // The original ends with a newline; render trims trailing
        // whitespace before appending. Verify the meaningful content
        // (every non-trailing-whitespace byte) is preserved.
        let stripped_orig = stderr.trim_end();
        assert!(rendered.starts_with(stripped_orig));
    }
    ```

- [ ] **Step 7.5: Add a subprocess-level integration test**

    The cheapest way to simulate "Python subprocess exits with
    ImportError" without relying on a real venv is to use Rust's
    `--test-threads` and a small Python script piped into `python3` if
    available. Skip the test when no system Python is on PATH.

    Append to `tests/cli_smoke.rs`:

    ```rust
    /// Probe whether a usable system Python exists. If not, the
    /// post-mortem integration tests skip rather than fail.
    fn system_python() -> Option<std::path::PathBuf> {
        for name in ["python3", "python"] {
            if let Ok(out) = std::process::Command::new(name)
                .arg("--version")
                .output()
            {
                if out.status.success() {
                    return Some(name.into());
                }
            }
        }
        None
    }

    #[test]
    fn post_mortem_rewrites_import_error_output() {
        let Some(_) = system_python() else {
            eprintln!("skip: no system python available");
            return;
        };
        // Build a venv-shaped tree where:
        // - the manifest declares no imports (pre-flight is a no-op)
        // - the source file has an inline `import yaml` that will fail
        // - the runner module isn't actually present, so we point the
        //   spawn at `python -c 'raise ImportError(...)'` via a sentinel
        //   env var Plan 2 honors for tests, `TOOLR_RUNNER_OVERRIDE`.
        //
        // If Plan 2 didn't add that override, this test calls out
        // explicitly via env: write a tiny script that `raise`s
        // ModuleNotFoundError, point TOOLR_RUNNER_OVERRIDE at it.

        let tmp = fixture_with_imports(
            "def hello(ctx):\n    import yaml\n",
            // No pre-flight imports — the static parser missed the inline
            // import inside the function body.
            &[],
            &[],
        );

        // Drop a one-line script that exits with the canonical
        // ModuleNotFoundError traceback.
        let script = tmp.path().join("fail_with_import_error.py");
        std::fs::write(
            &script,
            "import sys\n\
             sys.stderr.write(\
             \"Traceback (most recent call last):\\n\"\
             \"  File \\\"<tool>\\\", line 1, in <module>\\n\"\
             \"    import yaml\\n\"\
             \"ModuleNotFoundError: No module named 'yaml'\\n\")\n\
             sys.exit(1)\n",
        )
        .unwrap();

        let output = Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .env("TOOLR_VENV_OVERRIDE", tmp.path().join("fake-venv"))
            .env("TOOLR_RUNNER_OVERRIDE", &script)
            .args(["ci", "hello"])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_ne!(output.status.code(), Some(0));
        // Original traceback preserved.
        assert!(stderr.contains("ModuleNotFoundError: No module named 'yaml'"));
        // Toolr suggestion appended.
        assert!(stderr.contains("toolr project deps sync"));
        assert!(stderr.contains("yaml"));
    }
    ```

    `TOOLR_RUNNER_OVERRIDE` is the test seam Plan 2 should provide for
    redirecting the subprocess target. If Plan 2 omitted it, add a one-line
    branch in `spawn_runner_with_post_mortem` honoring it:

    ```rust
    let (program, args): (std::path::PathBuf, Vec<&str>) =
        match std::env::var_os("TOOLR_RUNNER_OVERRIDE") {
            Some(path) => (std::path::PathBuf::from(path), vec![]),
            None => (
                venv.python_bin().to_path_buf(),
                vec!["-m", "toolr._runner"],
            ),
        };
    let mut child = Command::new(&program)
        .args(&args)
        .env("TOOLR_SPEC_FILE", spec_path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .spawn()?;
    ```

    (Strictly, this isn't part of Plan 7's domain — it's a missing test
    hook in Plan 2. Surface to the Plan 2 author or fold into this plan.)

- [ ] **Step 7.6: Run the tests**

    ```bash
    cargo test --lib deps_check::
    cargo test --test cli_smoke post_mortem_
    ```

    Expected: deps_check tests pass (now 20); the post_mortem integration
    test passes if system Python is available.

- [ ] **Step 7.7: Commit**

    ```bash
    git add src/deps_check/ src/runner.rs src/bin/toolr/dispatch.rs tests/cli_smoke.rs
    git commit -m "feat(runner): Intercept ImportError tracebacks and append sync hint"
    ```

---

## Task 8: End-to-end test — pre-flight vs post-mortem split

A single integration test that demonstrates the two paths against the
same project: one command with a top-level import that the static parser
sees (caught by pre-flight); one command with an inline import that the
static parser misses (caught by post-mortem). This is the readable proof
that the design's user contract holds.

**Files:**

- Modify: `tests/cli_smoke.rs`
- [ ] **Step 8.1: Build the dual-command fixture**

    ```rust
    fn fixture_with_two_commands(
        top_level_import: &str,
        inline_import: &str,
    ) -> tempfile::TempDir {
        use std::fs;
        let tmp = tempfile::TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        fs::create_dir_all(&tools).unwrap();

        let src = format!(
            "import {top_level_import}\n\
             \n\
             def with_top_level(ctx): pass\n\
             \n\
             def with_inline(ctx):\n    import {inline_import}\n",
        );
        fs::write(tools.join("ci.py"), src).unwrap();

        let manifest = format!(
            r#"{{
                "schema_version": 1,
                "static_hash": "h",
                "groups": [{{
                    "name": "ci", "title": "CI", "description": "",
                    "origin": "static"
                }}],
                "commands": [
                    {{
                        "name": "with-top-level", "group": "ci",
                        "module": "tools.ci", "function": "with_top_level",
                        "summary": "", "description": "",
                        "arguments": [], "imports": ["{top_level_import}"],
                        "origin": "static"
                    }},
                    {{
                        "name": "with-inline", "group": "ci",
                        "module": "tools.ci", "function": "with_inline",
                        "summary": "", "description": "",
                        "arguments": [], "imports": [],
                        "origin": "static"
                    }}
                ]
            }}"#
        );
        fs::write(tools.join(".toolr-manifest.json"), manifest).unwrap();

        // Empty venv → both modules missing. Pre-flight should reject
        // `with-top-level` because its top-level import is recorded; the
        // runner should reject `with-inline` because its import only
        // appears inside the function body.
        let venv = tmp.path().join("fake-venv");
        let sp = venv.join("lib").join("python3.13").join("site-packages");
        fs::create_dir_all(&sp).unwrap();
        let toolr_pkg = sp.join("toolr");
        fs::create_dir_all(&toolr_pkg).unwrap();
        fs::write(toolr_pkg.join("__init__.py"), "").unwrap();

        tmp
    }

    #[test]
    fn pre_flight_and_post_mortem_split_against_one_project() {
        let Some(_) = system_python() else {
            eprintln!("skip: no system python available");
            return;
        };
        let tmp = fixture_with_two_commands("yaml", "cv2");

        // 1. Top-level import case → pre-flight catches it.
        let top = Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .env("TOOLR_VENV_OVERRIDE", tmp.path().join("fake-venv"))
            .args(["ci", "with-top-level"])
            .output()
            .unwrap();
        let top_err = String::from_utf8_lossy(&top.stderr);
        assert_eq!(top.status.code(), Some(78), "pre-flight exit code");
        assert!(top_err.contains("import `yaml` not found"));
        assert!(top_err.contains("toolr project deps sync"));
        // Pre-flight should NOT include a Python traceback.
        assert!(!top_err.contains("Traceback"));

        // 2. Inline import case → pre-flight passes, runner fires the
        // ImportError, post-mortem rewrites the output.
        let script = tmp.path().join("fail_with_import_error.py");
        std::fs::write(
            &script,
            "import sys\n\
             sys.stderr.write(\
             \"Traceback (most recent call last):\\n\"\
             \"  File \\\"<tool>\\\", line 2, in with_inline\\n\"\
             \"    import cv2\\n\"\
             \"ModuleNotFoundError: No module named 'cv2'\\n\")\n\
             sys.exit(1)\n",
        )
        .unwrap();

        let inline = Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .env("TOOLR_VENV_OVERRIDE", tmp.path().join("fake-venv"))
            .env("TOOLR_RUNNER_OVERRIDE", &script)
            .args(["ci", "with-inline"])
            .output()
            .unwrap();
        let inline_err = String::from_utf8_lossy(&inline.stderr);
        // Non-zero exit code passed through from the runner.
        assert_ne!(inline.status.code(), Some(0));
        // Original traceback preserved verbatim.
        assert!(inline_err.contains("Traceback (most recent call last)"));
        assert!(inline_err.contains("ModuleNotFoundError: No module named 'cv2'"));
        // toolr suggestion appended.
        assert!(inline_err.contains("toolr project deps sync"));
        assert!(inline_err.contains("cv2"));
    }
    ```

- [ ] **Step 8.2: Run the test**

    ```bash
    cargo test --test cli_smoke pre_flight_and_post_mortem_split
    ```

    Expected: 1 test passing (or `skip: no system python available`
    when running on a host without Python).

- [ ] **Step 8.3: Commit**

    ```bash
    git add tests/cli_smoke.rs
    git commit -m "test(cli): Cover pre-flight vs post-mortem split end-to-end"
    ```

---

## Task 9: Update the roadmap

Mark Plan 7 as Done once Tasks 1–8 are merged.

**Files:**

- Modify: `specs/rust-front-end/01-roadmap.md`
- [ ] **Step 9.1: Update the Plan 7 entry**

    Change the Plan 7 block from:

    ```markdown
    ### Plan 7: Missing-dependency diagnostics

    - **Status:** ⬜ Not Started
    - **Plan doc:** _(not written yet)_
    ```

    to:

    ```markdown
    ### Plan 7: Missing-dependency diagnostics

    - **Status:** ✅ Done
    - **Plan doc:** [08-plan-7-missing-deps.md](./08-plan-7-missing-deps.md)
    ```

    Leave the rest of the entry (Depends on / Unblocks / Produces) intact.

- [ ] **Step 9.2: Commit**

    ```bash
    git add specs/rust-front-end/01-roadmap.md
    git commit -m "docs(roadmap): Mark Plan 7 as done"
    ```

---

## Done criteria

Plan 7 is complete when:

- `cargo test --lib deps_check::` passes — all 20 unit tests across probe,
  preflight, and post_mortem modules.
- `cargo test --test cli_smoke preflight_` passes — pre-flight blocks
  missing top-level imports with exit code 78 and the canonical message.
- `cargo test --test cli_smoke preflight_can_be_disabled` passes —
  `TOOLR_NO_PREFLIGHT_DEPS=1` short-circuits the check.
- `cargo test --test cli_smoke post_mortem_` passes (when a system Python
  is available) — the original Python traceback is preserved verbatim,
  followed by a `toolr project deps sync` suggestion.
- `cargo test --test cli_smoke pre_flight_and_post_mortem_split` passes —
  a single project demonstrates both diagnostic paths against two
  commands.
- A real `tools/` directory where a declared dependency has been removed
  from `tools/pyproject.toml` produces the pre-flight message; a real
  command with an inline import of an absent package produces the
  post-mortem message.
- The roadmap status table reflects Plan 7 as `✅ Done`.

## Open questions (for the implementer)

These are deliberately deferred — surface to the spec author if any block
progress, otherwise resolve in line:

1. **Venv-override env var name.** Task 4 assumes Plan 3 exposes
   `TOOLR_VENV_OVERRIDE` for forcing a venv path in tests. If Plan 3
   chose a different name (e.g. `TOOLR_TOOLS_VENV`), rename in both
   `dispatch.rs` and `tests/cli_smoke.rs` consistently.
2. **Runner-override env var.** Task 7 / Task 8 assume a
   `TOOLR_RUNNER_OVERRIDE` test seam in Plan 2's spawn code. If Plan 2
   omitted it, add the minimal branch shown in Task 7 Step 7.5 — but
   confirm with the Plan 2 author that this is the agreed test seam
   rather than an ad-hoc addition.
3. **Stderr capture vs streaming.** Task 7 switches the runner from
   inherited stderr to piped stderr. For long-running commands that
   write progress to stderr (e.g. rich progress bars), this changes the
   user experience: output arrives at completion instead of streaming.
   The alternative — a tee'd reader that forwards each line as it
   arrives and also buffers for post-mortem — adds complexity. v1
   choice: full capture. Revisit if users complain about laggy stderr.
4. **Namespace-package false negatives.** The probe checks for
   `__init__.py` and `<module>.py`. PEP 420 namespace packages (bare
   directory, no `__init__.py`) will fail pre-flight even though
   `import` would succeed. The escape hatch `TOOLR_NO_PREFLIGHT_DEPS`
   covers this, but it's worth deciding whether to also probe for "any
   directory of that name with at least one `.py` inside" as a softer
   heuristic. v1 chooses the strict check — explicit is better than
   accidental.
5. **`ImportError` vs other side-effect failures.** Some Python packages
   raise `ImportError` from their `__init__.py` for non-dependency
   reasons (broken C extensions, runtime configuration errors, etc.).
   Post-mortem will incorrectly append the "run deps sync" suggestion in
   those cases. The cost is one misleading suggestion among accurate
   ones; the alternative — trying to distinguish "missing dep" from
   "broken dep" by parsing the message — is fragile and provides
   marginal benefit. v1 accepts this false-positive case.
