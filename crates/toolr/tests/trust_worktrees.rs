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
        .args([
            "-c",
            "init.defaultBranch=main",
            "-c",
            "commit.gpgsign=false",
        ])
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
fn init_repo(root: &Path) -> PathBuf {
    fs::create_dir_all(root).unwrap();
    git(root, &["init"]);
    let tools = common::write_project(root);
    fs::write(tools.join("greet.py"), common::GREET).unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-m", "init"]);
    fs::canonicalize(root).unwrap()
}

fn add_worktree(repo: &Path, at: &Path, branch: &str) -> PathBuf {
    fs::create_dir_all(at.parent().unwrap()).unwrap();
    git(
        repo,
        &["worktree", "add", "-b", branch, at.to_str().unwrap()],
    );
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
    let main = init_repo(&tmp.path().join("main"));
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
    let main = init_repo(&tmp.path().join("main"));
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
    let main = init_repo(&tmp.path().join("main"));
    let gitdir = main.join(".git");
    let wt = add_worktree(&main, &tmp.path().join("wt"), "wt");
    trust(&env, &main);
    let before = shown(&main);
    env.toolr(tmp.path())
        .args(["self", "trusted"])
        .assert()
        .success()
        .stdout(before.clone());

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
    env.toolr(&wt)
        .args(["self", "trusted", "--prune"])
        .assert()
        .success()
        .stdout(before.clone())
        .stderr(contains("pruned").not());
    env.toolr(&wt)
        .args(["self", "trusted"])
        .assert()
        .success()
        .stdout(before.clone());

    git(
        &wt,
        &["worktree", "remove", "--force", wt.to_str().unwrap()],
    );
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
    let main = init_repo(&tmp.path().join("main"));
    let bare = tmp.path().join("foo.git");
    git(
        tmp.path(),
        &[
            "clone",
            "--bare",
            main.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
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
    let main = init_repo(&tmp.path().join("main"));
    let other = init_repo(&tmp.path().join("other"));
    let wt = add_worktree(&main, &tmp.path().join("wt"), "wt");
    trust(&env, &main);
    let other_git = other.join(".git");
    env.toolr(&wt)
        .args(["project", "untrust"])
        .env("GIT_DIR", &other_git)
        .env("GIT_COMMON_DIR", &other_git)
        .assert()
        .success()
        .stdout(shown(&main));
    env.toolr(&other)
        .args(["greet", "hi"])
        .assert()
        .failure()
        .stderr(contains(REFUSAL));
}

/// `git worktree repair <path>`, quoted the way the refusal prints it.
fn repair_command(path: &Path) -> String {
    format!("`git worktree repair {}`", path.display())
}

#[test]
fn moved_worktree_refusal_hints_repair() {
    let env = TrustEnv::new();
    let tmp = TempDir::new().unwrap();
    let main = init_repo(&tmp.path().join("main"));
    let tools = common::write_project(&main.join("sub"));
    fs::write(tools.join("greet.py"), common::GREET).unwrap();
    git(&main, &["add", "-A"]);
    git(&main, &["commit", "-m", "nested project"]);
    let wt = add_worktree(&main, &tmp.path().join("wt"), "wt");
    trust(&env, &main);
    fs::rename(&wt, tmp.path().join("moved")).unwrap();
    let moved = fs::canonicalize(tmp.path().join("moved")).unwrap();
    env.toolr(&moved.join("sub"))
        .args(["greet", "hi"])
        .assert()
        .failure()
        .stderr(contains(REFUSAL))
        .stderr(contains(format!(
            "claims to be worktree `wt` of {}. Only if you moved that worktree yourself, run {}",
            main.display(),
            repair_command(&moved)
        )))
        .stderr(contains(repair_command(&moved.join("sub"))).not());
}

#[test]
fn deleted_worktree_claimed_elsewhere_gets_cautious_hint() {
    let env = TrustEnv::new();
    let tmp = TempDir::new().unwrap();
    let main = init_repo(&tmp.path().join("main"));
    let wt = add_worktree(&main, &tmp.path().join("feature"), "feature");
    trust(&env, &main);
    fs::remove_dir_all(&wt).unwrap();
    let evil = tmp.path().join("Downloads/evil");
    let tools = common::write_project(&evil);
    fs::write(tools.join("greet.py"), common::GREET).unwrap();
    let gitdir = main.join(".git/worktrees/feature");
    fs::write(evil.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
    let evil = fs::canonicalize(evil).unwrap();
    env.toolr(&evil)
        .args(["greet", "hi"])
        .assert()
        .failure()
        .stderr(contains(REFUSAL))
        .stderr(contains(format!(
            "claims to be worktree `feature` of {}. Only if you moved that worktree yourself, run {}",
            main.display(),
            repair_command(&evil)
        )));
}

/// A stub uv whose `sync` builds a minimal venv at `$UV_PROJECT_ENVIRONMENT`
/// and logs each call to `log`.
fn venv_building_uv(bin: &Path, log: &Path) {
    fs::create_dir_all(bin).unwrap();
    common::write_executable(
        &bin.join("uv"),
        &format!(
            "#!/bin/sh\ncase \"$1\" in\n  --version) echo 'uv 0.5.0';;\n  sync)\n    echo \"sync $PWD\" >> {log}\n    \
             /bin/mkdir -p \"$UV_PROJECT_ENVIRONMENT/bin\" \"$UV_PROJECT_ENVIRONMENT/lib/python3.13/site-packages/toolr\"\n    \
             : > \"$UV_PROJECT_ENVIRONMENT/lib/python3.13/site-packages/toolr/__init__.py\"\n    \
             printf '#!/bin/sh\\nexit 0\\n' > \"$UV_PROJECT_ENVIRONMENT/bin/python\"\n    \
             /bin/chmod +x \"$UV_PROJECT_ENVIRONMENT/bin/python\";;\nesac\nexit 0\n",
            log = log.display()
        ),
    );
}

#[test]
fn worktrees_keep_separate_venv_tokens() {
    let env = TrustEnv::new();
    let tmp = TempDir::new().unwrap();
    let main = init_repo(&tmp.path().join("main"));
    let a = add_worktree(&main, &tmp.path().join("a"), "a");
    let b = add_worktree(&main, &tmp.path().join("b"), "b");
    let bin = tmp.path().join("bin");
    let log = tmp.path().join("uv.log");
    venv_building_uv(&bin, &log);
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    trust(&env, &main);
    let run = |wt: &Path, args: &[&str]| {
        env.toolr(wt)
            .env("PATH", &path)
            .env_remove("TOOLR_VENV_LOCATION")
            .args(args)
            .assert()
            .success();
    };
    for wt in [&a, &b] {
        fs::write(wt.join("tools/uv.lock"), "version = 1\n").unwrap();
        run(wt, &["project", "venv", "sync"]);
    }
    assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), 2);
    fs::remove_file(&log).unwrap();
    run(&a, &["greet", "hi"]);
    run(&b, &["greet", "hi"]);
    assert!(!log.exists(), "a worktree's sync rebuilt the other's venv");
}
