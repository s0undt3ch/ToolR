//! Integration tests for `toolr self build-manifest`. Drives the
//! installed `toolr` binary via `assert_cmd` so the assertions exercise
//! the same code path users hit on the command line.

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn write(tmp: &Path, rel: &str, contents: &str) {
    let path = tmp.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

/// Lay down a one-command package at `<tmp>/<name>/` so the build path
/// has something to chew on. Returns the package directory.
fn minimal_plugin(tmp: &Path, name: &str) -> PathBuf {
    let pkg = tmp.join(name);
    write(&pkg, "__init__.py", "");
    write(
        &pkg,
        "commands.py",
        r#""""Commands."""
from toolr import Context
from toolr import command_group

g = command_group("g", "G")

@g.command
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

    let output = Command::cargo_bin("toolr")
        .unwrap()
        .args(["self", "build-manifest", "--source-dir"])
        .arg(&pkg)
        .args(["--package", "mypkg"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "build-manifest failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let written = pkg.join("toolr-manifest.json");
    assert!(written.is_file(), "manifest was not written to {}", written.display());
    let body = fs::read_to_string(&written).unwrap();
    assert!(body.contains("\"package\": \"mypkg\""), "missing package field: {body}");
    assert!(body.contains("\"name\": \"hi\""), "missing hi command: {body}");
    assert!(body.ends_with('\n'), "missing trailing newline");
}

#[test]
fn check_passes_when_manifest_is_in_sync() {
    let tmp = TempDir::new().unwrap();
    let pkg = minimal_plugin(tmp.path(), "mypkg");
    // Generate once, then --check.
    let gen = Command::cargo_bin("toolr")
        .unwrap()
        .args(["self", "build-manifest", "--source-dir"])
        .arg(&pkg)
        .args(["--package", "mypkg"])
        .output()
        .unwrap();
    assert!(gen.status.success());

    let check = Command::cargo_bin("toolr")
        .unwrap()
        .args(["self", "build-manifest", "--source-dir"])
        .arg(&pkg)
        .args(["--package", "mypkg", "--check"])
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "--check should pass against just-generated manifest; stderr: {}",
        String::from_utf8_lossy(&check.stderr)
    );
}

#[test]
fn check_emits_diff_and_exits_2_on_drift() {
    let tmp = TempDir::new().unwrap();
    let pkg = minimal_plugin(tmp.path(), "mypkg");
    // Plant a stale manifest at the expected output path.
    fs::write(pkg.join("toolr-manifest.json"), "{\"stale\": true}\n").unwrap();

    let output = Command::cargo_bin("toolr")
        .unwrap()
        .args(["self", "build-manifest", "--source-dir"])
        .arg(&pkg)
        .args(["--package", "mypkg", "--check"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2), "expected exit 2 on drift");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("out of date"), "expected drift message; got: {stderr}");
}

#[test]
fn rejects_namespace_package() {
    let tmp = TempDir::new().unwrap();
    let pkg = tmp.path().join("nsp");
    fs::create_dir_all(&pkg).unwrap();
    // Deliberately no __init__.py.

    let output = Command::cargo_bin("toolr")
        .unwrap()
        .args(["self", "build-manifest", "--source-dir"])
        .arg(&pkg)
        .args(["--package", "nsp"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("namespace package"),
        "expected namespace-package error; got: {stderr}"
    );
}

#[test]
fn package_positional_conflicts_with_source_dir() {
    let tmp = TempDir::new().unwrap();
    let pkg = minimal_plugin(tmp.path(), "mypkg");

    let output = Command::cargo_bin("toolr")
        .unwrap()
        .args(["self", "build-manifest", "mypkg", "--source-dir"])
        .arg(&pkg)
        .output()
        .unwrap();
    assert!(!output.status.success(), "clap should reject the combination");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflicts"),
        "expected mutex error; got: {stderr}"
    );
}

#[test]
fn python_flag_is_no_longer_accepted() {
    let tmp = TempDir::new().unwrap();
    let pkg = minimal_plugin(tmp.path(), "mypkg");

    let output = Command::cargo_bin("toolr")
        .unwrap()
        .args(["self", "build-manifest", "--source-dir"])
        .arg(&pkg)
        .args(["--package", "mypkg", "--python", "/usr/bin/python3"])
        .output()
        .unwrap();
    assert!(!output.status.success(), "--python should be rejected by clap");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected argument") || stderr.contains("--python"),
        "expected unknown-arg error; got: {stderr}"
    );
}
