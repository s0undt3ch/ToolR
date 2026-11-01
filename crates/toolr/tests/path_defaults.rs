//! A relative path default resolves from the repo root, through the real
//! binary, when `toolr` runs from a subdirectory.

mod common;

use std::fs;
use std::path::PathBuf;
use std::process::Output;
use std::time::Duration;

use tempfile::TempDir;

fn project_with_default(default: &str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir_all(&tools).unwrap();
    fs::create_dir_all(tmp.path().join("sub").join("dir")).unwrap();
    fs::write(
        tools.join("pyproject.toml"),
        "[project]\nname = \"tools\"\nversion = \"0.0.0\"\n",
    )
    .unwrap();
    fs::write(
        tools.join("demo.py"),
        format!(
            r#"from toolr import Context, command_group
from toolr.types import FilePath

group = command_group("demo", "Demo")

@group.command
def hello(ctx: Context, *, cfg: FilePath = "{default}") -> None:  # type: ignore[assignment]
    """Say hello."""
"#
        ),
    )
    .unwrap();
    tmp
}

fn subdir(tmp: &TempDir) -> PathBuf {
    tmp.path().join("sub").join("dir")
}

// Bounded: a wrongly accepted default would carry on into the venv sync.
fn toolr_from_subdir(tmp: &TempDir, args: &[&str]) -> Output {
    common::toolr()
        .args(args)
        .current_dir(subdir(tmp))
        .env("TOOLR_NO_CACHE_HINT", "1")
        .timeout(Duration::from_secs(60))
        .output()
        .unwrap()
}

#[test]
fn help_from_subdirectory_shows_relative_path_default() {
    let tmp = project_with_default("tools/pyproject.toml");
    let output = toolr_from_subdir(&tmp, &["demo", "hello", "--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "help failed: {output:?}");
    assert!(
        stdout.contains("[default: tools/pyproject.toml]"),
        "stdout:\n{stdout}"
    );
    assert!(!stderr.contains("path does not exist"), "stderr:\n{stderr}");
}

#[test]
fn missing_relative_default_errors_naming_repo_root() {
    let tmp = project_with_default("tools/missing.toml");
    let output = toolr_from_subdir(&tmp, &["demo", "hello"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr:\n{stderr}");
    assert!(
        stderr.contains("default 'tools/missing.toml'"),
        "stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("resolved from the repo root"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn typed_missing_value_from_subdirectory_still_fails_at_parse() {
    let tmp = project_with_default("tools/pyproject.toml");
    let output = toolr_from_subdir(&tmp, &["demo", "hello", "--cfg", "nope.toml"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr:\n{stderr}");
    assert!(
        stderr.contains("path does not exist: nope.toml"),
        "stderr:\n{stderr}"
    );
}
