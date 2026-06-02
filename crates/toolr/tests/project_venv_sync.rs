//! Integration tests for `toolr project venv sync` — the renamed
//! and behavior-flipped successor to `toolr project deps sync`.
//!
//! Covers the flag surface and the unattended-mode (`--quiet`) guard
//! table. Tests deliberately don't run real `uv`; they exercise the
//! input-validation and discovery paths that fire before the uv
//! invocation, matching the convention of `project_venv_upgrade.rs`.

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

fn cargo_bin() -> Command {
    Command::cargo_bin("toolr").unwrap()
}

/// `toolr project venv sync` (no flags) reports the missing project
/// root when run outside a toolr-using directory.
#[test]
fn sync_errors_when_not_in_a_toolr_repo() {
    let tmp = TempDir::new().unwrap();
    let output = cargo_bin()
        .current_dir(tmp.path())
        .args(["project", "venv", "sync"])
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

/// `--quiet` swallows the not-a-toolr-repo case: exit 0, silent.
/// This is the contract the mise enter-hook recipe depends on.
#[test]
fn sync_quiet_silently_exits_when_not_a_toolr_repo() {
    let tmp = TempDir::new().unwrap();
    let output = cargo_bin()
        .current_dir(tmp.path())
        .args(["project", "venv", "sync", "--quiet"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "--quiet must exit 0 when not in a toolr repo; stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty() && output.stderr.is_empty(),
        "--quiet must produce no output;\nstdout={:?}\nstderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// `--quiet` swallows the missing-pyproject case too. The hook fires
/// on every `cd` and toolr should never surface noise for non-toolr
/// directories — even ones whose ancestor happened to be a toolr repo.
#[test]
fn sync_quiet_silently_exits_when_pyproject_missing() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("tools")).unwrap();
    // No tools/pyproject.toml.

    let output = cargo_bin()
        .current_dir(tmp.path())
        .args(["project", "venv", "sync", "--quiet"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "--quiet must exit 0 when tools/pyproject.toml is missing; stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `--help` for the new subcommand lists both flags. (`cli_smoke.rs`
/// has a parallel test; this one anchors the assertion in the
/// venv-sync-specific test file so a future split of these test
/// crates keeps the contract close to the behavior it tests.)
#[test]
fn sync_help_lists_force_and_quiet() {
    let output = cargo_bin()
        .args(["project", "venv", "sync", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success(), "help should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--force"), "help missing --force: {stdout}");
    assert!(stdout.contains("--quiet"), "help missing --quiet: {stdout}");
    assert!(
        stdout.contains("no-op when fresh") || stdout.contains("freshness stamp"),
        "help should describe the new no-op-when-fresh default: {stdout}"
    );
}

/// `venv sync --help` lists the new -U / -P flags.
#[test]
fn sync_help_lists_upgrade_flags() {
    let output = cargo_bin()
        .args(["project", "venv", "sync", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success(), "help should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--upgrade"), "help missing --upgrade: {stdout}");
    assert!(stdout.contains("--upgrade-package"), "help missing --upgrade-package: {stdout}");
}

/// `venv sync -P` with an unknown package fails the pyproject pre-flight
/// guard the same way `venv upgrade` used to.
#[test]
fn sync_dash_p_errors_when_package_not_declared() {
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
        .args(["project", "venv", "sync", "-P", "nonexistent-package"])
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
