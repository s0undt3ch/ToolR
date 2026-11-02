//! A `uv` inside the repository never runs through an empty or relative
//! `PATH` entry, which would resolve it against the cwd.

#![cfg(unix)]

mod common;

use std::fs;
use std::process::Output;

use common::VenvFixture;
use tempfile::TempDir;

/// A project whose only `uv`s sit in the repo (`./uv` and `./rel-bin/uv`),
/// plus an empty data dir so no managed uv exists either.
fn planted_repo() -> (VenvFixture, TempDir) {
    let fx = VenvFixture::new();
    let stub = fx.bin_dir.join("uv");
    let body = fs::read_to_string(&stub).unwrap();
    common::write_executable(&fx.root.join("uv"), &body);
    fs::create_dir(fx.root.join("rel-bin")).unwrap();
    common::write_executable(&fx.root.join("rel-bin").join("uv"), &body);
    fs::remove_file(&stub).unwrap();
    (fx, TempDir::new().unwrap())
}

fn run(fx: &VenvFixture, data: &TempDir, path: &str) -> Output {
    common::toolr()
        .current_dir(&fx.root)
        .env("PATH", path)
        .env("XDG_DATA_HOME", data.path())
        .env_remove("TOOLR_AUTO_INSTALL_UV")
        .args(["project", "venv", "lock", "--quiet"])
        .output()
        .unwrap()
}

fn assert_repo_uv_never_ran(path: &str) {
    let (fx, data) = planted_repo();
    let out = run(&fx, &data, path);
    assert_eq!(fx.uv_argv(), "", "the repo's uv ran with PATH={path:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "expected failure:\n{stderr}");
    assert!(
        stderr.contains("TOOLR_AUTO_INSTALL_UV=1"),
        "missing the no-uv hint:\n{stderr}"
    );
}

#[test]
fn empty_path_entry_does_not_run_repo_uv() {
    assert_repo_uv_never_ran(":/usr/bin:/bin");
}

#[test]
fn trailing_colon_does_not_run_repo_uv() {
    assert_repo_uv_never_ran("/usr/bin:/bin:");
}

#[test]
fn dot_path_entry_does_not_run_repo_uv() {
    assert_repo_uv_never_ran(".:/usr/bin:/bin");
}

#[test]
fn relative_path_entry_does_not_run_repo_uv() {
    assert_repo_uv_never_ran("rel-bin:/usr/bin:/bin");
}
