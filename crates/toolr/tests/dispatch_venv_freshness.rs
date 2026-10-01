//! Dispatch must not run a tools venv that is out of date with
//! `tools/uv.lock` (#527): a stale venv is re-synced first, and when that
//! can't happen the command is refused rather than run on old code.
#![cfg(unix)]

use std::fs;
use std::path::Path;

use assert_cmd::Command;

#[path = "common/mod.rs"]
mod common;
use common::VenvFixture;

const HELLO_TOOLS: &str = "\"\"\"Hi.\"\"\"\nfrom toolr import command_group\ngroup = command_group(\"hello\", \"Hi\")\n@group.command\ndef world(ctx):\n    \"\"\"World.\"\"\"\n";

struct Project {
    fx: VenvFixture,
    sentinel: std::path::PathBuf,
    cache: std::path::PathBuf,
}

impl Project {
    fn toolr(&self, path: &str) -> Command {
        let mut cmd = Command::cargo_bin("toolr").unwrap();
        cmd.current_dir(&self.fx.root)
            .env("PATH", path)
            .env("XDG_CACHE_HOME", &self.cache)
            .env_remove("TOOLR_AUTO_INSTALL_UV")
            .env_remove("TOOLR_VENV_LOCATION");
        cmd
    }

    fn path_with_uv(&self) -> String {
        format!("{}:/usr/bin:/bin", self.fx.bin_dir.display())
    }

    fn reset_observations(&self) {
        let _ = fs::remove_file(&self.sentinel);
        let _ = fs::remove_file(&self.fx.uv_argv_log);
    }

    /// Rewrite `tools/uv.lock` so it is newer than the sync stamp, the way a
    /// `git pull` bumping a dependency would.
    fn bump_lock(&self) {
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(self.fx.tools_dir.join("uv.lock"), "version = 1\n# bumped\n").unwrap();
    }
}

/// Factory: a project whose tools venv was provisioned by `toolr project
/// venv sync` (stub uv), with a `hello world` command whose interpreter
/// drops a sentinel when it runs.
fn synced_project() -> Project {
    let fx = VenvFixture::new();
    let sentinel = fx.root.join("ran");
    let cache = fx.root.join("xdg-cache");
    fs::write(
        fx.venv_dir.join("bin").join("python"),
        format!("#!/bin/sh\necho ran > {}\nexit 0\n", sentinel.display()),
    )
    .unwrap();
    fs::write(fx.tools_dir.join("hello.py"), HELLO_TOOLS).unwrap();
    let project = Project {
        fx,
        sentinel,
        cache,
    };
    project
        .toolr(&project.path_with_uv())
        .args(["project", "venv", "sync"])
        .assert()
        .success();
    project.reset_observations();
    project
}

fn uv_synced(log: &str) -> bool {
    log.lines().any(|token| token == "sync")
}

fn stamp_is_newer_than_lock(venv: &Path, tools: &Path) -> bool {
    let stamp = fs::metadata(venv.join(".toolr-sync-stamp"))
        .unwrap()
        .modified()
        .unwrap();
    let lock = fs::metadata(tools.join("uv.lock"))
        .unwrap()
        .modified()
        .unwrap();
    stamp >= lock
}

#[test]
fn fresh_venv_dispatches_without_running_uv() {
    let p = synced_project();
    p.toolr(&p.path_with_uv())
        .args(["hello", "world"])
        .assert()
        .success();
    assert!(p.sentinel.exists(), "the command should have run");
    assert_eq!(p.fx.uv_argv(), "", "a fresh venv must not invoke uv");
}

#[test]
fn stale_venv_is_synced_before_dispatch() {
    let p = synced_project();
    p.bump_lock();
    p.toolr(&p.path_with_uv())
        .args(["hello", "world"])
        .assert()
        .success();
    let log = p.fx.uv_argv();
    assert!(
        uv_synced(&log),
        "expected a `uv sync` before dispatch, uv argv was:\n{log}"
    );
    assert!(stamp_is_newer_than_lock(&p.fx.venv_dir, &p.fx.tools_dir));
    assert!(p.sentinel.exists(), "the command should run after the sync");
}

#[test]
fn stale_venv_without_uv_refuses_to_run() {
    let p = synced_project();
    p.bump_lock();
    let output = p
        .toolr("/usr/bin:/bin")
        .args(["--quiet", "hello", "world"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "a stale venv must not run; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("out of date with tools/uv.lock"),
        "stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("toolr project venv sync"),
        "stderr:\n{stderr}"
    );
    assert!(
        !p.sentinel.exists(),
        "the stale interpreter must not have run"
    );
}
