//! End-to-end checks for `cargo xtask patch-coverage` against a real
//! throwaway git repository, so the git invocation is exercised unmocked.

use std::fs;
use std::path::Path;
use std::process::Command as Process;

use assert_cmd::Command;
use tempfile::TempDir;

fn git(root: &Path, args: &[&str]) {
    let status = Process::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
        .status()
        .expect("spawn git");
    assert!(status.success(), "git {args:?} failed");
}

/// A repo whose `base` branch has a 2-line lib and whose HEAD adds lines 3-5.
fn fixture() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    let src = root.join("crates/demo/src");
    fs::create_dir_all(&src).expect("mkdir");
    git(&root, &["init", "-q", "-b", "base"]);
    fs::write(src.join("lib.rs"), "fn a() {}\nfn b() {}\n").expect("write");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "base"]);
    git(&root, &["checkout", "-q", "-b", "change"]);
    fs::write(
        src.join("lib.rs"),
        "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\n",
    )
    .expect("write");
    git(&root, &["commit", "-q", "-am", "change"]);
    dir
}

fn write_lcov(root: &Path, hits: [u32; 3]) {
    let sf = root
        .canonicalize()
        .expect("canonicalize")
        .join("crates/demo/src/lib.rs");
    let da: String = hits
        .iter()
        .enumerate()
        .map(|(i, h)| format!("DA:{},{h}\n", i + 3))
        .collect();
    fs::create_dir_all(root.join("target/coverage")).expect("mkdir");
    fs::write(
        root.join("target/coverage/lcov.info"),
        format!("SF:{}\nDA:1,1\n{da}end_of_record\n", sf.display()),
    )
    .expect("write lcov");
}

fn xtask(root: &Path) -> Command {
    let mut cmd = Command::cargo_bin("xtask").expect("xtask binary");
    cmd.args(["patch-coverage", "--base", "base", "--root"])
        .arg(root);
    cmd
}

struct Run {
    ok: bool,
    stdout: String,
    stderr: String,
}

fn run(cmd: &mut Command) -> Run {
    let out = cmd.output().expect("run xtask");
    Run {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

#[test]
fn passes_when_changed_lines_are_covered() {
    let repo = fixture();
    write_lcov(repo.path(), [1, 1, 1]);
    let r = run(&mut xtask(repo.path()));
    assert!(r.ok, "{}", r.stderr);
    assert_eq!(r.stdout, "patch coverage: 3/3 = 100.0%\n");
}

#[test]
fn fails_below_threshold_and_lists_missed_lines() {
    let repo = fixture();
    write_lcov(repo.path(), [1, 0, 1]);
    let r = run(&mut xtask(repo.path()));
    assert!(!r.ok);
    assert_eq!(
        r.stdout,
        "crates/demo/src/lib.rs: 2/3, missed lines [4]\npatch coverage: 2/3 = 66.7%\n"
    );
    assert!(r.stderr.contains("below the 99% threshold"), "{}", r.stderr);
}

#[test]
fn fail_under_can_be_lowered() {
    let repo = fixture();
    write_lcov(repo.path(), [1, 0, 1]);
    let r = run(xtask(repo.path()).args(["--fail-under", "60"]));
    assert!(r.ok, "{}", r.stderr);
}

#[test]
fn absolute_lcov_path_is_used_as_is() {
    let repo = fixture();
    write_lcov(repo.path(), [1, 1, 1]);
    let elsewhere = tempfile::tempdir().expect("tempdir");
    let lcov = elsewhere.path().join("lcov.info");
    fs::rename(repo.path().join("target/coverage/lcov.info"), &lcov).expect("move lcov");
    let r = run(xtask(repo.path()).arg("--lcov").arg(&lcov));
    assert!(r.ok, "{}", r.stderr);
    assert_eq!(r.stdout, "patch coverage: 3/3 = 100.0%\n");
}

#[test]
fn root_defaults_to_the_workspace() {
    let elsewhere = tempfile::tempdir().expect("tempdir");
    let lcov = elsewhere.path().join("missing.info");
    let r = run(Command::cargo_bin("xtask")
        .expect("xtask binary")
        .args(["patch-coverage", "--base", "main", "--lcov"])
        .arg(&lcov));
    assert!(!r.ok);
    assert!(
        r.stderr.contains(&format!("reading {}", lcov.display())),
        "{}",
        r.stderr
    );
}

#[test]
fn missing_lcov_explains_how_to_generate_it() {
    let repo = fixture();
    let r = run(&mut xtask(repo.path()));
    assert!(!r.ok);
    assert!(r.stderr.contains("cargo llvm-cov"), "{}", r.stderr);
    assert!(
        r.stderr.contains("target/coverage/lcov.info"),
        "{}",
        r.stderr
    );
}

#[test]
fn unknown_base_surfaces_git_stderr() {
    let repo = fixture();
    write_lcov(repo.path(), [1, 1, 1]);
    let r = run(Command::cargo_bin("xtask")
        .expect("xtask binary")
        .args(["patch-coverage", "--base", "no-such-ref", "--root"])
        .arg(repo.path()));
    assert!(!r.ok);
    assert!(
        r.stderr.contains("git diff no-such-ref...HEAD failed"),
        "{}",
        r.stderr
    );
}
