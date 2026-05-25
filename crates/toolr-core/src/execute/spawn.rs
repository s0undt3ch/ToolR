//! Spawn the Python runner subprocess.

use std::io;
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// Spawn `<python> -m toolr._runner` with:
///
/// - `TOOLR_SPEC_FILE` set to `spec_path`.
/// - stdin/stdout/stderr inherited untouched (so Rich's TTY detection,
///   tools that read stdin, etc., all work).
pub fn spawn_runner(python: &Path, spec_path: &Path) -> io::Result<Child> {
    Command::new(python)
        .arg("-m")
        .arg("toolr._runner")
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
        let result = spawn_runner(&bogus, Path::new("/tmp/whatever.json"));
        assert!(result.is_err());
    }
}
