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
    #[error(
        "{path} can't run toolr: {reason}. toolr needs Python 3.{MIN_PYTHON_MINOR}+; \
         set TOOLR_PYTHON to a supported interpreter"
    )]
    Unsupported { path: String, reason: String },
}

/// Oldest Python 3 minor toolr supports. Keep in step with `requires-python`
/// in `crates/toolr-py/pyproject.toml`; the runner also spawns with `-P` (3.11+).
const MIN_PYTHON_MINOR: u32 = 11;

/// Resolve a Python interpreter, in priority order:
///
/// 1. `$TOOLR_PYTHON` if set. A bare name is looked up on PATH; any other
///    path is returned unchanged.
/// 2. `python3` on PATH.
/// 3. `python` on PATH.
///
/// PATH hits come back absolute, so callers can `is_file()` them, and must
/// meet [`MIN_PYTHON_MINOR`]. An absolute `$TOOLR_PYTHON` is trusted as is.
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
        let found = which::which_in(&p, path, cwd)
            .map_err(|_| PythonError::ToolrPythonNotOnPath(p.display().to_string()))?;
        return probe(&found).map(|()| found);
    }
    let mut rejected = None;
    for candidate in ["python3", "python"] {
        if let Ok(found) = which::which_in(candidate, path.as_ref(), cwd) {
            match probe(&found) {
                Ok(()) => return Ok(found),
                Err(e) => {
                    rejected.get_or_insert(e);
                }
            }
        }
    }
    Err(rejected.unwrap_or(PythonError::NotFound))
}

fn is_bare_name(p: &Path) -> bool {
    let mut components = p.components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

/// Run `<exe> --version` and check it is a Python toolr supports. Also weeds
/// out PATH entries that exist but don't run, such as the Windows Store alias.
fn probe(exe: &Path) -> Result<(), PythonError> {
    let unsupported = |reason: String| PythonError::Unsupported {
        path: exe.display().to_string(),
        reason,
    };
    let output = Command::new(exe)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| unsupported(format!("it does not run ({e})")))?;
    if !output.status.success() {
        return Err(unsupported("`--version` failed".to_string()));
    }
    // Python 2 printed its version to stderr.
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    match parse_minor(&text) {
        Some(minor) if minor >= MIN_PYTHON_MINOR => Ok(()),
        _ => Err(unsupported(format!("it reports {:?}", text.trim()))),
    }
}

fn parse_minor(version_output: &str) -> Option<u32> {
    let rest = version_output.trim().strip_prefix("Python 3.")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Guards the one test that sets `TOOLR_PYTHON` to cover the env read in
    /// `resolve_python`. Prefer `resolve_python_in` for anything else.
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
    fn path_candidate_that_does_not_run_is_skipped() {
        let bin = tempfile::TempDir::new().unwrap();
        stub_python(bin.path(), "python3", None);
        let python = stub_python(bin.path(), "python", Some("3.12.0"));
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
    fn probe_rejects_missing_binary() {
        assert!(probe(Path::new("definitely-not-a-real-binary-toolr-test")).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn too_old_python3_is_skipped_for_a_supported_python() {
        let bin = tempfile::TempDir::new().unwrap();
        stub_python(bin.path(), "python3", Some("3.9.6"));
        let python = stub_python(bin.path(), "python", Some("3.11.4"));
        let cwd = tempfile::TempDir::new().unwrap();
        let p = resolve_python_in(None, Some(bin.path().into()), cwd.path()).unwrap();
        assert_eq!(p, python);
    }

    #[test]
    #[cfg(unix)]
    fn only_too_old_python_on_path_is_unsupported() {
        let bin = tempfile::TempDir::new().unwrap();
        stub_python(bin.path(), "python3", Some("3.9.6"));
        let cwd = tempfile::TempDir::new().unwrap();
        let err = resolve_python_in(None, Some(bin.path().into()), cwd.path()).unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, PythonError::Unsupported { .. }), "{msg}");
        assert!(msg.contains("3.9.6") && msg.contains("3.11"), "{msg}");
    }

    #[test]
    #[cfg(unix)]
    fn bare_toolr_python_too_old_is_unsupported() {
        let bin = tempfile::TempDir::new().unwrap();
        stub_python(bin.path(), "python3", Some("3.10.12"));
        let cwd = tempfile::TempDir::new().unwrap();
        let err = resolve_python_in(Some("python3".into()), Some(bin.path().into()), cwd.path())
            .unwrap_err();
        assert!(matches!(err, PythonError::Unsupported { .. }), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn bare_toolr_python_that_does_not_run_is_unsupported() {
        let bin = tempfile::TempDir::new().unwrap();
        stub_python(bin.path(), "python3", None);
        let cwd = tempfile::TempDir::new().unwrap();
        let err = resolve_python_in(Some("python3".into()), Some(bin.path().into()), cwd.path())
            .unwrap_err();
        assert!(matches!(err, PythonError::Unsupported { .. }), "{err}");
    }

    #[test]
    fn parse_minor_reads_python_3_versions_only() {
        assert_eq!(parse_minor("Python 3.11.4\n"), Some(11));
        assert_eq!(parse_minor("Python 3.13.0rc1"), Some(13));
        assert_eq!(parse_minor("Python 2.7.18"), None);
        assert_eq!(parse_minor("garbage"), None);
    }

    #[cfg(unix)]
    /// `version: None` makes a stub whose `--version` fails.
    fn stub_python(dir: &Path, name: &str, version: Option<&str>) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        let body = match version {
            Some(v) => format!("#!/bin/sh\necho 'Python {v}'\n"),
            None => "#!/bin/sh\nexit 1\n".to_string(),
        };
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    #[cfg(unix)]
    fn path_candidate_resolves_to_absolute_path() {
        let bin = tempfile::TempDir::new().unwrap();
        let stub = stub_python(bin.path(), "python3", Some("3.12.0"));
        let cwd = tempfile::TempDir::new().unwrap();
        // An empty `TOOLR_PYTHON` counts as unset.
        for toolr_python in [None, Some("".into())] {
            let p = resolve_python_in(toolr_python, Some(bin.path().into()), cwd.path()).unwrap();
            assert_eq!(p, stub);
        }
    }

    #[test]
    #[cfg(unix)]
    fn bare_toolr_python_resolves_against_path() {
        let bin = tempfile::TempDir::new().unwrap();
        let stub = stub_python(bin.path(), "python3.12", Some("3.12.0"));
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
