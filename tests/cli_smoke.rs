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

/// Returns `Some(path-to-python)` if a Python with msgspec + the local
/// `toolr` package installed is available, otherwise `None`. We accept
/// the project's own dev venv (created by `uv sync`) as the runner.
///
/// These smoke tests deliberately exercise the *legacy* fallback path in
/// `dispatch.rs`: they write a `tools/.toolr-manifest.json` but no
/// `tools/pyproject.toml`, so the dispatcher resolves Python via
/// `TOOLR_PYTHON` rather than the tools venv. The full venv-driven path
/// is covered by the network-dependent `end_to_end_sync` test (Task 17).
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
        "schema_version": 1, "static_hash": "h", "dynamic_hash": "",
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
        "schema_version": 1, "static_hash": "h", "dynamic_hash": "",
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
