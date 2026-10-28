//! Fallback Python interpreter resolution for projects without a
//! `tools/pyproject.toml` (i.e. no tools venv). Resolves
//! `$TOOLR_PYTHON` → `python3` on PATH → `python` on PATH.
//!
//! The primary path resolves Python via the tools venv
//! (`<venv>/bin/python`, see `toolr_core::venv::resolve_venv_path`);
//! this module is the legacy-shape fallback.

use std::env;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PythonError {
    #[error("no Python interpreter found. Set TOOLR_PYTHON or install python3 on PATH")]
    NotFound,
    #[error("TOOLR_PYTHON={0} was not found on PATH. Set it to an absolute interpreter path")]
    ToolrPythonNotOnPath(String),
}

/// Resolve a Python interpreter, in priority order:
///
/// 1. `$TOOLR_PYTHON` if set. A bare name is looked up on PATH; any other
///    path is returned unchanged.
/// 2. `python3` on PATH.
/// 3. `python` on PATH.
///
/// PATH hits come back absolute, so callers can `is_file()` them.
pub fn resolve_python() -> Result<PathBuf, PythonError> {
    let cwd = env::current_dir().unwrap_or_default();
    resolve_python_in(env::var_os("TOOLR_PYTHON"), env::var_os("PATH"), &cwd)
}

fn resolve_python_in(
    toolr_python: Option<OsString>,
    path: Option<OsString>,
    cwd: &Path,
) -> Result<PathBuf, PythonError> {
    if let Some(p) = toolr_python.filter(|p| !p.is_empty()) {
        let p = PathBuf::from(p);
        if !is_bare_name(&p) {
            return Ok(p);
        }
        return which::which_in(&p, path, cwd)
            .map_err(|_| PythonError::ToolrPythonNotOnPath(p.display().to_string()));
    }
    for candidate in ["python3", "python"] {
        if let Ok(found) = which::which_in(candidate, path.as_ref(), cwd) {
            if runs(&found) {
                return Ok(found);
            }
        }
    }
    Err(PythonError::NotFound)
}

fn is_bare_name(p: &Path) -> bool {
    let mut components = p.components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

/// Spawn `<exe> --version` and see if it succeeds. Weeds out PATH entries
/// that exist but don't run, such as the Windows Store `python3.exe` alias.
fn runs(exe: &Path) -> bool {
    Command::new(exe)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialises tests that mutate `TOOLR_PYTHON`; cargo runs tests in
    /// parallel and env vars are process-wide.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn toolr_python_env_var_wins() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: ENV_LOCK serialises every test in this module that
        // touches `TOOLR_PYTHON`, and no other thread in the crate
        // mutates that env var.
        unsafe {
            env::set_var("TOOLR_PYTHON", "/custom/python");
        }
        let p = resolve_python().expect("should resolve");
        assert_eq!(p, PathBuf::from("/custom/python"));
        unsafe {
            env::remove_var("TOOLR_PYTHON");
        }
    }

    #[test]
    #[cfg(unix)]
    fn empty_toolr_python_falls_through_to_path_lookup() {
        let bin = tempfile::TempDir::new().unwrap();
        let stub = stub_python(bin.path(), "python3", 0);
        let cwd = tempfile::TempDir::new().unwrap();
        let p = resolve_python_in(Some("".into()), Some(bin.path().into()), cwd.path()).unwrap();
        assert_eq!(p, stub);
    }

    #[test]
    #[cfg(unix)]
    fn path_candidate_that_does_not_run_is_skipped() {
        let bin = tempfile::TempDir::new().unwrap();
        stub_python(bin.path(), "python3", 1);
        let python = stub_python(bin.path(), "python", 0);
        let cwd = tempfile::TempDir::new().unwrap();
        let p = resolve_python_in(None, Some(bin.path().into()), cwd.path()).unwrap();
        assert_eq!(p, python);
    }

    #[test]
    fn no_python_on_path_is_not_found() {
        let bin = tempfile::TempDir::new().unwrap();
        let cwd = tempfile::TempDir::new().unwrap();
        let err = resolve_python_in(None, Some(bin.path().into()), cwd.path()).unwrap_err();
        assert!(matches!(err, PythonError::NotFound), "{err}");
    }

    #[test]
    fn runs_is_false_for_missing_binary() {
        assert!(!runs(Path::new("definitely-not-a-real-binary-toolr-test")));
    }

    #[cfg(unix)]
    fn stub_python(dir: &Path, name: &str, exit_code: u8) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\nexit {exit_code}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    #[cfg(unix)]
    fn path_candidate_resolves_to_absolute_path() {
        let bin = tempfile::TempDir::new().unwrap();
        let stub = stub_python(bin.path(), "python3", 0);
        let cwd = tempfile::TempDir::new().unwrap();
        let p = resolve_python_in(None, Some(bin.path().into()), cwd.path()).unwrap();
        assert_eq!(p, stub);
    }

    #[test]
    #[cfg(unix)]
    fn bare_toolr_python_resolves_against_path() {
        let bin = tempfile::TempDir::new().unwrap();
        let stub = stub_python(bin.path(), "python3.12", 0);
        let cwd = tempfile::TempDir::new().unwrap();
        let p = resolve_python_in(
            Some("python3.12".into()),
            Some(bin.path().into()),
            cwd.path(),
        )
        .unwrap();
        assert_eq!(p, stub);
    }

    #[test]
    fn bare_toolr_python_missing_from_path_names_the_value() {
        let bin = tempfile::TempDir::new().unwrap();
        let cwd = tempfile::TempDir::new().unwrap();
        let err = resolve_python_in(
            Some("no-such-python-toolr".into()),
            Some(bin.path().into()),
            cwd.path(),
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("TOOLR_PYTHON=no-such-python-toolr"),
            "{err}"
        );
    }

    #[test]
    fn absolute_toolr_python_is_returned_unchanged() {
        let cwd = tempfile::TempDir::new().unwrap();
        let p = resolve_python_in(Some("/custom/python".into()), None, cwd.path()).unwrap();
        assert_eq!(p, PathBuf::from("/custom/python"));
    }

    #[test]
    fn python_error_not_found_display_mentions_toolr_python_env() {
        let s = PythonError::NotFound.to_string();
        assert!(s.contains("TOOLR_PYTHON"));
        assert!(s.contains("python3"));
    }
}
