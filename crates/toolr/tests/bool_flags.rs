//! End-to-end checks for the boolean flag rules, through the real binary.

use std::fs;
use std::process::{Command, Output};

use assert_cmd::prelude::*;
use tempfile::TempDir;

fn project_with(module_source: &str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir_all(&tools).unwrap();
    fs::write(
        tools.join("pyproject.toml"),
        "[project]\nname = \"tools\"\nversion = \"0.0.0\"\n",
    )
    .unwrap();
    fs::write(tools.join("example.py"), module_source).unwrap();
    tmp
}

fn toolr(tmp: &TempDir, args: &[&str]) -> Output {
    Command::cargo_bin("toolr")
        .unwrap()
        .args(args)
        .current_dir(tmp.path())
        .output()
        .unwrap()
}

#[test]
fn bool_true_default_fails_help_with_rewrite() {
    let tmp = project_with(
        r#"from toolr import Context, command_group

group = command_group("example", "Example")

@group.command
def run(ctx: Context, *, colour: bool = True) -> None:
    """Run."""
"#,
    );
    let output = toolr(&tmp, &["--help"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "expected failure: {output:?}");
    assert!(
        stderr.contains("invalid parameter declarations"),
        "stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("`no_colour: bool = False`"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn no_prefixed_flag_shows_in_help() {
    let tmp = project_with(
        r#"from toolr import Context, command_group

group = command_group("example", "Example")

@group.command
def run(ctx: Context, *, no_colour: bool = False) -> None:
    """Run."""
"#,
    );
    let output = toolr(&tmp, &["example", "run", "--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "help failed: {output:?}");
    assert!(stdout.contains("--no-colour"), "stdout:\n{stdout}");
}

#[test]
fn bool_alias_is_a_flag_in_help() {
    let tmp = project_with(
        r#"from typing import Annotated

from toolr import Context, arg, command_group

group = command_group("example", "Example")

Verbose = Annotated[bool, arg(env="V")]

@group.command
def run(ctx: Context, *, verbose: Verbose = False) -> None:
    """Run."""
"#,
    );
    let output = toolr(&tmp, &["example", "run", "--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "help failed: {output:?}");
    assert!(stdout.contains("--verbose"), "stdout:\n{stdout}");
    assert!(!stdout.contains("<VERBOSE>"), "stdout:\n{stdout}");
}
