//! `--quiet` never prompts to install uv, on every subcommand that takes it.
//! The prompt needs a terminal on stdin, so each case runs under a pty.

#![cfg(unix)]

mod common;

use std::path::Path;
use std::process::Output;

use common::VenvFixture;
use tempfile::TempDir;

// Substring of the prompt in `toolr_core::uv::install`. The tests that assert
// the prompt *does* show guard against a reword turning every `!contains`
// check below into a no-op.
const PROMPT: &str = "didn't find it on PATH";

/// Run toolr in `cwd` with a pseudo-terminal as stdin, an empty `PATH` and an
/// empty data dir, so uv can't be found and a prompt would be shown. The empty
/// `PATH` deliberately hides `VenvFixture`'s stub uv.
fn run_with_tty_stdin(cwd: &Path, args: &[&str]) -> Output {
    run_with_tty_stdin_env(cwd, args, &[])
}

/// [`run_with_tty_stdin`] plus extra environment, such as a private trust store.
fn run_with_tty_stdin_env(cwd: &Path, args: &[&str], env: &[(&str, &Path)]) -> Output {
    let empty = TempDir::new().unwrap();
    let mut cmd = common::toolr_std();
    cmd.args(args)
        .current_dir(cwd)
        .env("PATH", empty.path())
        .env("XDG_DATA_HOME", empty.path())
        .env_remove("TOOLR_AUTO_INSTALL_UV")
        .envs(env.iter().copied());
    // A prompt that does fire reads "n" and declines. A bare Enter would mean
    // "install" and download uv.
    common::run_on_pty(cmd, "n\n")
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
    let out = run_with_tty_stdin(tmp.path(), &["--quiet", "project", "init"]);
    assert_refused_without_prompt(&out);
}

#[test]
fn venv_lock_quiet_does_not_prompt_for_uv() {
    let fx = VenvFixture::new();
    let out = run_with_tty_stdin(&fx.root, &["--quiet", "project", "venv", "lock"]);
    assert_refused_without_prompt(&out);
}

#[test]
fn venv_add_quiet_does_not_prompt_for_uv() {
    let fx = VenvFixture::new();
    let out = run_with_tty_stdin(&fx.root, &["--quiet", "project", "venv", "add", "rich"]);
    assert_refused_without_prompt(&out);
}

#[test]
fn venv_remove_quiet_does_not_prompt_for_uv() {
    let fx = VenvFixture::new();
    let out = run_with_tty_stdin(
        &fx.root,
        &["--quiet", "project", "venv", "remove", "requests"],
    );
    assert_refused_without_prompt(&out);
}

#[test]
fn venv_run_quiet_does_not_prompt_for_uv() {
    let fx = VenvFixture::new();
    let out = run_with_tty_stdin(
        &fx.root,
        &["--quiet", "project", "venv", "run", "--", "true"],
    );
    assert_refused_without_prompt(&out);
}

#[test]
fn venv_sync_if_project_exits_silently_when_uv_is_refused() {
    let fx = VenvFixture::new();
    // Without --quiet the same fixture reaches the prompt, so the silent exit
    // below comes from the uv refusal and not another benign failure.
    let loud = run_with_tty_stdin(&fx.root, &["project", "venv", "sync"]);
    assert!(String::from_utf8_lossy(&loud.stderr).contains(PROMPT));

    let out = run_with_tty_stdin(
        &fx.root,
        &["--quiet", "project", "venv", "sync", "--if-project"],
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.is_empty(), "expected no output:\n{stderr}");
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

#[test]
fn root_quiet_before_project_does_not_prompt_for_uv() {
    let fx = VenvFixture::new();
    let out = run_with_tty_stdin(&fx.root, &["-q", "project", "venv", "lock"]);
    assert_refused_without_prompt(&out);
}

#[test]
fn root_quiet_before_project_init_does_not_prompt_for_uv() {
    let tmp = TempDir::new().unwrap();
    let out = run_with_tty_stdin(tmp.path(), &["-q", "project", "init"]);
    assert_refused_without_prompt(&out);
}

#[test]
fn root_quiet_before_project_venv_shell_does_not_prompt_for_uv() {
    let fx = VenvFixture::new();
    let out = run_with_tty_stdin(&fx.root, &["-q", "project", "venv", "shell"]);
    assert_refused_without_prompt(&out);
}

// `-q` only stops the prompt. The silent exit for benign failures is
// `venv sync --if-project`.
#[test]
fn root_quiet_before_project_venv_sync_refuses_without_prompt() {
    let fx = VenvFixture::new();
    let out = run_with_tty_stdin(&fx.root, &["-q", "project", "venv", "sync"]);
    assert_refused_without_prompt(&out);
}

// `project trust` asks to sync, which stands in for the uv prompt; `--quiet`
// declines that sync before uv is looked for, so it succeeds without asking.
fn assert_trust_synced_nothing_without_prompt(out: &Output) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains(PROMPT), "prompted for uv:\n{stderr}");
    assert!(
        !stderr.contains("Sync it now?"),
        "prompted to sync:\n{stderr}"
    );
    assert!(out.status.success(), "{stderr}");
}

#[test]
fn trust_quiet_does_not_prompt_for_uv() {
    let fx = VenvFixture::new();
    let state = TempDir::new().unwrap();
    let out = run_with_tty_stdin_env(
        &fx.root,
        &["--quiet", "project", "trust"],
        &[("XDG_STATE_HOME", state.path())],
    );
    assert_trust_synced_nothing_without_prompt(&out);
}

#[test]
fn root_quiet_before_project_trust_does_not_prompt_for_uv() {
    let fx = VenvFixture::new();
    let state = TempDir::new().unwrap();
    let out = run_with_tty_stdin_env(
        &fx.root,
        &["-q", "project", "trust"],
        &[("XDG_STATE_HOME", state.path())],
    );
    assert_trust_synced_nothing_without_prompt(&out);
}
