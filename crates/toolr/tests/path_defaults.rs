//! A relative path default resolves from the repo root, through the real
//! binary, when `toolr` runs from a subdirectory.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

use tempfile::TempDir;

/// A project with `demo hello` and `demo envy` (bound to `DEMO_CFG`), both
/// taking `cfg: FilePath = <default>`, run from `sub/dir`. Its in-tree venv
/// Python is a stand-in that prints the spec `toolr` hands it, so a test
/// sees the parsed values without a real interpreter.
struct Project {
    tmp: TempDir,
}

impl Project {
    fn new(default: &str) -> Self {
        let tmp = TempDir::new().unwrap();
        let tools = common::write_project(tmp.path());
        fs::create_dir_all(tools.join(".venv").join("bin")).unwrap();
        fs::create_dir_all(tmp.path().join("sub").join("dir")).unwrap();
        fs::write(
            tools.join("demo.py"),
            format!(
                r#"from typing import Annotated

from toolr import Context, arg, command_group
from toolr.types import FilePath

group = command_group("demo", "Demo", "Demo.")

@group.command
def hello(ctx: Context, *, cfg: FilePath = "{default}") -> None:  # type: ignore[assignment]
    """Say hello."""

@group.command
def envy(
    ctx: Context,
    *,
    cfg: Annotated[FilePath, arg(env="DEMO_CFG")] = "{default}",  # type: ignore[assignment]
) -> None:
    """Read the config from the environment."""
"#
            ),
        )
        .unwrap();
        common::write_executable(
            &tools.join(".venv").join("bin").join("python"),
            "#!/bin/sh\ncat \"$TOOLR_SPEC_FILE\"\n",
        );
        Self { tmp }
    }

    fn root(&self) -> PathBuf {
        dunce::canonicalize(self.tmp.path()).unwrap()
    }

    fn subdir(&self) -> PathBuf {
        self.tmp.path().join("sub").join("dir")
    }

    /// Bounded: a wrongly accepted value would carry on into the runner.
    fn run(&self, args: &[&str], env: Option<(&str, &str)>) -> Output {
        let mut cmd = common::toolr();
        cmd.args(args)
            .current_dir(self.subdir())
            .env("TOOLR_NO_CACHE_HINT", "1")
            .env_remove("DEMO_CFG")
            .timeout(Duration::from_secs(60));
        if let Some((name, value)) = env {
            cmd.env(name, value);
        }
        cmd.output().unwrap()
    }
}

/// Rendered by clap, as a bad value on any other path arg is: one
/// `error:` line, no `toolr:` prefix.
fn assert_one_clap_line(stderr: &str) {
    assert_eq!(stderr.lines().count(), 1, "stderr:\n{stderr}");
}

#[cfg(unix)]
fn assert_cfg(output: &Output, expected: &Path) {
    let stdout = String::from_utf8_lossy(&output.stdout).replace([' ', '\n'], "");
    assert!(output.status.success(), "{output:?}");
    let needle = format!(r#""cfg":"{}""#, expected.display());
    assert!(stdout.contains(&needle), "want {needle} in spec:\n{stdout}");
}

#[test]
fn help_from_subdirectory_shows_relative_path_default() {
    let project = Project::new("tools/pyproject.toml");
    let output = project.run(&["demo", "hello", "--help"], None);
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
    let project = Project::new("tools/missing.toml");
    let output = project.run(&["demo", "hello"], None);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr:\n{stderr}");
    assert!(
        stderr.starts_with(
            "error: default 'tools/missing.toml' for '--cfg <cfg>' (resolved from the repo root "
        ),
        "stderr:\n{stderr}"
    );
    assert_one_clap_line(&stderr);
}

#[test]
fn typed_missing_value_from_subdirectory_fails_before_running() {
    let project = Project::new("tools/pyproject.toml");
    let output = project.run(&["demo", "hello", "--cfg", "nope.toml"], None);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr:\n{stderr}");
    assert!(
        stderr.starts_with(
            "error: invalid value 'nope.toml' for '--cfg <cfg>': path does not exist: nope.toml"
        ),
        "stderr:\n{stderr}"
    );
    assert_one_clap_line(&stderr);
}

#[cfg(unix)]
#[test]
fn typed_missing_value_in_untrusted_repo_is_reported_after_the_trust_prompt() {
    let project = Project::new("tools/pyproject.toml");
    let env = common::TrustEnv::new();
    let mut cmd = std::process::Command::new(assert_cmd::cargo::cargo_bin("toolr"));
    cmd.args(["demo", "hello", "--cfg", "nope.toml"])
        .current_dir(project.subdir())
        .env_remove("TOOLR_TRUST_ALL")
        .env("XDG_STATE_HOME", env.state.path())
        .env("XDG_CACHE_HOME", env.cache.path())
        .env("XDG_DATA_HOME", env.data.path())
        .env("TOOLR_NO_CACHE_HINT", "1");
    let output = common::run_on_pty(cmd, "y\n");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr:\n{stderr}");
    let prompt = stderr.find("Trust it?").expect(&stderr);
    let typo = stderr
        .find("path does not exist: nope.toml")
        .expect(&stderr);
    assert!(prompt < typo, "stderr:\n{stderr}");
}

#[cfg(unix)]
#[test]
fn valid_relative_default_from_subdirectory_passes_the_repo_root_path() {
    let project = Project::new("tools/pyproject.toml");
    let output = project.run(&["demo", "hello"], None);
    assert_cfg(
        &output,
        &project.root().join("tools").join("pyproject.toml"),
    );
}

#[cfg(unix)]
#[test]
fn bad_env_value_renders_like_clap_naming_the_variable() {
    let project = Project::new("tools/pyproject.toml");
    let output = project.run(&["demo", "envy"], Some(("DEMO_CFG", "nope.toml")));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr:\n{stderr}");
    assert!(
        stderr.starts_with(
            "error: invalid value 'nope.toml' for '--cfg <cfg>' (from DEMO_CFG): \
             path does not exist: nope.toml"
        ),
        "stderr:\n{stderr}"
    );
    assert_one_clap_line(&stderr);
}

#[cfg(unix)]
#[test]
fn env_value_from_subdirectory_resolves_against_cwd() {
    let project = Project::new("tools/pyproject.toml");
    fs::write(project.subdir().join("local.toml"), "").unwrap();
    let output = project.run(&["demo", "envy"], Some(("DEMO_CFG", "local.toml")));
    assert_cfg(
        &output,
        &project.root().join("sub").join("dir").join("local.toml"),
    );
}
