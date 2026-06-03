use assert_cmd::Command;
use tempfile::TempDir;

/// Build a tmpdir containing a tools/ directory with one ci.py file and
/// a freshly-built manifest committed alongside it. This mirrors the
/// happy path: the cached manifest's static_hash matches the live tree.
fn fixture() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    std::fs::create_dir(&tools).unwrap();
    std::fs::write(
        tools.join("ci.py"),
        r#""""CI utilities."""
from typing import Literal

group = command_group("ci", "CI utilities", docstring=__doc__)

@group.command
def hello(ctx, name="world"):
    """Say hello.

    Args:
        name: Who to greet.
    """
    pass

@group.command
def deploy(ctx, env: Literal["staging", "production"]):
    """Deploy something."""
    pass
"#,
    )
    .unwrap();

    // Build the manifest in-process so the static_hash matches.
    Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .arg("__build-static-manifest")
        .assert()
        .success();

    tmp
}

fn complete(tmp: &TempDir, args: &[&str]) -> String {
    let cwd = tmp.path().to_path_buf();
    let mut full: Vec<String> = vec!["__complete".into(), cwd.to_string_lossy().to_string()];
    for a in args {
        full.push((*a).to_string());
    }
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(&full)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "expected __complete to succeed, got status {:?}, stderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn completes_groups_at_top_level() {
    let tmp = fixture();
    let stdout = complete(&tmp, &[""]);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(lines.contains(&"ci"), "missing ci in {stdout}");
}

#[test]
fn completes_commands_under_a_group() {
    let tmp = fixture();
    let stdout = complete(&tmp, &["ci", ""]);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(lines.contains(&"hello"), "missing hello in {stdout}");
    assert!(lines.contains(&"deploy"), "missing deploy in {stdout}");
}

#[test]
fn completes_command_prefixes() {
    let tmp = fixture();
    let stdout = complete(&tmp, &["ci", "h"]);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines, vec!["hello"]);
}

#[test]
fn completes_literal_flag_values() {
    let tmp = fixture();
    let stdout = complete(&tmp, &["ci", "deploy", "--env", ""]);
    let mut lines: Vec<&str> = stdout.lines().collect();
    lines.sort();
    assert_eq!(lines, vec!["production", "staging"]);
}

#[test]
fn returns_no_completions_for_unknown_group() {
    let tmp = fixture();
    let stdout = complete(&tmp, &["unknown", ""]);
    assert!(stdout.trim().is_empty());
}

#[test]
fn reparses_when_tools_change_after_manifest_was_written() {
    let tmp = fixture();
    // Add a new command after the cached manifest was built.
    std::fs::write(
        tmp.path().join("tools/extra.py"),
        r#"group = command_group("extra", "Extra utilities")

@group.command
def shiny(ctx):
    """Shiny new command."""
    pass
"#,
    )
    .unwrap();

    let stdout = complete(&tmp, &[""]);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.contains(&"extra"),
        "missing freshly-added group in {stdout}"
    );
}

#[test]
fn completes_builtins_when_no_tools_dir_anywhere() {
    let tmp = TempDir::new().unwrap();
    let cwd = tmp.path().to_path_buf();
    // GHA Windows runners ship with `C:\tools\` populated, so the
    // ancestor walk succeeds when it crawls past the drive root.
    // Same hazard on Unix hosts with `/tools`. Skip when the host
    // violates the test precondition (no tools/ anywhere up the
    // chain from the tempdir).
    let mut walker = cwd.canonicalize().unwrap_or_else(|_| cwd.clone());
    let ancestor_has_tools = loop {
        if walker.join("tools").is_dir() {
            break true;
        }
        if !walker.pop() {
            break false;
        }
    };
    if ancestor_has_tools {
        eprintln!(
            "skipping: an ancestor of {} has a tools/ dir; \
             this host violates the test precondition.",
            cwd.display(),
        );
        return;
    }
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(&cwd)
        .args(["__complete", &cwd.to_string_lossy(), ""])
        .output()
        .unwrap();
    // Built-in `self` / `project` subtree doesn't depend on a project
    // root, so completion must still offer them outside any toolr project.
    // Only user-defined commands need a tools/ ancestor.
    assert!(
        output.status.success(),
        "expected success, got {:?}, stderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    for expected in ["self", "project"] {
        assert!(
            lines.contains(&expected),
            "missing built-in {expected} outside a toolr project; got: {stdout}",
        );
    }
    assert!(output.stderr.is_empty(), "expected silent stderr");
}

#[test]
fn self_completion_print_emits_bash_script() {
    let tmp = TempDir::new().unwrap();
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["self", "completion", "print", "bash"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("toolr __complete"));
    assert!(stdout.contains("complete -F _toolr_complete toolr"));
}
