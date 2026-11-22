//! Where a venv keeps its `site-packages`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::pyvenv_cfg::pyvenv_cfg_value;

/// A directory on the way to `site-packages` exists but can't be read.
#[derive(Debug, Error)]
#[error("I/O error reading {path}: {source}")]
pub struct SitePackagesError {
    pub path: PathBuf,
    #[source]
    pub source: io::Error,
}

/// A `major.minor` Python version.
type PyVersion = (u32, u32);

/// Existing `site-packages` directories in `venv_dir`, sorted: `lib/python*/site-packages` and
/// `Lib/site-packages`, on every platform, since a venv can be copied between them.
///
/// It probes those names rather than globbing, so nothing in `venv_dir` is read as a pattern.
/// `site-packages` is matched by its exact name, as uv and `venv` write it. What a directory
/// that can't be read means depends on which one it is:
///
/// - A missing venv or `lib` is an empty list: nothing is installed yet.
/// - The venv root itself is never listed, so one that can be searched but not listed works.
/// - An unreadable `lib`, `Lib/site-packages`, or the `site-packages` of the Python that
///   `pyvenv.cfg` names is an error. Each may hold the venv's packages, and reading it as empty
///   would quietly drop every plugin's commands.
/// - Any other unreadable `python*` directory is skipped: it is left over from an older Python,
///   and a stale directory must not block dispatch. Without `pyvenv.cfg` there is no own Python
///   to tell apart, so every unreadable `python*` directory is skipped.
pub fn site_packages_dirs(venv_dir: &Path) -> Result<Vec<PathBuf>, SitePackagesError> {
    site_packages_dirs_for(venv_dir, pyvenv_cfg_version(venv_dir))
}

/// [`site_packages_dirs`], best match for the venv's own interpreter first: the
/// `pythonX.Y` named by `pyvenv.cfg`, then the rest by descending version, then the
/// unversioned `Lib/site-packages`. For lookups that stop at the first hit, so a stale
/// `python3.12` left next to `python3.13` doesn't win.
pub fn preferred_site_packages_dirs(venv_dir: &Path) -> Result<Vec<PathBuf>, SitePackagesError> {
    let own = pyvenv_cfg_version(venv_dir);
    let mut dirs = site_packages_dirs_for(venv_dir, own)?;
    dirs.sort_by_key(|sp| {
        let version = dir_version(sp);
        std::cmp::Reverse((own.is_some() && version == own, version))
    });
    Ok(dirs)
}

/// [`site_packages_dirs`] for a venv whose own Python is `own`, as read from `pyvenv.cfg`.
fn site_packages_dirs_for(
    venv_dir: &Path,
    own: Option<PyVersion>,
) -> Result<Vec<PathBuf>, SitePackagesError> {
    let mut out = Vec::new();

    let lib = venv_dir.join("lib");
    if is_dir(&lib)? {
        for child in entries(&lib)? {
            let is_python = child
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("python"));
            if !is_python || !child.is_dir() {
                continue;
            }
            let site_packages = child.join("site-packages");
            match is_dir(&site_packages) {
                Ok(true) => out.push(site_packages),
                Ok(false) => {}
                Err(e) if must_read(&site_packages, own) => return Err(e),
                Err(_) => {}
            }
        }
    }

    // The Windows layout, and the same without a `pythonX.Y` level. On a case-insensitive file
    // system both names are one directory, kept once under the first spelling.
    for site_packages in [
        venv_dir.join("Lib").join("site-packages"),
        lib.join("site-packages"),
    ] {
        if is_dir(&site_packages)? && !out.iter().any(|seen| same_dir(seen, &site_packages)) {
            out.push(site_packages);
        }
    }

    out.sort();
    out.dedup();
    Ok(out)
}

/// Whether an unreadable `pythonX.Y/site-packages` is an error rather than a stale directory to
/// skip: only when it belongs to the Python that `pyvenv.cfg` names.
fn must_read(site_packages: &Path, own: Option<PyVersion>) -> bool {
    own.is_some() && dir_version(site_packages) == own
}

/// The `major.minor` in a `.../pythonX.Y/site-packages` path.
fn dir_version(site_packages: &Path) -> Option<PyVersion> {
    site_packages
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix("python"))
        .and_then(parse_major_minor)
}

/// `major.minor` of the venv's interpreter from `pyvenv.cfg`: `version_info`, as uv writes it,
/// or `version`, as `venv` does.
fn pyvenv_cfg_version(venv_dir: &Path) -> Option<PyVersion> {
    pyvenv_cfg_value(venv_dir, "version_info")
        .or_else(|| pyvenv_cfg_value(venv_dir, "version"))
        .as_deref()
        .and_then(parse_major_minor)
}

/// `3.13` from `3.13`, `3.13.1` or `3.14t`.
fn parse_major_minor(version: &str) -> Option<PyVersion> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.trim_end_matches('t').parse().ok()?;
    Some((major, minor))
}

/// Paths of the entries in `dir`, which exists.
fn entries(dir: &Path) -> Result<Vec<PathBuf>, SitePackagesError> {
    fs::read_dir(dir)
        .map(|read| read.filter_map(Result::ok).map(|e| e.path()).collect())
        .map_err(|source| SitePackagesError {
            path: dir.to_path_buf(),
            source,
        })
}

/// Whether `dir` is a directory; an error other than "not found" means a parent can't be
/// searched, which must not read as "nothing installed".
fn is_dir(dir: &Path) -> Result<bool, SitePackagesError> {
    match fs::metadata(dir) {
        Ok(meta) => Ok(meta.is_dir()),
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(false)
        }
        Err(source) => Err(SitePackagesError {
            path: dir.to_path_buf(),
            source,
        }),
    }
}

/// Whether two existing paths are one directory.
#[cfg(unix)]
fn same_dir(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let id = |p: &Path| fs::metadata(p).ok().map(|m| (m.dev(), m.ino()));
    id(a) == id(b)
}

/// Whether two existing paths are one directory: Windows file systems ignore case.
#[cfg(not(unix))]
fn same_dir(a: &Path, b: &Path) -> bool {
    a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
}

#[cfg(test)]
mod tests;
