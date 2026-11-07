//! Trust follows the git repository, not the checkout: one `project trust`
//! covers every worktree, driven here against real `git` repositories.
#![cfg(unix)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use tempfile::TempDir;

use common::TrustEnv;

const REFUSAL: &str = "refusing to run code from an untrusted repository";

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(["-c", "init.defaultBranch=main", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_CEILING_DIRECTORIES")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Make `root` a committed git repository holding a toolr project.
fn init_repo(root: &Path, extra_init: &[&str]) -> PathBuf {
    fs::create_dir_all(root).unwrap();
    let mut args = vec!["init"];
    args.extend_from_slice(extra_init);
    git(root, &args);
    let tools = common::write_project(root);
    fs::write(tools.join("greet.py"), common::GREET).unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-m", "init"]);
    fs::canonicalize(root).unwrap()
}

fn add_worktree(repo: &Path, at: &Path, branch: &str) -> PathBuf {
    fs::create_dir_all(at.parent().unwrap()).unwrap();
    git(repo, &["worktree", "add", "-b", branch, at.to_str().unwrap()]);
    fs::canonicalize(at).unwrap()
}

fn shown(path: &Path) -> String {
    format!("{}\n", path.display())
}

fn trust(env: &TrustEnv, at: &Path) {
    env.toolr(at)
        .args(["project", "trust", "--no-sync"])
        .assert()
        .success();
}

#[test]
fn trust_main_covers_worktrees() {
    let env = TrustEnv::new();
    let tmp = TempDir::new().unwrap();
    let main = init_repo(&tmp.path().join("main"), &[]);
    let inside = add_worktree(&main, &main.join(".worktrees/inner"), "inner");
    let outside = add_worktree(&main, &tmp.path().join("outer"), "outer");

    for wt in [&inside, &outside] {
        trust(&env, &main);
        env.toolr(wt)
            .args(["project", "untrust"])
            .assert()
            .success()
            .stdout(shown(&main));
    }
}

#[test]
fn trust_from_worktree_prints_main_path() {
    let env = TrustEnv::new();
    let tmp = TempDir::new().unwrap();
    let main = init_repo(&tmp.path().join("main"), &[]);
    let wt = add_worktree(&main, &tmp.path().join("wt"), "wt");
    env.toolr(&wt)
        .args(["project", "trust", "--no-sync"])
        .assert()
        .success()
        .stdout(shown(&main));
    env.toolr(&main)
        .args(["project", "untrust"])
        .assert()
        .success()
        .stdout(shown(&main));
}

#[test]
fn self_trusted_shows_repo_path_and_prunes_by_key() {
    let env = TrustEnv::new();
    let tmp = TempDir::new().unwrap();
    let main = init_repo(&tmp.path().join("main"), &[]);
    let gitdir = main.join(".git");
    let wt = add_worktree(&main, &tmp.path().join("wt"), "wt");
    trust(&env, &main);
    let listed = env
        .toolr(tmp.path())
        .args(["self", "trusted"])
        .output()
        .unwrap();
    let before = String::from_utf8(listed.stdout).unwrap();
    assert!(!before.contains("(missing)"), "{before}");

    // Drop the main checkout's files but keep the common dir the worktree needs.
    for entry in fs::read_dir(&main).unwrap() {
        let path = entry.unwrap().path();
        if path != gitdir {
            if path.is_dir() {
                fs::remove_dir_all(&path).unwrap();
            } else {
                fs::remove_file(&path).unwrap();
            }
        }
    }
    env.toolr(&wt)
        .args(["self", "trusted"])
        .assert()
        .success()
        .stdout(before.clone());

    git(&wt, &["worktree", "remove", "--force", wt.to_str().unwrap()]);
    fs::remove_dir_all(&gitdir).unwrap();
    let missing = format!("{} (missing)\n", before.trim_end());
    env.toolr(tmp.path())
        .args(["self", "trusted"])
        .assert()
        .success()
        .stdout(missing);
    env.toolr(tmp.path())
        .args(["self", "trusted", "--prune"])
        .assert()
        .success()
        .stdout("");
    env.toolr(tmp.path())
        .args(["self", "trusted"])
        .assert()
        .success()
        .stdout("");
}

#[test]
fn bare_repo_listed_as_bare_dir() {
    let env = TrustEnv::new();
    let tmp = TempDir::new().unwrap();
    let main = init_repo(&tmp.path().join("main"), &[]);
    let bare = tmp.path().join("foo.git");
    git(
        tmp.path(),
        &["clone", "--bare", main.to_str().unwrap(), bare.to_str().unwrap()],
    );
    let wt = tmp.path().join("wt");
    git(&bare, &["worktree", "add", wt.to_str().unwrap(), "main"]);
    let bare = fs::canonicalize(bare).unwrap();
    trust(&env, &wt);
    env.toolr(tmp.path())
        .args(["self", "trusted"])
        .assert()
        .success()
        .stdout(shown(&bare));
}

#[test]
fn git_env_does_not_change_trust() {
    let env = TrustEnv::new();
    let tmp = TempDir::new().unwrap();
    let main = init_repo(&tmp.path().join("main"), &[]);
    let other = init_repo(&tmp.path().join("other"), &[]);
    let wt = add_worktree(&main, &tmp.path().join("wt"), "wt");
    trust(&env, &main);
    let other_git = other.join(".git");
    env.toolr(&wt)
        .args(["greet", "hi"])
        .env("GIT_DIR", &other_git)
        .env("GIT_COMMON_DIR", &other_git)
        .assert()
        .stderr(contains(REFUSAL).not());
    env.toolr(&other)
        .args(["greet", "hi"])
        .assert()
        .failure()
        .stderr(contains(REFUSAL));
}

#[test]
fn moved_worktree_refusal_hints_repair() {
    let env = TrustEnv::new();
    let tmp = TempDir::new().unwrap();
    let main = init_repo(&tmp.path().join("main"), &[]);
    let wt = add_worktree(&main, &tmp.path().join("wt"), "wt");
    trust(&env, &main);
    let moved = tmp.path().join("moved");
    fs::rename(&wt, &moved).unwrap();
    env.toolr(&moved)
        .args(["greet", "hi"])
        .assert()
        .failure()
        .stderr(contains(REFUSAL))
        .stderr(contains("git worktree repair"));
}
