use std::path::PathBuf;

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
        "third_party_hash": "",
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

/// Returns `Some(path-to-python)` if a Python with msgspec + the local
/// `toolr` package installed is available, otherwise `None`. We accept
/// the project's own dev venv (created by `uv sync`) as the runner.
///
/// These smoke tests deliberately exercise the no-pyproject fallback
/// path in `dispatch.rs`: they write a `tools/.toolr-manifest.json`
/// but no `tools/pyproject.toml`, so the dispatcher resolves Python
/// via `TOOLR_PYTHON` rather than the tools venv. The full
/// venv-driven path is covered by the network-dependent
/// `end_to_end_sync` test.
fn detect_test_python() -> Option<PathBuf> {
    let candidate = std::env::var_os("TOOLR_TEST_PYTHON").map(PathBuf::from);
    let candidate = candidate.or_else(|| {
        // Project dev venv from `uv sync`.
        let p = PathBuf::from(".venv/bin/python");
        if p.exists() { Some(p) } else { None }
    })?;
    // Make absolute without canonicalizing (canonicalize resolves the
    // venv symlink to the real interpreter, which loses the venv's
    // site-packages — and with it, the `toolr` editable install).
    // Tests later set `current_dir` to a tmpdir, so a relative path
    // wouldn't survive across the spawn.
    let python = if candidate.is_absolute() {
        candidate
    } else {
        std::env::current_dir().ok()?.join(candidate)
    };
    // Verify it can import `toolr._runner`. If not, skip.
    let status = std::process::Command::new(&python)
        .args(["-c", "import toolr._runner"])
        .status()
        .ok()?;
    if status.success() { Some(python) } else { None }
}

fn write_tools_demo(repo_root: &std::path::Path) {
    let tools_dir = repo_root.join("tools");
    std::fs::create_dir_all(&tools_dir).unwrap();
    std::fs::write(tools_dir.join("__init__.py"), "").unwrap();
    std::fs::write(
        tools_dir.join("demo.py"),
        r#"
from toolr import command_group

group = command_group("demo", "Demo", description="demo group")

@group.command
def hello(ctx, name: str = "world") -> None:
    ctx.print(f"hi {name}")
"#,
    )
    .unwrap();
    let manifest = r#"{
        "schema_version": 1, "static_hash": "h", "third_party_hash": "",
        "groups": [{"name": "demo", "title": "Demo", "description": "", "origin": "static"}],
        "commands": [{
            "name": "hello", "group": "demo", "module": "tools.demo",
            "function": "hello", "summary": "", "description": "",
            "arguments": [
                {
                    "name": "name", "kind": "optional", "help": "",
                    "default": "world", "type_annotation": "str",
                    "allowed_values": []
                }
            ],
            "imports": [], "origin": "static"
        }]
    }"#;
    std::fs::write(tools_dir.join(".toolr-manifest.json"), manifest).unwrap();
}

#[test]
fn running_a_user_command_invokes_python_runner() {
    let Some(python) = detect_test_python() else {
        eprintln!(
            "skipping: no .venv/bin/python with toolr installed. \
             Run `uv sync` first, or set TOOLR_TEST_PYTHON to a python \
             that can `import toolr._runner`."
        );
        return;
    };
    let tmp = TempDir::new().unwrap();
    write_tools_demo(tmp.path());
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .env("TOOLR_PYTHON", &python)
        .env("PYTHONPATH", tmp.path())
        .args(["demo", "hello", "--name", "Alice"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "expected success, got code {:?}\nstderr:\n{stderr}\nstdout:\n{stdout}",
        output.status.code()
    );
    assert!(stdout.contains("hi Alice"), "stdout was:\n{stdout}");
}

#[test]
fn project_manifest_rebuild_help_lists_command() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    std::fs::create_dir(&tools).unwrap();
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["project", "manifest", "--help"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("rebuild"),
        "expected rebuild listed, got:\n{stdout}"
    );
}

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

