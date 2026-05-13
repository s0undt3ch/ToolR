//! Post-sync validation: the venv must contain the toolr Python package.

use std::path::{Path, PathBuf};

use thiserror::Error;
use walkdir::WalkDir;

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error(
        "toolr: tools/pyproject.toml must declare a `toolr>=X.Y` dependency. \
         Add it and retry."
    )]
    ToolrPackageMissing,
    #[error("toolr: venv at {0} does not contain a python interpreter")]
    InterpreterMissing(PathBuf),
}

/// Walk the venv's `lib/python*/site-packages` and look for the toolr
/// package directory. Returns its path on success.
pub fn locate_toolr_package(venv_dir: &Path) -> Option<PathBuf> {
    for candidate in candidate_site_packages(venv_dir) {
        let init = candidate.join("toolr").join("__init__.py");
        if init.is_file() {
            return Some(candidate.join("toolr"));
        }
    }
    None
}

/// Iterate possible `site-packages` directories within a venv.
/// Linux/macOS: `<venv>/lib/python*/site-packages/`.
/// Windows: `<venv>/Lib/site-packages/`.
pub fn candidate_site_packages(venv_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if cfg!(windows) {
        out.push(venv_dir.join("Lib").join("site-packages"));
    } else {
        let lib = venv_dir.join("lib");
        if lib.is_dir() {
            for entry in WalkDir::new(&lib).max_depth(1).into_iter().filter_map(|e| e.ok()) {
                let name = entry.file_name().to_string_lossy();
                if name.starts_with("python") {
                    out.push(entry.path().join("site-packages"));
                }
            }
        }
    }
    out
}

/// Validate the venv has both a python interpreter and the toolr package.
pub fn validate_venv(venv_dir: &Path, python: &Path) -> Result<PathBuf, ValidationError> {
    if !python.is_file() {
        return Err(ValidationError::InterpreterMissing(venv_dir.to_path_buf()));
    }
    locate_toolr_package(venv_dir).ok_or(ValidationError::ToolrPackageMissing)
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::*;
    #[cfg(unix)]
    use tempfile::TempDir;

    #[cfg(unix)]
    fn fake_unix_venv(root: &Path, with_toolr: bool, with_python: bool) {
        let py_dir = root.join("lib").join("python3.13").join("site-packages");
        std::fs::create_dir_all(&py_dir).unwrap();
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        if with_python {
            std::fs::write(bin.join("python"), b"").unwrap();
        }
        if with_toolr {
            std::fs::create_dir_all(py_dir.join("toolr")).unwrap();
            std::fs::write(py_dir.join("toolr").join("__init__.py"), b"").unwrap();
        }
    }

    #[test]
    #[cfg(unix)]
    fn detects_installed_toolr_package() {
        let tmp = TempDir::new().unwrap();
        fake_unix_venv(tmp.path(), true, true);
        let python = tmp.path().join("bin").join("python");
        let pkg = validate_venv(tmp.path(), &python).unwrap();
        assert!(pkg.ends_with("toolr"));
    }

    #[test]
    #[cfg(unix)]
    fn reports_missing_toolr_package() {
        let tmp = TempDir::new().unwrap();
        fake_unix_venv(tmp.path(), false, true);
        let python = tmp.path().join("bin").join("python");
        let err = validate_venv(tmp.path(), &python).unwrap_err();
        assert!(matches!(err, ValidationError::ToolrPackageMissing));
    }

    #[test]
    #[cfg(unix)]
    fn reports_missing_interpreter() {
        let tmp = TempDir::new().unwrap();
        fake_unix_venv(tmp.path(), true, false);
        let python = tmp.path().join("bin").join("python");
        let err = validate_venv(tmp.path(), &python).unwrap_err();
        assert!(matches!(err, ValidationError::InterpreterMissing(_)));
    }
}
