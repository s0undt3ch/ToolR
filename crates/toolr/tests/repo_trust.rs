//! Repository trust: toolr must not run a repository's code (import
//! `tools/*.py`, `uv sync`, run venv binaries) until the user trusts it.
#![cfg(unix)]

mod common;

use std::fs;
use std::path::PathBuf;

use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use tempfile::TempDir;

use common::{TrustEnv, hostile_repo};
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

    /// The `meta.json` a pre-trust toolr left after a finished sync.
    fn seed_synced_meta(&self) {
        let mut meta = Meta::new(self.root(), "0.35.1", "3.13.1");
        meta.interpreter_hash = Some("hash".into());
        meta.write(&self.cache_entry()).unwrap();
    }

    /// The bare `meta.json` dispatch backfills, including for a run it then
    /// refuses, and the one every sync writes once trust exists.
    fn seed_backfilled_meta(&self) {
        Meta::new(self.root(), "0.35.1", "3.13.1")
            .write(&self.cache_entry())
            .unwrap();
    }

    fn cache_entry(&self) -> PathBuf {
        self.env.cache.path().join("toolr").join("seeded")
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
    h.seed_synced_meta();
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
    h.seed_backfilled_meta();
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

#[test]
fn home_with_tools_dir_can_be_trusted() {
    let home = TempDir::new().unwrap();
    let home_root = fs::canonicalize(home.path()).unwrap();
    fs::create_dir(home_root.join("tools")).unwrap();
    let proj = home_root.join("proj");
    fs::create_dir(&proj).unwrap();
    let env = TrustEnv::new();
    let toolr = || {
        let mut cmd = env.toolr(&proj);
        cmd.env("HOME", &home_root)
            .env_remove("XDG_STATE_HOME")
            .env_remove("XDG_DATA_HOME");
        cmd
    };
    toolr()
        .args(["project", "trust"])
        .assert()
        .success()
        .stdout(format!("{}\n", home_root.display()));
    toolr()
        .args(["self", "trusted"])
        .assert()
        .success()
        .stdout(format!("{}\n", home_root.display()));
}

#[test]
fn upgrade_trusts_a_repo_with_a_finished_sync() {
    let h = Hostile::new(true);
    h.seed_synced_meta();
    h.toolr()
        .args(["hello", "world"])
        .assert()
        .stderr(contains(REFUSAL).not());
    assert!(h.sentinel.exists());
}

/// A pre-trust toolr refused to run this repo but still backfilled its
/// cache sidecar. That is no consent, so the upgrade must not trust it.
#[test]
fn upgrade_does_not_trust_a_refused_attempt() {
    let h = Hostile::new(true);
    h.seed_backfilled_meta();
    assert!(!h.cache_entry().join("venv").exists());
    h.toolr()
        .args(["project", "venv", "sync", "--quiet"])
        .assert()
        .failure()
        .stderr(contains(REFUSAL));
    h.toolr()
        .args(["hello", "world"])
        .assert()
        .failure()
        .stderr(contains(REFUSAL));
    h.toolr()
        .args(["self", "trusted"])
        .assert()
        .success()
        .stdout("");
    assert!(!h.sentinel.exists());
}

#[test]
fn untrust_survives_a_wiped_trust_store() {
    let h = Hostile::new(true);
    h.seed_synced_meta();
    h.toolr().args(["project", "untrust"]).assert().success();
    fs::remove_dir_all(h.env.state.path().join("toolr").join("trust")).unwrap();
    h.toolr()
        .args(["hello", "world"])
        .assert()
        .failure()
        .stderr(contains(REFUSAL));
    assert!(!h.sentinel.exists());
}

#[test]
fn trust_commands_escape_control_characters() {
    let env = TrustEnv::new();
    let parent = TempDir::new().unwrap();
    let repo = fs::canonicalize(parent.path()).unwrap().join("evil\rLEGIT");
    fs::create_dir_all(repo.join("tools")).unwrap();
    let expected = format!(
        "{}\\rLEGIT\n",
        repo.parent().unwrap().join("evil").display()
    );
    env.toolr(&repo)
        .args(["project", "trust"])
        .assert()
        .success()
        .stdout(expected.clone());
    env.toolr(&repo)
        .args(["self", "trusted"])
        .assert()
        .success()
        .stdout(expected.clone());
    env.toolr(&repo)
        .args(["project", "untrust"])
        .assert()
        .success()
        .stdout(expected);
}
