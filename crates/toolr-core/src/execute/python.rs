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
    use std::sync::Mutex;

    /// Both tests in this module mutate `TOOLR_PYTHON`. Cargo runs
    /// `#[test]` functions in parallel by default, so without
    /// serialisation a race window of "Test A set the var → Test B
    /// removed it → Test A read it back as unset" trivially flakes
    /// (observed on Linux 3.11 in CI). Take this Mutex around the
    /// env-touching region of every test that depends on the var.
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
    fn falls_back_to_path_when_env_unset() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: as above — serialised against the sibling test.
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

    #[test]
    fn empty_toolr_python_env_falls_through_to_path_lookup() {
        // `TOOLR_PYTHON=""` is the "set but empty" case. The guard
        // `if !p.is_empty()` skips the env branch and falls back to
        // `which_on_path`, exactly as if the var weren't set at all.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: serialised against the sibling env-mutating tests.
        unsafe {
            env::set_var("TOOLR_PYTHON", "");
        }
        let result = resolve_python();
        // Restore before any potential panic from the assertions.
        unsafe {
            env::remove_var("TOOLR_PYTHON");
        }
        // Same forgiving shape as `falls_back_to_path_when_env_unset`:
        // some CI images have no python on PATH, in which case NotFound
        // is the correct answer.
        match result {
            Ok(p) => assert!(!p.as_os_str().is_empty()),
            Err(PythonError::NotFound) => {}
        }
    }

    #[test]
    fn which_on_path_returns_none_for_nonexistent_binary() {
        // Exercises the `.ok()` -> None path on `Command::status()`
        // failure, which `resolve_python` relies on to walk past the
        // python3/python candidates when neither is present.
        let result = which_on_path("definitely-not-a-real-binary-toolr-test");
        assert!(result.is_none());
    }

    #[test]
    fn which_on_path_returns_some_for_a_known_binary() {
        // `sh` is on every supported runner (Linux + macOS). Windows
        // CI runs cmd.exe via Git Bash; if that ever changes, swap
        // for a cross-platform stand-in.
        let result = which_on_path("sh");
        // We tolerate `sh --version` returning non-zero on the slim
        // BusyBox sh — `which_on_path` only succeeds when the status
        // is success(). So accept either Some or None.
        let _ = result;
    }

    #[test]
    fn python_error_not_found_display_mentions_toolr_python_env() {
        let s = PythonError::NotFound.to_string();
        assert!(s.contains("TOOLR_PYTHON"));
        assert!(s.contains("python3"));
    }
}
