//! Untrusted-repository regression tests for SEC-01.
//! A repo must not be able to run code via toolr's read-only surfaces.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use assert_cmd::Command;
use tempfile::TempDir;

/// Build a malicious repo: in-tree venv-location, a committed fake
/// `tools/.venv/bin/python` that drops a sentinel when executed, and NO
/// `.toolr-manifest.json`.
fn malicious_repo(sentinel: &std::path::Path) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir_all(tools.join(".venv").join("bin")).unwrap();
    fs::write(
        tools.join("pyproject.toml"),
        "[project]\nname=\"evil\"\nversion=\"0\"\n\n[tool.toolr]\nvenv-location = \"in-tree\"\n",
    )
    .unwrap();
    fs::write(tools.join("hello.py"), "\"\"\"Hi.\"\"\"\n").unwrap();
    let py = tools.join(".venv").join("bin").join("python");
    fs::write(&py, format!("#!/bin/sh\necho pwned > {}\n", sentinel.display())).unwrap();
    fs::set_permissions(&py, fs::Permissions::from_mode(0o755)).unwrap();
    tmp
}

#[test]
#[cfg(unix)]
fn help_does_not_execute_committed_interpreter() {
    let out = TempDir::new().unwrap();
    let sentinel = out.path().join("sentinel");
    let repo = malicious_repo(&sentinel);

    Command::cargo_bin("toolr")
        .unwrap()
        .arg("--help")
        .current_dir(repo.path())
        .assert()
        .success();

    assert!(!sentinel.exists(), "toolr --help executed the committed interpreter");
}

#[test]
#[cfg(unix)]
fn bare_invocation_does_not_execute_committed_interpreter() {
    let out = TempDir::new().unwrap();
    let sentinel = out.path().join("sentinel");
    let repo = malicious_repo(&sentinel);

    // Bare `toolr` exits non-zero (no command) but must not run the interpreter.
    let _ = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(repo.path())
        .assert();

    assert!(!sentinel.exists(), "bare toolr executed the committed interpreter");
}
