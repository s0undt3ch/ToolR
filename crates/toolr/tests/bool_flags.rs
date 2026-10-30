//! End-to-end checks for the boolean flag rules, through the real binary.

use std::fs;
use std::path::{Path, PathBuf};
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

/// Stands in for the Python runner: prints the spec `toolr` hands it, so
/// the test sees the parsed values without needing a real interpreter.
#[cfg(unix)]
fn spec_echo_python(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let python = dir.join("python");
    fs::write(&python, "#!/bin/sh\ncat \"$TOOLR_SPEC_FILE\"\n").unwrap();
    fs::set_permissions(&python, fs::Permissions::from_mode(0o755)).unwrap();
    python
}

fn run_with_env(root: &Path, python: &Path, verbose_env: Option<&str>, args: &[&str]) -> Output {
    let mut cmd = Command::cargo_bin("toolr").unwrap();
    cmd.args(args)
        .current_dir(root)
        .env("TOOLR_PYTHON", python)
        .env_remove("V");
    if let Some(value) = verbose_env {
        cmd.env("V", value);
    }
    cmd.output().unwrap()
}

#[cfg(unix)]
#[test]
fn bool_alias_flag_reads_its_env_var() {
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
    let rebuild = toolr(&tmp, &["project", "manifest", "rebuild"]);
    assert!(rebuild.status.success(), "rebuild failed: {rebuild:?}");
    // Without a tools pyproject the dispatcher runs the built manifest
    // under TOOLR_PYTHON instead of syncing a tools venv.
    fs::remove_file(tmp.path().join("tools").join("pyproject.toml")).unwrap();
    let python = spec_echo_python(tmp.path());

    let cases: &[(Option<&str>, &[&str], &str)] = &[
        (None, &[], r#""verbose":false"#),
        (None, &["--verbose"], r#""verbose":true"#),
        (Some("true"), &[], r#""verbose":true"#),
        (Some("false"), &[], r#""verbose":false"#),
        (Some("false"), &["--verbose"], r#""verbose":true"#),
    ];
    for (env, extra, expected) in cases {
        let args: Vec<&str> = ["example", "run"]
            .iter()
            .chain(extra.iter())
            .copied()
            .collect();
        let output = run_with_env(tmp.path(), &python, *env, &args);
        let stdout = String::from_utf8_lossy(&output.stdout).replace([' ', '\n'], "");
        assert!(output.status.success(), "V={env:?} {extra:?}: {output:?}");
        assert!(
            stdout.contains(expected),
            "V={env:?} {extra:?}: spec:\n{stdout}"
        );
    }

    let output = run_with_env(tmp.path(), &python, Some("1"), &["example", "run"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr:\n{stderr}");
    assert!(
        stderr.contains("[possible values: true, false]"),
        "stderr:\n{stderr}"
    );
}
