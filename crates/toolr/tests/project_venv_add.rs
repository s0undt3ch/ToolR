//! Integration tests for `toolr project venv add`. These don't run real
//! uv — they cover the clap surface and the help/usage paths.

use assert_cmd::Command;

#[path = "common/mod.rs"]
mod common;
use common::VenvFixture;

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

#[cfg(unix)]
#[test]
fn add_success_path_invokes_uv_add() {
    let fx = VenvFixture::new();
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .env("PATH", &fx.bin_dir)
        .current_dir(&fx.root)
        .args(["project", "venv", "add", "httpx"])
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    let argv = fx.uv_argv();
    assert!(argv.lines().any(|l| l == "add"), "uv argv should contain `add`; got:\n{argv}");
    assert!(argv.contains("httpx"), "uv argv should contain `httpx`; got:\n{argv}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("added"), "expected `added` in stdout, got: {stdout}");
}
