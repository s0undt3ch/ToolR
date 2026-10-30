//! Repository trust: toolr must not run a repository's code (import
//! `tools/*.py`, `uv sync`, run venv binaries) until the user trusts it.
#![cfg(unix)]

mod common;

use std::fs;
use std::path::PathBuf;

use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use tempfile::TempDir;

use common::{hostile_repo, TrustEnv};
use toolr_core::cache::Meta;

const REFUSAL: &str = "refusing to run code from an untrusted repository";

struct Hostile {
    env: TrustEnv,
    _out: TempDir,
    sentinel: PathBuf,
    repo: TempDir,
}

impl Hostile {
    fn new(with_pyproject: bool) -> Self {
        let out = TempDir::new().unwrap();
        let sentinel = out.path().join("sentinel");
        let repo = hostile_repo(&sentinel, with_pyproject);
        Self {
            env: TrustEnv::new(),
            _out: out,
            sentinel,
            repo,
        }
    }

    fn root(&self) -> PathBuf {
        fs::canonicalize(self.repo.path()).unwrap()
    }

    fn toolr(&self) -> assert_cmd::Command {
        self.env.toolr(self.repo.path())
    }

    /// The `meta.json` a successful sync leaves in toolr's cache root.
    fn seed_cache_meta(&self) {
        Meta::new(self.root(), "0.0.0", "3.13.1")
            .write(&self.env.cache.path().join("toolr").join("seeded"))
            .unwrap();
    }
}

#[test]
fn untrusted_dispatch_is_refused() {
    let h = Hostile::new(true);
    h.toolr()
        .args(["hello", "world"])
        .assert()
        .failure()
        .stderr(contains(REFUSAL))
        .stderr(contains(h.root().display().to_string()));
    assert!(
        !h.sentinel.exists(),
        "untrusted dispatch ran repository code"
    );
}

#[test]
fn untrusted_repo_without_pyproject_is_refused() {
    let h = Hostile::new(false);
    h.toolr()
        .args(["hello", "world"])
        .assert()
        .failure()
        .stderr(contains(REFUSAL));
    assert!(
        !h.sentinel.exists(),
        "untrusted dispatch ran repository code"
    );
}

#[test]
fn quiet_sync_refusal_is_not_swallowed() {
    let h = Hostile::new(true);
    h.toolr()
        .args(["project", "venv", "sync", "--quiet"])
        .assert()
        .failure()
        .stderr(contains(REFUSAL));
    assert!(!h.sentinel.exists());
}

#[test]
fn venv_run_no_sync_is_refused() {
    let h = Hostile::new(true);
    h.toolr()
        .args([
            "project",
            "venv",
            "run",
            "--no-sync",
            "--",
            "python",
            "-c",
            "1",
        ])
        .assert()
        .failure()
        .stderr(contains(REFUSAL));
    assert!(!h.sentinel.exists());
}

#[test]
fn help_and_complete_need_no_trust() {
    let h = Hostile::new(true);
    h.toolr().arg("--help").assert().success();
    h.toolr()
        .args(["__complete", &h.repo.path().to_string_lossy(), ""])
        .assert()
        .success();
    assert!(!h.sentinel.exists());
}

#[test]
fn trust_then_dispatch_runs() {
    let h = Hostile::new(true);
    let root = h.root().display().to_string();
    h.toolr()
        .args(["project", "trust"])
        .assert()
        .success()
        .stdout(format!("{root}\n"));
    h.toolr()
        .args(["self", "trusted"])
        .assert()
        .success()
        .stdout(format!("{root}\n"));
    h.toolr()
        .args(["hello", "world"])
        .assert()
        .stderr(contains(REFUSAL).not());
    assert!(
        h.sentinel.exists(),
        "trusted dispatch should have run the repository's code"
    );
}

#[test]
fn untrust_then_refused() {
    let h = Hostile::new(true);
    h.seed_cache_meta();
    h.toolr().args(["project", "trust"]).assert().success();
    h.toolr().args(["project", "untrust"]).assert().success();
    h.toolr()
        .args(["hello", "world"])
        .assert()
        .failure()
        .stderr(contains(REFUSAL));
    assert!(!h.sentinel.exists());
}

#[test]
fn trusted_prune_drops_missing_paths() {
    let env = TrustEnv::new();
    let repo = TempDir::new().unwrap();
    fs::create_dir(repo.path().join("tools")).unwrap();
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
fn trust_all_env_bypasses() {
    let h = Hostile::new(true);
    h.toolr()
        .env("TOOLR_TRUST_ALL", "1")
        .args(["hello", "world"])
        .assert()
        .stderr(contains(REFUSAL).not());
    assert!(
        h.sentinel.exists(),
        "TOOLR_TRUST_ALL=1 dispatch should have run the repository's code"
    );
}

#[test]
fn trust_all_run_does_not_leave_trust_behind() {
    let h = Hostile::new(true);
    h.toolr()
        .env("TOOLR_TRUST_ALL", "1")
        .args(["hello", "world"])
        .assert()
        .stderr(contains(REFUSAL).not());
    fs::remove_file(&h.sentinel).unwrap();
    // The fixture can't really sync, so stand in for the meta.json that a
    // successful sync under TOOLR_TRUST_ALL=1 would have written.
    h.seed_cache_meta();
    h.toolr()
        .args(["hello", "world"])
        .assert()
        .failure()
        .stderr(contains(REFUSAL));
    assert!(!h.sentinel.exists());
}

#[test]
fn quiet_sync_in_untrusted_stray_tools_dir_is_silent() {
    let env = TrustEnv::new();
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("tools")).unwrap();
    env.toolr(dir.path())
        .args(["project", "venv", "sync", "--quiet"])
        .assert()
        .success()
        .stdout("")
        .stderr("");
    env.toolr(dir.path())
        .args(["self", "trusted"])
        .assert()
        .success()
        .stdout("");
}

#[test]
fn project_init_records_trust_before_sync() {
    let env = TrustEnv::new();
    let dir = TempDir::new().unwrap();
    env.toolr(dir.path())
        .args(["project", "init", "--no-sync"])
        .assert()
        .success();
    let root = fs::canonicalize(dir.path()).unwrap().display().to_string();
    env.toolr(dir.path())
        .args(["self", "trusted"])
        .assert()
        .success()
        .stdout(format!("{root}\n"));
}
