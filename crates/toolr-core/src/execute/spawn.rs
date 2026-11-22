//! Spawn the Python runner subprocess.

use std::io;
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// The fixed argv (after the interpreter) for the runner.
///
/// `-P` enables safe-path mode (drops the implicit CWD `sys.path` entry); it is
/// a flag, so it is NOT inherited by child processes the command spawns.
fn runner_args() -> [&'static str; 3] {
    ["-P", "-m", "toolr._runner"]
}

/// Spawn `<python> -P -m toolr._runner` with:
///
/// - working directory set to `repo_root`, so the command runs from the
///   project root regardless of where the user invoked toolr (the
///   make/cargo convention). Child processes the command spawns inherit
///   this cwd. The runner itself no longer chdirs.
/// - `TOOLR_SPEC_FILE` set to `spec_path`.
/// - `PYTHONIOENCODING=utf-8`, unless the user set a non-empty value: on
///   Windows CPython before 3.15 would otherwise encode piped stdio with the
///   ANSI code page, which fails on characters such as `…`. Unlike UTF-8
///   mode it leaves `open()`'s default encoding alone. Child Python
///   processes inherit it.
/// - stdin/stdout/stderr inherited untouched (so Rich's TTY detection,
///   tools that read stdin, etc., all work).
pub fn spawn_runner(python: &Path, spec_path: &Path, repo_root: &Path) -> io::Result<Child> {
    runner_command(
        python,
        spec_path,
        repo_root,
        std::env::var_os("PYTHONIOENCODING").is_some_and(|v| !v.is_empty()),
    )
    .spawn()
}

fn runner_command(
    python: &Path,
    spec_path: &Path,
    repo_root: &Path,
    user_set_io_encoding: bool,
) -> Command {
    // nosemgrep: rust.actix.command-injection.rust-actix-command-injection.rust-actix-command-injection
    let mut cmd = Command::new(python);
    cmd.args(runner_args())
        .current_dir(repo_root)
        .env("TOOLR_SPEC_FILE", spec_path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if !user_set_io_encoding {
        cmd.env("PYTHONIOENCODING", "utf-8");
    }
    cmd
}

#[cfg(test)]
mod tests;
