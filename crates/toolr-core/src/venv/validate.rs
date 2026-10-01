//! Post-sync validation: the venv must contain the toolr Python package.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;
use walkdir::WalkDir;

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error(
        "toolr: tools/pyproject.toml must declare a `toolr-py>=X.Y` dependency. \
         Add it and retry."
    )]
    ToolrPackageMissing,
    #[error("toolr: venv at {0} does not contain a python interpreter")]
    InterpreterMissing(PathBuf),
}

/// Walk the venv's `lib/python*/site-packages` and look for the toolr
/// package directory. Returns its path on success.
///
/// Handles both regular installs (package directory in site-packages) and
/// editable/direct-url installs where a `.pth` file adds the source root to
/// `sys.path` (as maturin's editable mode does).
pub fn locate_toolr_package(venv_dir: &Path) -> Option<PathBuf> {
    for site_packages in candidate_site_packages(venv_dir) {
        // Regular (non-editable) install: toolr/__init__.py is in site-packages.
        let init = site_packages.join("toolr").join("__init__.py");
        if init.is_file() {
            return Some(site_packages.join("toolr"));
        }
        // Editable / direct-url install: a .pth file adds the source root to
        // sys.path. Walk every .pth file and check if any pointed-to directory
        // contains toolr/__init__.py.
        if let Some(path) = locate_via_pth_files(&site_packages) {
            return Some(path);
        }
    }
    None
}

/// Read every `.pth` file in `site_packages` and return the `toolr` package
/// path if any of them point at a directory that contains one.
fn locate_via_pth_files(site_packages: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(site_packages).ok()?;
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("pth") {
            continue;
        }
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for line in content.lines() {
            let line = line.trim();
            // .pth files may contain comments and `import` statements; skip them.
            if line.is_empty() || line.starts_with('#') || line.starts_with("import ") {
                continue;
            }
            let src = Path::new(line);
            let init = src.join("toolr").join("__init__.py");
            if init.is_file() {
                return Some(src.join("toolr"));
            }
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

/// The `toolr-py` version installed in the venv, read from its
/// `toolr_py-<version>.dist-info` directory name. Lists `site-packages`
/// only: no file reads, no interpreter.
pub fn installed_toolr_py_version(venv_dir: &Path) -> Option<String> {
    candidate_site_packages(venv_dir).iter().find_map(|sp| {
        fs::read_dir(sp).ok()?.filter_map(Result::ok).find_map(|entry| {
            let name = entry.file_name();
            let version = name.to_str()?.strip_prefix("toolr_py-")?.strip_suffix(".dist-info")?;
            Some(version.to_string())
        })
    })
}

/// Order two versions by `major.minor` alone. `None` when either is
/// unparsable, so a warning built on this can't misfire.
pub fn compare_minor_versions(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    fn major_minor(v: &str) -> Option<(u64, u64)> {
        let mut parts = v.split('.');
        Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
    }
    Some(major_minor(a)?.cmp(&major_minor(b)?))
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

    /// maturin editable installs drop a .pth file in site-packages that adds
    /// the source root to sys.path — the toolr package directory is NOT copied
    /// into site-packages itself.
    #[test]
    #[cfg(unix)]
    fn detects_toolr_package_via_pth_file() {
        let tmp = TempDir::new().unwrap();
        let py_dir = tmp.path().join("lib").join("python3.13").join("site-packages");
        std::fs::create_dir_all(&py_dir).unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("python"), b"").unwrap();

        // Simulate the source tree that the .pth file will point at.
        let src_root = tmp.path().join("src");
        std::fs::create_dir_all(src_root.join("toolr")).unwrap();
        std::fs::write(src_root.join("toolr").join("__init__.py"), b"").unwrap();

        // Write the .pth file (as maturin develop would).
        std::fs::write(py_dir.join("toolr_py.pth"), src_root.to_str().unwrap()).unwrap();

        let python = tmp.path().join("bin").join("python");
        let pkg = validate_venv(tmp.path(), &python).unwrap();
        assert!(pkg.ends_with("toolr"), "expected path ending in toolr, got {pkg:?}");
    }

    #[test]
    #[cfg(unix)]
    fn reads_installed_toolr_py_version_from_dist_info() {
        let tmp = TempDir::new().unwrap();
        fake_unix_venv(tmp.path(), true, true);
        let sp = tmp.path().join("lib").join("python3.13").join("site-packages");
        std::fs::create_dir_all(sp.join("toolr_py-0.32.0.dist-info")).unwrap();
        std::fs::create_dir_all(sp.join("toolr_example_plugin-1.0.0.dist-info")).unwrap();
        assert_eq!(installed_toolr_py_version(tmp.path()).as_deref(), Some("0.32.0"));
    }

    #[test]
    #[cfg(unix)]
    fn no_toolr_py_dist_info_means_no_version() {
        let tmp = TempDir::new().unwrap();
        fake_unix_venv(tmp.path(), true, true);
        assert_eq!(installed_toolr_py_version(tmp.path()), None);
    }

    #[test]
    fn minor_version_order_ignores_patch_level() {
        use std::cmp::Ordering::{Equal, Greater, Less};
        assert_eq!(super::compare_minor_versions("0.33.0", "0.33.4"), Some(Equal));
        assert_eq!(super::compare_minor_versions("0.32.0", "0.33.0"), Some(Less));
        assert_eq!(super::compare_minor_versions("1.0.0", "0.33.0"), Some(Greater));
    }

    #[test]
    fn unparsable_versions_have_no_order() {
        assert_eq!(super::compare_minor_versions("garbage", "0.33.0"), None);
        assert_eq!(super::compare_minor_versions("0.33.0", ""), None);
    }
}
