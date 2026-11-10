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

    #[test]
    fn runner_command_leaves_a_user_set_io_encoding_alone() {
        let cmd = runner_command(
            Path::new("python"),
            Path::new("spec.json"),
            Path::new("."),
            true,
        );
        assert!(cmd.get_envs().all(|(k, _)| k != "PYTHONIOENCODING"));
    }

    /// Runs a stand-in interpreter that records `PYTHONIOENCODING` as it sees it.
    #[cfg(unix)]
    fn io_encoding_seen_by_child(user_value: Option<&str>) -> String {
        use crate::test_env::EnvGuard;
        use std::os::unix::fs::PermissionsExt;

        let mut env = EnvGuard::write();
        match user_value {
            Some(v) => env.set("PYTHONIOENCODING", v),
            None => env.remove("PYTHONIOENCODING"),
        }
        let dir = tempfile::TempDir::new().unwrap();
        let python = dir.path().join("fake-python");
        std::fs::write(
            &python,
            "#!/bin/sh\nprintf '%s' \"${PYTHONIOENCODING-unset}\" > \"$TOOLR_SPEC_FILE.out\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).unwrap();
        let spec = dir.path().join("spec.json");
        let mut child = spawn_runner(&python, &spec, dir.path()).unwrap();
        assert!(child.wait().unwrap().success());
        std::fs::read_to_string(dir.path().join("spec.json.out")).unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn spawned_runner_sees_utf8_stdio_unless_the_user_chose() {
        assert_eq!(io_encoding_seen_by_child(None), "utf-8");
        assert_eq!(io_encoding_seen_by_child(Some("latin-1")), "latin-1");
        assert_eq!(io_encoding_seen_by_child(Some("")), "utf-8");
    }
}
