//! `--quiet` never prompts to install uv, on every subcommand that takes it.
//! The prompt needs a terminal on stdin, so each case runs under a pty.

#![cfg(unix)]

mod common;

use std::fs::File;
use std::io::Write;
use std::os::fd::FromRawFd;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use common::VenvFixture;
use tempfile::TempDir;

const PROMPT: &str = "didn't find it on PATH";

/// Run toolr in `cwd` with a pseudo-terminal as stdin, an empty `PATH` and an
/// empty data dir, so uv can't be found and a prompt would be shown.
fn run_with_tty_stdin(cwd: &Path, args: &[&str]) -> Output {
    let empty = TempDir::new().unwrap();
    let (mut master, mut slave) = (0, 0);
    // SAFETY: openpty writes two fds into the out-params; the null pointers
    // ask for no name, termios or winsize.
    let rc = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(rc, 0, "openpty failed");
    // SAFETY: both fds are fresh from openpty and owned only here.
    let (mut master, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
    // A prompt that does fire reads "n" and declines. A bare Enter would mean
    // "install" and download uv.
    master.write_all(b"n\n").unwrap();

    let child = Command::new(assert_cmd::cargo::cargo_bin("toolr"))
        .args(args)
        .current_dir(cwd)
        .env("PATH", empty.path())
        .env("XDG_DATA_HOME", empty.path())
        .env_remove("TOOLR_AUTO_INSTALL_UV")
        .stdin(Stdio::from(slave))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = child.wait_with_output().unwrap();
    drop(master);
    out
}

fn assert_refused_without_prompt(out: &Output) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains(PROMPT),
        "prompted despite --quiet:\n{stderr}"
    );
    assert!(
        stderr.contains("TOOLR_AUTO_INSTALL_UV=1"),
        "missing the uv install hint:\n{stderr}"
    );
    assert!(!out.status.success(), "expected failure:\n{stderr}");
}

#[test]
fn init_quiet_does_not_prompt_for_uv() {
    let tmp = TempDir::new().unwrap();
    let out = run_with_tty_stdin(tmp.path(), &["project", "init", "--quiet"]);
    assert_refused_without_prompt(&out);
}

#[test]
fn venv_lock_quiet_does_not_prompt_for_uv() {
    let fx = VenvFixture::new();
    let out = run_with_tty_stdin(&fx.root, &["project", "venv", "lock", "--quiet"]);
    assert_refused_without_prompt(&out);
}

#[test]
fn venv_add_quiet_does_not_prompt_for_uv() {
    let fx = VenvFixture::new();
    let out = run_with_tty_stdin(&fx.root, &["project", "venv", "add", "--quiet", "rich"]);
    assert_refused_without_prompt(&out);
}

#[test]
fn venv_remove_quiet_does_not_prompt_for_uv() {
    let fx = VenvFixture::new();
    let out = run_with_tty_stdin(
        &fx.root,
        &["project", "venv", "remove", "--quiet", "requests"],
    );
    assert_refused_without_prompt(&out);
}

#[test]
fn venv_run_quiet_does_not_prompt_for_uv() {
    let fx = VenvFixture::new();
    let out = run_with_tty_stdin(
        &fx.root,
        &["project", "venv", "run", "--quiet", "--", "true"],
    );
    assert_refused_without_prompt(&out);
}

#[test]
fn venv_sync_quiet_still_exits_silently() {
    let fx = VenvFixture::new();
    let out = run_with_tty_stdin(&fx.root, &["project", "venv", "sync", "--quiet"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains(PROMPT),
        "prompted despite --quiet:\n{stderr}"
    );
    assert!(out.status.success(), "{stderr}");
}

#[test]
fn without_quiet_the_prompt_still_shows() {
    let fx = VenvFixture::new();
    let out = run_with_tty_stdin(&fx.root, &["project", "venv", "lock"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(PROMPT),
        "the harness didn't reach the prompt:\n{stderr}"
    );
}
