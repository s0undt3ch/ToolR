//! Repository trust: toolr must not run a repository's code (import
//! `tools/*.py`, `uv sync`, run venv binaries) until the user trusts it.
#![cfg(unix)]

mod common;

use std::fs;
use std::path::PathBuf;

use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use tempfile::TempDir;

use common::{TrustEnv, hostile_repo, write_executable};
use toolr_core::cache::Meta;
use toolr_core::manifest::{Manifest, SCHEMA_VERSION, write_manifest};

const REFUSAL: &str = "refusing to run code from an untrusted repository";

struct Hostile {
    env: TrustEnv,
    _out: TempDir,
    sentinel: PathBuf,
    repo: TempDir,
}

impl Hostile {
    fn new() -> Self {
        let out = TempDir::new().unwrap();
        let sentinel = out.path().join("sentinel");
        let repo = hostile_repo(&sentinel);
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

    /// The `meta.json` a pre-trust toolr left after finishing a sync.
    fn seed_pre_trust_venv_cache(&self) {
        let entry = self.env.cache.path().join("toolr").join("seeded");
        let mut meta = Meta::new(self.root(), "0.35.1", "3.13.1");
        meta.interpreter_hash = Some("hash".into());
        meta.write(&entry).unwrap();
        fs::create_dir_all(entry.join("venv")).unwrap();
    }
}

#[test]
fn untrusted_dispatch_is_refused() {
    let h = Hostile::new();
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
fn quiet_sync_refusal_is_not_swallowed() {
    let h = Hostile::new();
    h.toolr()
        .args(["project", "venv", "sync", "--quiet"])
        .assert()
        .failure()
        .stderr(contains(REFUSAL));
    assert!(!h.sentinel.exists());
}

#[test]
fn venv_run_no_sync_is_refused() {
    let h = Hostile::new();
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
    let h = Hostile::new();
    h.toolr().arg("--help").assert().success();
    h.toolr()
        .args(["__complete", &h.repo.path().to_string_lossy(), ""])
        .assert()
        .success();
    assert!(!h.sentinel.exists());
}

#[test]
fn trust_then_dispatch_runs() {
    let h = Hostile::new();
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
    let h = Hostile::new();
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
fn trust_all_env_bypasses() {
    let h = Hostile::new();
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
    let h = Hostile::new();
    h.toolr()
        .env("TOOLR_TRUST_ALL", "1")
        .args(["hello", "world"])
        .assert()
        .stderr(contains(REFUSAL).not());
    fs::remove_file(&h.sentinel).unwrap();
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
fn project_init_records_trust_and_its_sync_passes_the_gate() {
    let env = TrustEnv::new();
    let dir = TempDir::new().unwrap();
    let bin = TempDir::new().unwrap();
    let uv_log = bin.path().join("uv.log");
    write_executable(
        &bin.path().join("uv"),
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> {}\n[ \"$1\" = --version ] && echo 'uv 0.5.0'\nexit 0\n",
            uv_log.display()
        ),
    );
    env.toolr(dir.path())
        .env("PATH", format!("{}:/usr/bin:/bin", bin.path().display()))
        .args(["project", "init", "--python", ">=3.11"])
        .assert()
        .stderr(contains(REFUSAL).not());
    let calls = fs::read_to_string(&uv_log).unwrap_or_default();
    assert!(
        calls.lines().any(|l| l == "sync"),
        "uv sync never ran: {calls:?}"
    );
    let root = fs::canonicalize(dir.path()).unwrap().display().to_string();
    env.toolr(dir.path())
        .args(["self", "trusted"])
        .assert()
        .success()
        .stdout(format!("{root}\n"));
}

#[test]
fn project_init_with_unwritable_store_warns_and_succeeds() {
    use std::os::unix::fs::PermissionsExt;
    let env = TrustEnv::new();
    fs::set_permissions(env.state.path(), fs::Permissions::from_mode(0o500)).unwrap();
    let dir = TempDir::new().unwrap();
    let assert = env
        .toolr(dir.path())
        .args(["project", "init", "--no-sync"])
        .assert();
    fs::set_permissions(env.state.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert
        .success()
        .stderr(contains("warning: could not record trust"));
    assert!(dir.path().join("tools").join("pyproject.toml").is_file());
}

#[test]
fn project_init_under_trust_all_records_nothing() {
    let env = TrustEnv::new();
    let dir = TempDir::new().unwrap();
    env.toolr(dir.path())
        .env("TOOLR_TRUST_ALL", "1")
        .args(["project", "init", "--no-sync"])
        .assert()
        .success();
    env.toolr(dir.path())
        .args(["self", "trusted"])
        .assert()
        .success()
        .stdout("");
}

#[test]
fn home_with_tools_dir_can_be_trusted() {
    let home = TempDir::new().unwrap();
    let home_root = fs::canonicalize(home.path()).unwrap();
    common::write_project(&home_root);
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
fn trust_commands_escape_control_characters() {
    let env = TrustEnv::new();
    let parent = TempDir::new().unwrap();
    let repo = fs::canonicalize(parent.path()).unwrap().join("evil\rLEGIT");
    common::write_project(&repo);
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

/// Trust starts empty on upgrade: a venv an older toolr built is no
/// consent to keep running the repository.
#[test]
fn a_venv_from_before_trust_does_not_trust_the_repo() {
    let h = Hostile::new();
    h.seed_pre_trust_venv_cache();
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

/// A repository with a `demo hello` command and no venv, as in the reported repro.
fn demo_repo() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir_all(&tools).unwrap();
    fs::write(
        tools.join("pyproject.toml"),
        "[project]\nname = \"demo-tools\"\nversion = \"0\"\n",
    )
    .unwrap();
    fs::write(
        tools.join("demo.py"),
        "\"\"\"Demo.\"\"\"\nfrom toolr import command_group\n\
         group = command_group(\"demo\", \"Demo commands\")\n\n\
         @group.command\ndef hello(ctx):\n    \"\"\"Say hello.\"\"\"\n",
    )
    .unwrap();
    tmp
}

fn manifest_path(repo: &TempDir) -> PathBuf {
    repo.path().join("tools").join(".toolr-manifest.json")
}

#[test]
fn root_help_in_untrusted_repo_does_not_write_manifest() {
    let env = TrustEnv::new();
    let repo = demo_repo();
    env.toolr(repo.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("demo"));
    assert!(
        !manifest_path(&repo).exists(),
        "`toolr --help` wrote the manifest into an untrusted repository"
    );
    env.toolr(repo.path()).assert().stdout(contains("demo"));
    assert!(
        !manifest_path(&repo).exists(),
        "bare `toolr` wrote the manifest into an untrusted repository"
    );
}

#[test]
fn refused_dispatch_does_not_write_manifest() {
    let env = TrustEnv::new();
    let repo = demo_repo();
    env.toolr(repo.path())
        .args(["demo", "hello"])
        .write_stdin("")
        .assert()
        .failure()
        .stderr(contains(REFUSAL));
    assert!(
        !manifest_path(&repo).exists(),
        "a refused dispatch wrote the manifest into an untrusted repository"
    );
}

#[test]
fn stale_manifest_in_untrusted_repo_is_not_rewritten() {
    let env = TrustEnv::new();
    let repo = demo_repo();
    let path = manifest_path(&repo);
    let stale = Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: "stale".into(),
        third_party_hash: String::new(),
        toolr_version: env!("CARGO_PKG_VERSION").into(),
        groups: vec![],
        commands: vec![],
        plugin_warnings: vec![],
    };
    write_manifest(&path, &stale).unwrap();
    let bytes = fs::read_to_string(&path).unwrap();
    let mtime = fs::metadata(&path).unwrap().modified().unwrap();

    env.toolr(repo.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("demo"))
        .stderr(contains("unreadable").not());

    assert_eq!(fs::read_to_string(&path).unwrap(), bytes);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), mtime);
}

#[test]
fn trusted_repo_help_still_writes_manifest() {
    let env = TrustEnv::new();
    let repo = demo_repo();
    env.toolr(repo.path())
        .args(["project", "trust"])
        .assert()
        .success();
    env.toolr(repo.path()).arg("--help").assert().success();
    let written = fs::read_to_string(manifest_path(&repo)).unwrap();
    assert!(written.contains("\"demo\""));
}

#[test]
fn trust_all_env_help_writes_manifest() {
    let env = TrustEnv::new();
    let repo = demo_repo();
    env.toolr(repo.path())
        .env("TOOLR_TRUST_ALL", "1")
        .arg("--help")
        .assert()
        .success();
    assert!(manifest_path(&repo).is_file());
}
