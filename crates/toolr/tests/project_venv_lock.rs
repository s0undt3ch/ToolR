//! Integration tests for `toolr project venv lock`. Like the other
//! `project_venv_*` tests, these don't run real uv; they cover the
//! pre-flight validation and discovery paths.

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

fn cargo_bin() -> Command {
    Command::cargo_bin("toolr").unwrap()
}

/// `--help` lists the new -U / -P flags.
#[test]
fn lock_help_lists_upgrade_flags() {
    let output = cargo_bin()
        .args(["project", "venv", "lock", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success(), "help should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--upgrade"), "help missing --upgrade: {stdout}");
    assert!(stdout.contains("--upgrade-package"), "help missing --upgrade-package: {stdout}");
}

/// `venv lock` (no flags) reports the missing project root when run
/// outside a toolr-using directory.
#[test]
fn lock_errors_when_not_in_a_toolr_repo() {
    let tmp = TempDir::new().unwrap();
    let output = cargo_bin()
        .current_dir(tmp.path())
        .args(["project", "venv", "lock"])
        .output()
        .unwrap();

    assert!(!output.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("locating project root")
            || stderr.contains("resolving the tools venv path"),
        "stderr should explain the missing project, got:\n{stderr}"
    );
}

/// `venv lock -P <pkg>` runs the same pyproject pre-flight as
/// `venv sync -P <pkg>`.
#[test]
fn lock_dash_p_errors_when_package_not_declared() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("tools")).unwrap();
    fs::write(
        tmp.path().join("tools/pyproject.toml"),
        r#"[project]
name = "tools"
version = "0.0.0"
requires-python = ">=3.11"
dependencies = [
    "requests",
]

[tool.toolr]
venv-location = "cache"
"#,
    )
    .unwrap();

    let output = cargo_bin()
        .current_dir(tmp.path())
        .args(["project", "venv", "lock", "-P", "nonexistent-package"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not declared"), "stderr: {stderr}");
    assert!(stderr.contains("nonexistent-package"), "stderr: {stderr}");
}
