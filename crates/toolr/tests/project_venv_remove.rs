//! Integration tests for `toolr project venv remove`. Cover clap +
//! pre-flight guard (package must be declared).

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

fn cargo_bin() -> Command {
    Command::cargo_bin("toolr").unwrap()
}

#[test]
fn remove_help_lists_package_positional() {
    let output = cargo_bin()
        .args(["project", "venv", "remove", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success(), "help should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PACKAGE"), "help missing PACKAGE: {stdout}");
    assert!(stdout.contains("uv remove"), "help should reference `uv remove`: {stdout}");
}

#[test]
fn remove_requires_at_least_one_package() {
    let output = cargo_bin()
        .args(["project", "venv", "remove"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("required") || stderr.contains("PACKAGE") || stderr.contains("usage"),
        "expected clap usage error, stderr was:\n{stderr}"
    );
}

#[test]
fn remove_errors_when_package_not_declared() {
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
        .args(["project", "venv", "remove", "nonexistent-package"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not declared"), "stderr: {stderr}");
    assert!(stderr.contains("nonexistent-package"), "stderr: {stderr}");
}
