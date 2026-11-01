//! Project-root discovery through the binary: only a `tools/` with a
//! `pyproject.toml` makes a project, so an unrelated `tools/` never hides or
//! hijacks the repo's commands, and a bare one is reported.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use tempfile::TempDir;

const DEMO: &str = "\"\"\"Demo.\"\"\"\nfrom toolr import command_group\n\
     group = command_group(\"demo\", \"Demo commands\")\n\
     @group.command\ndef hello(ctx):\n    \"\"\"Say hello.\"\"\"\n";

/// `toolr` in `cwd` with a private cache.
fn toolr_in(cwd: &Path, cache: &TempDir) -> Command {
    let mut cmd = common::toolr();
    cmd.current_dir(cwd)
        .env("XDG_CACHE_HOME", cache.path())
        .env("TOOLR_NO_CACHE_HINT", "1");
    cmd
}

/// The reported layout: a toolr repo with a `demo` group, run from
/// `src/pkg`, which holds an unrelated `tools/` Python package.
fn repo_with_unrelated_tools_package() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let tools = common::write_project(tmp.path());
    fs::write(tools.join("demo.py"), DEMO).unwrap();
    let pkg = tmp.path().join("src").join("pkg");
    fs::create_dir_all(pkg.join("tools")).unwrap();
    fs::write(pkg.join("tools").join("__init__.py"), "").unwrap();
    (tmp, pkg)
}

fn manifest_in(dir: &Path) -> PathBuf {
    dir.join("tools").join(".toolr-manifest.json")
}

#[test]
fn user_group_found_below_unrelated_tools_dir() {
    let (_tmp, pkg) = repo_with_unrelated_tools_package();
    let cache = TempDir::new().unwrap();

    toolr_in(&pkg, &cache)
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("demo"));
}

#[test]
fn manifest_rebuild_below_unrelated_tools_dir_writes_root_manifest() {
    let (tmp, pkg) = repo_with_unrelated_tools_package();
    let cache = TempDir::new().unwrap();

    toolr_in(&pkg, &cache)
        .args(["project", "manifest", "rebuild"])
        .assert()
        .success()
        .stdout(contains(
            manifest_in(&tmp.path().canonicalize().unwrap())
                .display()
                .to_string(),
        ));

    assert!(
        !manifest_in(&pkg).exists(),
        "manifest written into the unrelated src/pkg/tools/"
    );
}

/// `tools/__init__.py` with `tools/pyproject.toml` is a working toolr layout;
/// a bare `tools/` above it, like `C:\tools` on Windows runners, must not
/// hide its commands.
#[test]
fn project_below_bare_tools_keeps_its_commands() {
    let outer = TempDir::new().unwrap();
    fs::create_dir_all(outer.path().join("tools")).unwrap();
    let proj = outer.path().join("proj");
    let tools = common::write_project(&proj);
    fs::write(tools.join("__init__.py"), "").unwrap();
    fs::write(tools.join("demo.py"), DEMO).unwrap();
    let cache = TempDir::new().unwrap();

    toolr_in(&proj, &cache)
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("demo"));
}

/// A repo with commands in `tools/` but no `tools/pyproject.toml`.
fn bare_tools_repo() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir_all(&tools).unwrap();
    fs::write(tools.join("demo.py"), DEMO).unwrap();
    let tools = tools.canonicalize().unwrap();
    (tmp, tools)
}

fn assert_names_missing_pyproject(stderr: &str, tools: &Path) {
    for needle in [
        &tools.display().to_string(),
        "has no `pyproject.toml`",
        "toolr needs `tools/pyproject.toml`",
        "toolr project init",
    ] {
        assert!(stderr.contains(needle), "missing {needle:?}:\n{stderr}");
    }
}

#[test]
fn user_command_in_bare_tools_repo_says_pyproject_is_missing() {
    let (tmp, tools) = bare_tools_repo();
    let cache = TempDir::new().unwrap();

    let output = toolr_in(tmp.path(), &cache)
        .args(["demo", "hello"])
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert_names_missing_pyproject(&String::from_utf8_lossy(&output.stderr), &tools);
}

/// Plain `--help` stays quiet: an unrelated bare `tools/` may sit above
/// any directory toolr runs in.
#[test]
fn help_in_bare_tools_repo_is_quiet() {
    let (tmp, _tools) = bare_tools_repo();
    let cache = TempDir::new().unwrap();

    toolr_in(tmp.path(), &cache)
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("demo").not())
        .stderr("");
    assert!(!manifest_in(tmp.path()).exists());
}

#[test]
fn trust_in_bare_tools_repo_says_pyproject_is_missing() {
    let (tmp, tools) = bare_tools_repo();
    let env = common::TrustEnv::new();

    let output = env
        .toolr(tmp.path())
        .args(["project", "trust"])
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert_names_missing_pyproject(&String::from_utf8_lossy(&output.stderr), &tools);
}
