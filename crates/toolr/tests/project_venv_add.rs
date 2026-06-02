//! Integration tests for `toolr project venv add`. These don't run real
//! uv — they cover the clap surface and the help/usage paths.

use assert_cmd::Command;

fn cargo_bin() -> Command {
    Command::cargo_bin("toolr").unwrap()
}

#[test]
fn add_help_lists_package_positional() {
    let output = cargo_bin()
        .args(["project", "venv", "add", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success(), "help should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PACKAGE"), "help missing PACKAGE: {stdout}");
    assert!(stdout.contains("uv add"), "help should reference `uv add`: {stdout}");
}

#[test]
fn add_requires_at_least_one_package() {
    let output = cargo_bin()
        .args(["project", "venv", "add"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("required") || stderr.contains("PACKAGE") || stderr.contains("usage"),
        "expected clap usage error, stderr was:\n{stderr}"
    );
}
