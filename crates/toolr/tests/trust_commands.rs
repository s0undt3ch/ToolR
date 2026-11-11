//! `project trust`, `project untrust` and `self trusted` against a real
//! trust store, on every platform.

mod common;

use std::fs;

use predicates::str::contains;
use tempfile::TempDir;

use common::TrustEnv;

fn repo() -> TempDir {
    let repo = TempDir::new().unwrap();
    common::write_project(repo.path());
    repo
}

#[test]
fn trust_list_untrust_round_trip() {
    let env = TrustEnv::new();
    let repo = repo();
    let root = fs::canonicalize(repo.path()).unwrap().display().to_string();
    env.toolr(repo.path())
        .args(["project", "trust"])
        .assert()
        .success()
        .stdout(format!("{root}\n"));
    env.toolr(repo.path())
        .args(["self", "trusted"])
        .assert()
        .success()
        .stdout(format!("{root}\n"));
    env.toolr(repo.path())
        .args(["project", "untrust"])
        .assert()
        .success()
        .stdout(format!("{root}\n"));
    env.toolr(repo.path())
        .args(["self", "trusted"])
        .assert()
        .success()
        .stdout("");
}

#[test]
fn trust_accepts_a_path_argument() {
    let env = TrustEnv::new();
    let repo = repo();
    let elsewhere = TempDir::new().unwrap();
    let root = fs::canonicalize(repo.path()).unwrap().display().to_string();
    env.toolr(elsewhere.path())
        .args(["project", "trust"])
        .arg(repo.path())
        .assert()
        .success()
        .stdout(format!("{root}\n"));
}

#[test]
fn trust_confirms_on_stderr_and_says_when_already_trusted() {
    let env = TrustEnv::new();
    let repo = repo();
    let root = fs::canonicalize(repo.path()).unwrap().display().to_string();
    for expected in [
        format!("toolr: trusted {root}\n"),
        format!("toolr: {root} was already trusted\n"),
    ] {
        env.toolr(repo.path())
            .args(["project", "trust", "--no-sync"])
            .assert()
            .success()
            .stdout(format!("{root}\n"))
            .stderr(expected);
    }
}

#[test]
fn quiet_trust_and_untrust_print_only_the_root() {
    let env = TrustEnv::new();
    let repo = repo();
    let root = fs::canonicalize(repo.path()).unwrap().display().to_string();
    for args in [
        &["--quiet", "project", "trust", "--no-sync"][..],
        &["-q", "project", "trust", "--no-sync"][..],
        &["-q", "project", "untrust"][..],
    ] {
        env.toolr(repo.path())
            .args(args)
            .assert()
            .success()
            .stdout(format!("{root}\n"))
            .stderr("");
    }
}

#[test]
fn untrust_confirms_on_stderr() {
    let env = TrustEnv::new();
    let repo = repo();
    let root = fs::canonicalize(repo.path()).unwrap().display().to_string();
    env.toolr(repo.path())
        .args(["project", "trust", "--no-sync"])
        .assert()
        .success();
    env.toolr(repo.path())
        .args(["project", "untrust"])
        .assert()
        .success()
        .stdout(format!("{root}\n"))
        .stderr(format!("toolr: untrusted {root}\n"));
}

#[test]
fn untrust_of_an_untrusted_repo_says_so() {
    let env = TrustEnv::new();
    let repo = repo();
    env.toolr(repo.path())
        .args(["project", "untrust"])
        .assert()
        .success()
        .stdout("")
        .stderr(contains("was not trusted"));
}

#[test]
fn trusted_prune_drops_missing_paths() {
    let env = TrustEnv::new();
    let repo = repo();
    let elsewhere = TempDir::new().unwrap();
    env.toolr(repo.path())
        .args(["project", "trust"])
        .assert()
        .success();
    let root = fs::canonicalize(repo.path()).unwrap().display().to_string();
    repo.close().unwrap();

    env.toolr(elsewhere.path())
        .args(["self", "trusted"])
        .assert()
        .success()
        .stdout(format!("{root} (missing)\n"));
    env.toolr(elsewhere.path())
        .args(["self", "trusted", "--prune"])
        .assert()
        .success()
        .stdout("");
    env.toolr(elsewhere.path())
        .args(["self", "trusted"])
        .assert()
        .success()
        .stdout("");
}

#[test]
fn init_says_it_trusted_the_repository() {
    let env = TrustEnv::new();
    let repo = TempDir::new().unwrap();
    let root = fs::canonicalize(repo.path()).unwrap().display().to_string();
    env.toolr(repo.path())
        .args(["project", "init", "--yes", "--no-sync"])
        .assert()
        .success()
        .stderr(contains(format!("toolr: trusted {root}")));
    env.toolr(repo.path())
        .args(["self", "trusted"])
        .assert()
        .success()
        .stdout(format!("{root}\n"));
}

#[test]
fn quiet_init_trusts_without_saying_so() {
    let env = TrustEnv::new();
    let repo = TempDir::new().unwrap();
    let out = env
        .toolr(repo.path())
        .args(["--quiet", "project", "init", "--yes", "--no-sync"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("trusted"));
    let root = fs::canonicalize(repo.path()).unwrap().display().to_string();
    env.toolr(repo.path())
        .args(["self", "trusted"])
        .assert()
        .success()
        .stdout(format!("{root}\n"));
}

#[cfg(unix)]
#[test]
fn retrust_rebuilds_the_in_tree_venv_once() {
    let env = TrustEnv::new();
    let fx = common::VenvFixture::new();
    let toolr = |args: &[&str]| {
        let out = env
            .toolr(&fx.root)
            .env("PATH", &fx.bin_dir)
            .env("TOOLR_AUTO_SYNC", "1")
            .env_remove("TOOLR_VENV_LOCATION")
            .args(args)
            .output()
            .unwrap();
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(out.status.success(), "toolr {args:?}: {stderr}");
        let synced = fx.uv_ran_sync();
        let _ = fs::remove_file(&fx.uv_argv_log);
        (
            synced,
            stderr.contains("toolr didn't build this in-tree venv"),
        )
    };
    for round in 0..2 {
        assert_eq!(
            toolr(&["project", "trust"]),
            (true, true),
            "trust, round {round}"
        );
        assert_eq!(
            toolr(&["project", "venv", "sync"]),
            (false, false),
            "round {round}"
        );
        toolr(&["project", "untrust"]);
    }
}
