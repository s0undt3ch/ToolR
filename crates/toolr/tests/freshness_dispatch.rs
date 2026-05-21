//! Integration tests for dispatch-time manifest freshness.

use std::fs;
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::TempDir;

const EXAMPLE_PY: &str = r#"
from toolr import Context, command_group

example = command_group("example", "Example commands")

@example.command
def hello(ctx: Context, name: str = "world") -> None:
    """Greet someone."""
    ctx.print(f"hello, {name}")
"#;

/// Build a minimal toolr project at `tmp`: a `tools/pyproject.toml`
/// plus an intentionally-stale `tools/.toolr-manifest.json`. Tests
/// then drop additional files into `tools/` and verify the freshness
/// step picks them up.
fn write_minimal_project(tmp: &std::path::Path) {
    let tools = tmp.join("tools");
    fs::create_dir_all(&tools).unwrap();
    fs::write(
        tools.join("pyproject.toml"),
        r#"[project]
name = "tools"
version = "0.0.0"
"#,
    )
    .unwrap();
    // Seed an empty, intentionally-stale manifest so `ensure_manifest_present_or_bootstrap`
    // doesn't try to bootstrap via Python — we want to exercise the
    // freshness path, not the missing-manifest path.
    fs::write(
        tools.join(".toolr-manifest.json"),
        r#"{
            "schema_version": 1,
            "static_hash": "stale",
            "third_party_hash": "",
            "groups": [],
            "commands": []
        }"#,
    )
    .unwrap();
}

#[test]
fn new_tools_file_appears_in_help_without_explicit_rebuild() {
    let tmp = TempDir::new().unwrap();
    write_minimal_project(tmp.path());

    // Drop a new `example.py` in tools/ after the manifest was seeded.
    fs::write(tmp.path().join("tools").join("example.py"), EXAMPLE_PY).unwrap();

    let output = Command::cargo_bin("toolr")
        .unwrap()
        .arg("--help")
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "toolr --help failed: {output:?}");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("example"),
        "expected `example` in --help, got:\n{stdout}"
    );

    // Manifest on disk should have been rewritten to include the group.
    let manifest =
        fs::read_to_string(tmp.path().join("tools").join(".toolr-manifest.json")).unwrap();
    assert!(
        manifest.contains(r#""name": "example""#) || manifest.contains(r#""name":"example""#),
        "manifest was not persisted with the example group:\n{manifest}"
    );
}

#[test]
fn syntax_error_in_tools_warns_and_serves_cached() {
    let tmp = TempDir::new().unwrap();
    write_minimal_project(tmp.path());

    // Overwrite the empty manifest seeded by `write_minimal_project`
    // with one that has a pre-existing `good` group — proves the
    // soft-fail path falls back to (not erases) the cached manifest.
    fs::write(
        tmp.path().join("tools").join(".toolr-manifest.json"),
        r#"{
            "schema_version": 1,
            "static_hash": "stale",
            "third_party_hash": "",
            "groups": [
                {"name": "good", "title": "Good", "description": "", "parent": null, "origin": "static"}
            ],
            "commands": []
        }"#,
    )
    .unwrap();

    // Drop a syntactically broken Python file so the static rebuild
    // returns BuildError::Build (unclosed parenthesis = parse error).
    fs::write(
        tmp.path().join("tools").join("broken.py"),
        "def not closed(",
    )
    .unwrap();

    let output = Command::cargo_bin("toolr")
        .unwrap()
        .arg("--help")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    // toolr --help itself must succeed — we're soft-failing.
    assert!(
        output.status.success(),
        "toolr --help failed unexpectedly: {output:?}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("tools manifest is stale and a fresh build failed"),
        "expected soft-fail warning in stderr; got:\n{stderr}"
    );
    assert!(
        stderr.contains("broken.py"),
        "expected the offending filename in the warning; got:\n{stderr}"
    );
    assert!(
        stderr.contains("toolr project manifest rebuild"),
        "expected pointer to explicit rebuild command; got:\n{stderr}"
    );

    // Cached `good` group must still be visible — we fell back, didn't erase.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("good"),
        "expected cached group in --help; got:\n{stdout}"
    );
}
