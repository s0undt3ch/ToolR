//! End-to-end smoke. Requires network access (uv download) and that
//! Plan 2's runner is already wired. Run explicitly with:
//!
//!     cargo test --test end_to_end_sync -- --ignored --nocapture

use assert_cmd::Command;
use tempfile::TempDir;

const PYPROJECT: &str = r#"
[project]
name = "toolr-tools"
version = "0"
requires-python = ">=3.11"
dependencies = ["toolr"]

[tool.toolr]
venv-location = "in-tree"
"#;

#[test]
#[ignore = "network-touching: requires uv to be available or installable"]
fn deps_sync_then_run_user_command() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    std::fs::create_dir(&tools).unwrap();
    std::fs::write(tools.join("pyproject.toml"), PYPROJECT).unwrap();
    // A minimal command file so Plan 1 picks up a group.
    std::fs::write(
        tools.join("ci.py"),
        r#"
"""CI helpers."""

from toolr import command_group

group = command_group("ci", "CI helpers", docstring=__doc__)

@group.command
def hello(ctx):
    """Say hello."""
    print("hello from tools.ci")
"#,
    )
    .unwrap();

    // 1. Build the static manifest.
    Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["__build-static-manifest"])
        .env("TOOLR_AUTO_INSTALL_UV", "1")
        .assert()
        .success();

    // 2. Sync the venv (will install uv on first run if needed).
    Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["project", "deps", "sync"])
        .env("TOOLR_AUTO_INSTALL_UV", "1")
        .assert()
        .success();

    // 3. Run the user command — Plan 2's runner now executes via the
    //    venv python.
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["ci", "hello"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("hello from tools.ci"));
}
