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
/// - stdin/stdout/stderr inherited untouched (so Rich's TTY detection,
///   tools that read stdin, etc., all work).
pub fn spawn_runner(python: &Path, spec_path: &Path, repo_root: &Path) -> io::Result<Child> {
    // nosemgrep: rust.actix.command-injection.rust-actix-command-injection.rust-actix-command-injection
    Command::new(python)
        .args(runner_args())
        .current_dir(repo_root)
        .env("TOOLR_SPEC_FILE", spec_path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn spawn_with_nonexistent_python_returns_io_error() {
        let bogus = PathBuf::from("/definitely/not/a/real/python-binary-xyz");
        let result = spawn_runner(&bogus, Path::new("/tmp/whatever.json"), Path::new("/tmp"));
        assert!(result.is_err());
    }

    #[test]
    fn spawn_runner_passes_safe_path_flag_before_module() {
        // We can't run a real interpreter here; assert the argv we build.
        let args = runner_args();
        assert_eq!(args, ["-P", "-m", "toolr._runner"]);
    }
}