/// Build a fixture project with:
/// - `tools/pyproject.toml` opting into the in-tree venv layout so
///   `resolve_venv_path` lands at `tools/.venv/`.
/// - `tools/.venv/lib/python3.13/site-packages/` containing the modules
///   listed in `present_in_venv` (each materialised as a package with an
///   `__init__.py`).
/// - `tools/.venv/bin/python` as a fake interpreter that just exits 1
///   when the runner is spawned (so we exercise stdout/stderr handling).
/// - A `tools/.toolr-manifest.json` with one `ci hello` command whose
///   `imports` list is whatever the test passes.
#[cfg(unix)]
fn preflight_fixture(
    imports: &[&str],
    present_in_venv: &[&str],
) -> tempfile::TempDir {
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir_all(&tools).unwrap();
    fs::write(
        tools.join("pyproject.toml"),
        "[project]\nname = \"tools\"\nversion = \"0.0.0\"\n\
         [tool.toolr]\nvenv-location = \"in-tree\"\n",
    )
    .unwrap();
    fs::write(tools.join("__init__.py"), "").unwrap();
    fs::write(tools.join("ci.py"), "def hello(ctx): pass\n").unwrap();

    // Build the venv tree before computing hashes so the third_party_hash
    // accounts for the final site-packages state.
    let sp = tools
        .join(".venv")
        .join("lib")
        .join("python3.13")
        .join("site-packages");
    fs::create_dir_all(&sp).unwrap();
    for name in present_in_venv {
        let pkg = sp.join(name);
        fs::create_dir(&pkg).unwrap();
        fs::write(pkg.join("__init__.py"), "").unwrap();
    }
    // `toolr` itself must appear installed so the venv-validate guard
    // (if any) doesn't trip the dispatcher before pre-flight runs.
    let toolr_pkg = sp.join("toolr");
    fs::create_dir_all(&toolr_pkg).unwrap();
    fs::write(toolr_pkg.join("__init__.py"), "").unwrap();

    // Stamp real hashes so ensure_manifest_fresh treats the manifest as
    // Fresh and doesn't rebuild it (which would drop the `imports` field
    // that the pre-flight tests rely on).
    let static_hash = toolr_core::hash::hash_tools_dir(&tools).unwrap();
    let venv_dir = tools.join(".venv");
    let third_party_hash =
        toolr_core::dynamic::compute_third_party_hash(&venv_dir).unwrap();

    let imports_json: String = imports
        .iter()
        .map(|i| format!("\"{i}\""))
        .collect::<Vec<_>>()
        .join(",");
    let manifest = format!(
        r#"{{
            "schema_version": 1,
            "static_hash": "{static_hash}", "third_party_hash": "{third_party_hash}",
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

    let bin_dir = tools.join(".venv").join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let py = bin_dir.join("python");
    let mut f = fs::File::create(&py).unwrap();
    writeln!(f, "#!/bin/sh").unwrap();
    // The dispatcher invokes `python -m toolr._introspect ...` during
    // auto-rebuild and `python -m toolr._runner` during the actual
    // command execution. Branch on the argv so both work without a
    // real Python.
    writeln!(f, r#"case " $* " in"#).unwrap();
    writeln!(
        f,
        r#"  *toolr._introspect*) echo '{{"payload_schema_version":1,"groups":[],"commands":[],"warnings":[]}}'; exit 0;;"#
    )
    .unwrap();
    writeln!(f, "  *) exit 1;;").unwrap();
    writeln!(f, "esac").unwrap();
    drop(f);
    let mut perms = fs::metadata(&py).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&py, perms).unwrap();

    tmp
}

#[test]
#[cfg(unix)]
fn preflight_fails_when_an_import_is_missing_from_venv() {
    let tmp = preflight_fixture(&["yaml"], &[]);
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["ci", "hello"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(78), "stderr:\n{stderr}");
    assert!(
        stderr.contains("import `yaml` not found"),
        "stderr:\n{stderr}"
    );
    assert!(stderr.contains("toolr project deps sync"));
}

/// Regression: deleting the resolved tools venv's `bin/python` between
/// invocations used to surface as the bare error
/// `toolr: No such file or directory (os error 2)` — the user couldn't
/// tell what was missing or how to recover. Dispatch now pre-checks
/// `python.is_file()` and emits a message that names the path plus
/// `toolr project deps sync` as the recovery action.
#[test]
#[cfg(unix)]
fn dispatch_emits_clear_error_when_venv_python_is_missing() {
    use std::fs;

    let tmp = preflight_fixture(&[], &[]);
    let py = tmp
        .path()
        .join("tools")
        .join(".venv")
        .join("bin")
        .join("python");
    assert!(py.exists(), "fixture should start with a stub python");
    fs::remove_file(&py).unwrap();

    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["ci", "hello"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_ne!(output.status.code(), Some(0), "expected failure; stderr:\n{stderr}");
    assert!(
        stderr.contains("Python interpreter not found at"),
        "expected a 'Python interpreter not found at <path>' line; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("toolr project deps sync"),
        "expected recovery hint; stderr:\n{stderr}"
    );
    // Anti-regression: the old bare `os error 2` line must not be the
    // entire message.
    assert!(
        !stderr.trim().ends_with("No such file or directory (os error 2)")
            || stderr.contains("Python interpreter not found at"),
        "stderr should not be just the bare io::Error display; stderr:\n{stderr}"
    );
}

/// Regression: when the pre-flight is disabled and the runner emits a
/// traceback, the Rust side must pass that traceback through to the
/// terminal *unaltered* — no capture, no rewriting. The styled
/// "run `toolr project deps sync`" hint on `ImportError` is the
/// runner's responsibility (see `toolr._runner.run()` in toolr-py),
/// and is covered by Python-side runner tests. Here we just guard
/// against a future regression that re-introduces a capture path.
#[test]
#[cfg(unix)]
fn dispatch_passes_runner_traceback_through_unaltered() {
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let tmp = preflight_fixture(&[], &[]);
    // Replace the fake python so its non-introspect path emits a
    // ModuleNotFoundError traceback (simulating the real runner
    // failing at import time on an inline import).
    let py = tmp
        .path()
        .join("tools")
        .join(".venv")
        .join("bin")
        .join("python");
    fs::remove_file(&py).unwrap();
    let mut f = fs::File::create(&py).unwrap();
    writeln!(f, "#!/bin/sh").unwrap();
    writeln!(f, r#"case " $* " in"#).unwrap();
    writeln!(
        f,
        r#"  *toolr._introspect*) echo '{{"payload_schema_version":1,"groups":[],"commands":[],"warnings":[]}}'; exit 0;;"#
    )
    .unwrap();
    writeln!(f, "  *)").unwrap();
    writeln!(
        f,
        r#"    printf 'Traceback (most recent call last):\n  File "<tool>", line 2, in hello\n    import yaml\nModuleNotFoundError: No module named '"'"'yaml'"'"'\n' 1>&2"#
    )
    .unwrap();
    writeln!(f, "    exit 1;;").unwrap();
    writeln!(f, "esac").unwrap();
    drop(f);
    let mut perms = fs::metadata(&py).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&py, perms).unwrap();

    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["ci", "hello"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_ne!(output.status.code(), Some(0), "stderr:\n{stderr}");
    assert!(
        stderr.contains("ModuleNotFoundError: No module named 'yaml'"),
        "expected traceback preserved verbatim, got:\n{stderr}"
    );
}

#[test]
#[cfg(unix)]
fn preflight_can_be_disabled_with_env_var() {
    let tmp = preflight_fixture(&["yaml"], &[]);
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .env("TOOLR_NO_PREFLIGHT_DEPS", "1")
        .args(["ci", "hello"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("not found in tools venv"),
        "stderr:\n{stderr}"
    );
    // Pre-flight skipped → runner spawn proceeds (fake python exits 1).
    assert_ne!(output.status.code(), Some(78));
}

#[test]
#[cfg(unix)]
fn preflight_passes_when_all_imports_present() {
    let tmp = preflight_fixture(&["packaging"], &["packaging"]);
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["ci", "hello"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Pre-flight passes → fake python runner runs and exits 1 (no
    // pre-flight diagnostic in stderr).
    assert!(
        !stderr.contains("not found in tools venv"),
        "stderr:\n{stderr}"
    );
    assert_ne!(output.status.code(), Some(78));
}

#[test]
fn user_command_propagates_nonzero_exit() {
    let Some(python) = detect_test_python() else {
        eprintln!("skipping: no test python (see above)");
        return;
    };
    let tmp = TempDir::new().unwrap();
    let tools_dir = tmp.path().join("tools");
    std::fs::create_dir_all(&tools_dir).unwrap();
    std::fs::write(tools_dir.join("__init__.py"), "").unwrap();
    std::fs::write(
        tools_dir.join("demo.py"),
        r#"
from toolr import command_group

group = command_group("demo", "Demo", description="demo group")

@group.command
def boom(ctx) -> None:
    ctx.exit(7, "failing on purpose")
"#,
    )
    .unwrap();
    let manifest = r#"{
        "schema_version": 1, "static_hash": "h", "third_party_hash": "",
        "groups": [{"name": "demo", "title": "Demo", "description": "", "origin": "static"}],
        "commands": [{
            "name": "boom", "group": "demo", "module": "tools.demo",
            "function": "boom", "summary": "", "description": "",
            "arguments": [], "imports": [], "origin": "static"
        }]
    }"#;
    std::fs::write(tools_dir.join(".toolr-manifest.json"), manifest).unwrap();
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .env("TOOLR_PYTHON", &python)
        .env("PYTHONPATH", tmp.path())
        .args(["demo", "boom"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(7));
}
