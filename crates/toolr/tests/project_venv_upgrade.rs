//! Integration tests for `toolr project venv upgrade <pkg>`.
//!
//! These tests don't actually run uv — they exercise the input-validation
//! and discovery paths that fire before the uv invocation, where the
//! command surface is most likely to regress.

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

fn cargo_bin() -> Command {
    Command::cargo_bin("toolr").unwrap()
}

/// Requesting a package that isn't declared in tools/pyproject.toml fails
/// loudly instead of silently no-opping inside uv.
#[test]
fn upgrade_errors_when_package_not_declared() {
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
        .args(["project", "venv", "upgrade", "nonexistent-package"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not declared"),
        "expected validation error, stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("nonexistent-package"),
        "stderr should name the package, stderr was:\n{stderr}"
    );
}

/// `toolr project venv upgrade` with no package name fails clap arg parsing
/// (not a runtime error).
#[test]
fn upgrade_requires_a_package_argument() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("tools")).unwrap();
    fs::write(tmp.path().join("tools/pyproject.toml"), "[project]\nname=\"tools\"\n").unwrap();

    let output = cargo_bin()
        .current_dir(tmp.path())
        .args(["project", "venv", "upgrade"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("PACKAGE") || stderr.contains("required") || stderr.contains("usage"),
        "expected clap usage error, stderr was:\n{stderr}"
    );
}

/// `toolr project venv upgrade --help` lists the command and the PACKAGE
/// positional, with a meaningful one-liner.
#[test]
fn upgrade_help_lists_package_positional() {
    let output = cargo_bin()
        .args(["project", "venv", "upgrade", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success(), "help should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PACKAGE"), "help missing PACKAGE: {stdout}");
    assert!(stdout.contains("upgrade-package"), "help should reference the underlying uv verb: {stdout}");
}
