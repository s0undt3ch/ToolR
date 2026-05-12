//! Resolve a Python interpreter to use for `python -m toolr._runner`.
//!
//! Plan 2 ships the minimal viable lookup. Plan 3 replaces this with a
//! resolved tools-venv interpreter under `<venv>/bin/python`.

use std::env;
use std::path::PathBuf;
use std::process::Command;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PythonError {
    #[error("no Python interpreter found. Set TOOLR_PYTHON or install python3 on PATH")]
    NotFound,
}

/// Resolve a Python interpreter, in priority order:
///
/// 1. `$TOOLR_PYTHON` if set.
/// 2. `python3` on PATH.
/// 3. `python` on PATH.
pub fn resolve_python() -> Result<PathBuf, PythonError> {
    if let Ok(p) = env::var("TOOLR_PYTHON") {
        if !p.is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    for candidate in ["python3", "python"] {
        if which_on_path(candidate).is_some() {
            return Ok(PathBuf::from(candidate));
        }
    }
    Err(PythonError::NotFound)
}

/// Cheap PATH check: spawn `<exe> --version` and see if it runs.
/// (We avoid a `which` crate dep for one call site.)
fn which_on_path(exe: &str) -> Option<()> {
    Command::new(exe)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .status()
        .ok()
        .filter(|s| s.success())
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolr_python_env_var_wins() {
        // SAFETY: tests run in-process; we restore after.
        // SAFETY: std::env::set_var is single-threaded-safe inside a #[test]
        // when no other thread touches the environment. This crate's tests
        // don't spawn threads that touch env.
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
    fn falls_back_to_path_when_env_unset() {
        unsafe {
            env::remove_var("TOOLR_PYTHON");
        }
        // We can't assert a specific path without making the test brittle.
        // We only check that *if* python3/python is available, we get
        // a non-empty path back, *or* we get NotFound.
        match resolve_python() {
            Ok(p) => assert!(!p.as_os_str().is_empty()),
            Err(PythonError::NotFound) => {
                // Acceptable on systems without any python on PATH.
            }
        }
    }
}
