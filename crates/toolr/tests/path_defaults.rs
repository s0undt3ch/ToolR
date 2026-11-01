//! A relative path default resolves from the repo root, through the real
//! binary, when `toolr` runs from a subdirectory.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
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
fn typed_missing_value_from_subdirectory_fails_before_running() {
    let tmp = project_with_default("tools/pyproject.toml");
    let output = toolr_from_subdir(&tmp, &["demo", "hello", "--cfg", "nope.toml"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr:\n{stderr}");
    assert!(
        stderr.contains("path does not exist: nope.toml"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn typed_missing_value_in_untrusted_repo_reports_the_typo_first() {
    let tmp = project_with_default("tools/pyproject.toml");
    let env = common::TrustEnv::new();
    let output = env
        .toolr(&subdir(&tmp))
        .args(["demo", "hello", "--cfg", "nope.toml"])
        .timeout(Duration::from_secs(60))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr:\n{stderr}");
    assert!(
        stderr.contains("path does not exist: nope.toml"),
        "stderr:\n{stderr}"
    );
    assert!(!stderr.contains("trust"), "stderr:\n{stderr}");
}

/// A project whose commands run under a stand-in Python that prints the
/// spec `toolr` hands it. The tools pyproject is removed once the manifest
/// is built, so dispatch uses `TOOLR_PYTHON` instead of syncing a venv.
#[cfg(unix)]
struct EchoProject {
    tmp: TempDir,
    python: PathBuf,
}

#[cfg(unix)]
impl EchoProject {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        fs::create_dir_all(&tools).unwrap();
        fs::create_dir_all(tmp.path().join("sub").join("dir")).unwrap();
        fs::write(
            tools.join("pyproject.toml"),
            "[project]\nname = \"tools\"\nversion = \"0.0.0\"\n",
        )
        .unwrap();
        fs::write(tools.join("settings.toml"), "").unwrap();
        fs::write(
            tools.join("demo.py"),
            r#"from typing import Annotated

from toolr import Context, arg, command_group
from toolr.types import FilePath

group = command_group("demo", "Demo")

@group.command
def hello(ctx: Context, *, cfg: FilePath = "tools/settings.toml") -> None:  # type: ignore[assignment]
    """Say hello."""

@group.command
def envy(
    ctx: Context,
    *,
    cfg: Annotated[FilePath, arg(env="DEMO_CFG")] = "tools/settings.toml",  # type: ignore[assignment]
) -> None:
    """Read the config from the environment."""
"#,
        )
        .unwrap();
        let rebuild = common::toolr()
            .args(["project", "manifest", "rebuild"])
            .current_dir(tmp.path())
            .env("TOOLR_NO_CACHE_HINT", "1")
            .timeout(Duration::from_secs(60))
            .output()
            .unwrap();
        assert!(rebuild.status.success(), "rebuild failed: {rebuild:?}");
        fs::remove_file(tools.join("pyproject.toml")).unwrap();
        let python = tmp.path().join("python");
        fs::write(&python, "#!/bin/sh\ncat \"$TOOLR_SPEC_FILE\"\n").unwrap();
        fs::set_permissions(&python, fs::Permissions::from_mode(0o755)).unwrap();
        Self { tmp, python }
    }

    fn root(&self) -> PathBuf {
        dunce::canonicalize(self.tmp.path()).unwrap()
    }

    fn run(&self, args: &[&str], env: Option<(&str, &str)>) -> Output {
        let mut cmd = common::toolr();
        cmd.args(args)
            .current_dir(subdir(&self.tmp))
            .env("TOOLR_PYTHON", &self.python)
            .env("TOOLR_NO_CACHE_HINT", "1")
            .env_remove("DEMO_CFG")
            .timeout(Duration::from_secs(60));
        if let Some((name, value)) = env {
            cmd.env(name, value);
        }
        cmd.output().unwrap()
    }
}

#[cfg(unix)]
fn assert_cfg(output: &Output, expected: &Path) {
    let stdout = String::from_utf8_lossy(&output.stdout).replace([' ', '\n'], "");
    assert!(output.status.success(), "{output:?}");
    let needle = format!(r#""cfg":"{}""#, expected.display());
    assert!(stdout.contains(&needle), "want {needle} in spec:\n{stdout}");
}

#[cfg(unix)]
#[test]
fn valid_relative_default_from_subdirectory_passes_the_repo_root_path() {
    let project = EchoProject::new();
    let output = project.run(&["demo", "hello"], None);
    assert_cfg(&output, &project.root().join("tools").join("settings.toml"));
}

#[cfg(unix)]
#[test]
fn env_value_from_subdirectory_resolves_against_cwd() {
    let project = EchoProject::new();
    fs::write(subdir(&project.tmp).join("local.toml"), "").unwrap();
    let output = project.run(&["demo", "envy"], Some(("DEMO_CFG", "local.toml")));
    assert_cfg(
        &output,
        &project.root().join("sub").join("dir").join("local.toml"),
    );
}
