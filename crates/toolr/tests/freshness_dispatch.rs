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

#[test]
fn skip_list_argv_does_not_trigger_freshness() {
    let tmp = TempDir::new().unwrap();
    write_minimal_project(tmp.path());

    // Drop a syntax-broken `tools/*.py` that would crash a rebuild —
    // but skip-list argv must never call into freshness, so the
    // command should still succeed without any warning.
    fs::write(
        tmp.path().join("tools").join("broken.py"),
        "def not closed(",
    )
    .unwrap();

    for argv in [
        vec!["--version"],
        vec!["self", "cache", "list"],
        vec!["project", "manifest", "--help"],
    ] {
        let output = Command::cargo_bin("toolr")
            .unwrap()
            .args(&argv)
            .current_dir(tmp.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "argv {argv:?} should bypass freshness, got: {output:?}"
        );
        // The soft-fail warning must NOT appear; freshness was bypassed entirely.
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("tools manifest is stale"),
            "unexpected freshness warning for argv {argv:?}:\n{stderr}"
        );
    }
}

#[test]
fn version_mismatch_without_venv_preserves_third_party_entries() {
    // #420 regression guard: a toolr-version mismatch must not escalate
    // to `ThirdPartyDrift` when the venv can't be resolved — that would
    // discard cached third-party entries with nothing to re-glob them
    // from. Force venv resolution to fail via an invalid
    // `TOOLR_VENV_LOCATION`, seed a manifest with a matching static_hash
    // but a stale `toolr_version` and a third-party group/command, and
    // confirm the rebuild keeps the third-party entries and still
    // stamps the current version.
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir_all(&tools).unwrap();
    fs::write(
        tools.join("pyproject.toml"),
        "[project]\nname = \"tools\"\nversion = \"0.0.0\"\n",
    )
    .unwrap();

    let static_hash = toolr_core::hash::hash_tools_dir(&tools).unwrap();
    let manifest = format!(
        r#"{{
            "schema_version": 1,
            "static_hash": "{static_hash}",
            "third_party_hash": "",
            "toolr_version": "0.0.0-old",
            "groups": [
                {{"name": "plugin", "title": "Plugin", "description": "", "parent": null, "origin": "third_party"}}
            ],
            "commands": [
                {{
                    "name": "run", "group": "plugin", "module": "plugin_pkg.commands",
                    "function": "run", "summary": "", "description": "",
                    "arguments": [], "origin": "third_party"
                }}
            ]
        }}"#
    );
    fs::write(tools.join(".toolr-manifest.json"), manifest).unwrap();

    let output = Command::cargo_bin("toolr")
        .unwrap()
        .arg("--help")
        .current_dir(tmp.path())
        .env("TOOLR_VENV_LOCATION", "bogus")
        .output()
        .unwrap();
    assert!(output.status.success(), "toolr --help failed: {output:?}");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("plugin"),
        "expected cached third-party group in --help; got:\n{stdout}"
    );

    let manifest = fs::read_to_string(tools.join(".toolr-manifest.json")).unwrap();
    assert!(
        manifest.contains(r#""name": "run""#) || manifest.contains(r#""name":"run""#),
        "third-party command dropped from persisted manifest:\n{manifest}"
    );
    assert!(
        !manifest.contains("0.0.0-old"),
        "stale toolr_version was not refreshed:\n{manifest}"
    );
}

#[test]
fn tab_completion_does_not_persist_manifest() {
    let tmp = TempDir::new().unwrap();
    write_minimal_project(tmp.path());
    // Force StaticDrift so the freshness machinery would rebuild if not
    // bypassed: the tools/*.py content differs from the seeded
    // static_hash, but tab completion handles drift in-memory only.
    fs::write(tmp.path().join("tools").join("a.py"), "x = 1\n").unwrap();

    let manifest_path = tmp.path().join("tools").join(".toolr-manifest.json");
    let before = fs::read_to_string(&manifest_path).unwrap();
    let mtime_before = fs::metadata(&manifest_path).unwrap().modified().unwrap();

    // Trigger a completion call. The exact tokens after `__complete` don't
    // matter — completion's only job here is to fire the freshness path
    // via `resolve_manifest_at_tab`, which is responsible for staying in-memory.
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .args(["__complete", tmp.path().to_str().unwrap(), "toolr", ""])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "__complete failed: {output:?}");

    // Manifest file must not have been rewritten.
    let after = fs::read_to_string(&manifest_path).unwrap();
    let mtime_after = fs::metadata(&manifest_path).unwrap().modified().unwrap();
    assert_eq!(before, after, "tab completion rewrote the manifest contents");
    assert_eq!(mtime_before, mtime_after, "tab completion touched mtime");
}

/// GH #454, end-to-end through the real binary: a command imports an
/// `Enum` from a sibling module (relative import) while an unrelated
/// module elsewhere in the tree declares a same-named enum class. The
/// auto-rebuild triggered by `toolr --help` must succeed (not hard-fail
/// with "unsupported parameter types") and the persisted manifest must
/// carry the resolved enum's allowed values.
#[test]
fn cross_module_enum_import_rebuilds_and_persists_allowed_values() {
    let tmp = TempDir::new().unwrap();
    write_minimal_project(tmp.path());

    let tools = tmp.path().join("tools");
    fs::create_dir_all(tools.join("metrics")).unwrap();
    fs::write(tools.join("metrics").join("__init__.py"), "").unwrap();
    fs::write(
        tools.join("metrics").join("_common.py"),
        r#""""Shared metrics types."""
import enum

class Environment(enum.StrEnum):
    PRODUCTION = "production"
    STAGING = "staging"
"#,
    )
    .unwrap();
    fs::write(
        tools.join("metrics").join("analyse.py"),
        r#""""Metrics analysis -- imports Environment from a sibling module."""
from toolr import Context, command_group
from ._common import Environment

group = command_group("metrics", "Metrics")

@group.command
def analyse(ctx: Context, *, env: Environment = Environment.PRODUCTION) -> None:
    """Analyse."""
    ctx.print(env.value)
"#,
    )
    .unwrap();
    // Unrelated module declaring a same-named enum elsewhere in the
    // tree -- this is exactly the shape that hard-failed the build
    // before the fix (GH #454).
    fs::write(
        tools.join("job.py"),
        r#""""Job module -- unrelated same-named enum."""
import enum
from toolr import Context, command_group

group = command_group("job", "Job")

class Environment(enum.StrEnum):
    PRODUCTION = "production"
    STAGING = "staging"

@group.command
def run(ctx: Context, *, env: Environment = Environment.PRODUCTION) -> None:
    """Run."""
    ctx.print(env.value)
"#,
    )
    .unwrap();

    let output = Command::cargo_bin("toolr")
        .unwrap()
        .arg("--help")
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "toolr --help failed: {output:?}\nstderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unsupported parameter types"),
        "manifest build hard-failed on the cross-module enum import:\n{stderr}"
    );

    let manifest = fs::read_to_string(tools.join(".toolr-manifest.json")).unwrap();
    assert!(
        manifest.contains("production") && manifest.contains("staging"),
        "manifest missing the resolved enum's allowed values:\n{manifest}"
    );
    assert!(
        manifest.contains("tools.metrics._common"),
        "manifest missing the enum's declaring module (tools.metrics._common):\n{manifest}"
    );
}
