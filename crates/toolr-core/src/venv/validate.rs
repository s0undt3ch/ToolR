//! Post-sync validation: the venv must contain the toolr Python package.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::layout::{SitePackagesError, preferred_site_packages_dirs};

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error(
        "toolr: tools/pyproject.toml must declare a `toolr-py>=X.Y` dependency. \
         Add it and retry."
    )]
    ToolrPackageMissing,
    #[error("toolr: venv at {0} does not contain a python interpreter")]
    InterpreterMissing(PathBuf),
    #[error("toolr: I/O error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl From<SitePackagesError> for ValidationError {
    fn from(e: SitePackagesError) -> Self {
        Self::Io {
            path: e.path,
            source: e.source,
        }
    }
}

/// Look in the venv's `site-packages` directories, its own Python's first (see
/// [`preferred_site_packages_dirs`]), for the toolr package directory. Returns its path on success.
///
/// Handles both regular installs (package directory in site-packages) and
/// editable/direct-url installs where a `.pth` file adds the source root to
/// `sys.path` (as maturin's editable mode does).
pub fn locate_toolr_package(venv_dir: &Path) -> Result<Option<PathBuf>, SitePackagesError> {
    for site_packages in preferred_site_packages_dirs(venv_dir)? {
        // Regular (non-editable) install: toolr/__init__.py is in site-packages.
        let init = site_packages.join("toolr").join("__init__.py");
        if init.is_file() {
            return Ok(Some(site_packages.join("toolr")));
        }
        // Editable / direct-url install: a .pth file adds the source root to
        // sys.path. Walk every .pth file and check if any pointed-to directory
        // contains toolr/__init__.py.
        if let Some(path) = locate_via_pth_files(&site_packages) {
            return Ok(Some(path));
        }
    }
    Ok(None)
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

/// The `toolr-py` version installed in the venv, read from its
/// `toolr_py-<version>.dist-info` directory name. Lists `site-packages`
/// only: no file reads, no interpreter.
pub fn installed_toolr_py_version(venv_dir: &Path) -> Result<Option<String>, SitePackagesError> {
    Ok(preferred_site_packages_dirs(venv_dir)?
        .iter()
        .find_map(|sp| {
            fs::read_dir(sp)
                .ok()?
                .filter_map(Result::ok)
                .find_map(|entry| {
                    let name = entry.file_name();
                    let version = name
                        .to_str()?
                        .strip_prefix("toolr_py-")?
                        .strip_suffix(".dist-info")?;
                    Some(version.to_string())
                })
        }))
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
    locate_toolr_package(venv_dir)?.ok_or(ValidationError::ToolrPackageMissing)
}

#[cfg(test)]
mod tests;
