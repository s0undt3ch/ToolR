//! Where a venv keeps its `site-packages`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// A directory on the way to `site-packages` exists but can't be read.
#[derive(Debug, Error)]
#[error("I/O error reading {path}: {source}")]
pub struct SitePackagesError {
    pub path: PathBuf,
    #[source]
    pub source: io::Error,
}

/// Existing `site-packages` directories in `venv_dir`, sorted: `lib/python*/site-packages` and
/// `Lib/site-packages`, on every platform.
///
/// Lists directories rather than globbing, so nothing in `venv_dir` is read as a pattern. A
/// missing venv or `lib` gives an empty list; one that exists but can't be read is an error, so
/// an unreadable venv never looks like a venv with nothing installed.
pub fn site_packages_dirs(venv_dir: &Path) -> Result<Vec<PathBuf>, SitePackagesError> {
    let mut out = Vec::new();
    for lib in entries(venv_dir)? {
        if !name_is(&lib, |n| n.eq_ignore_ascii_case("lib")) || !lib.is_dir() {
            continue;
        }
        push_if_dir(&mut out, lib.join("site-packages"))?;
        for child in entries(&lib)? {
            if name_is(&child, |n| n.starts_with("python")) && child.is_dir() {
                push_if_dir(&mut out, child.join("site-packages"))?;
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// Paths of the entries in `dir`; none when it doesn't exist.
fn entries(dir: &Path) -> Result<Vec<PathBuf>, SitePackagesError> {
    match fs::read_dir(dir) {
        Ok(read) => Ok(read.filter_map(Result::ok).map(|e| e.path()).collect()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(source) => Err(SitePackagesError {
            path: dir.to_path_buf(),
            source,
        }),
    }
}

/// Push `dir` when it is a directory; an error other than "not found" means a parent can't be
/// searched, which must not read as "nothing installed".
fn push_if_dir(out: &mut Vec<PathBuf>, dir: PathBuf) -> Result<(), SitePackagesError> {
    match fs::metadata(&dir) {
        Ok(meta) => {
            if meta.is_dir() {
                out.push(dir);
            }
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(SitePackagesError { path: dir, source }),
    }
}

fn name_is(path: &Path, pred: impl Fn(&str) -> bool) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(pred)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn mkdirs(root: &Path, rel: &str) -> PathBuf {
        let dir = root.join(rel);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn site_packages_dirs_finds_unix_layout() {
        let tmp = TempDir::new().unwrap();
        let sp = mkdirs(tmp.path(), "lib/python3.13/site-packages");
        assert_eq!(site_packages_dirs(tmp.path()).unwrap(), [sp]);
    }

    #[test]
    fn site_packages_dirs_finds_windows_layout() {
        let tmp = TempDir::new().unwrap();
        let sp = mkdirs(tmp.path(), "Lib/site-packages");
        assert_eq!(site_packages_dirs(tmp.path()).unwrap(), [sp]);
    }

    #[test]
    fn site_packages_dirs_finds_free_threaded_layout() {
        let tmp = TempDir::new().unwrap();
        let sp = mkdirs(tmp.path(), "lib/python3.14t/site-packages");
        assert_eq!(site_packages_dirs(tmp.path()).unwrap(), [sp]);
    }

    // Needs a case-sensitive file system to hold both `lib` and `Lib`.
    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn site_packages_dirs_returns_both_layouts_sorted() {
        let tmp = TempDir::new().unwrap();
        let py313 = mkdirs(tmp.path(), "lib/python3.13/site-packages");
        let win = mkdirs(tmp.path(), "Lib/site-packages");
        let py312 = mkdirs(tmp.path(), "lib/python3.12/site-packages");
        assert_eq!(site_packages_dirs(tmp.path()).unwrap(), [win, py312, py313]);
    }

    #[test]
    fn site_packages_dirs_returns_every_python_dir_sorted() {
        let tmp = TempDir::new().unwrap();
        let py313 = mkdirs(tmp.path(), "lib/python3.13/site-packages");
        let py312 = mkdirs(tmp.path(), "lib/python3.12/site-packages");
        mkdirs(tmp.path(), "lib/pkgconfig");
        assert_eq!(site_packages_dirs(tmp.path()).unwrap(), [py312, py313]);
    }

    #[test]
    fn site_packages_dirs_matches_lib_ignoring_case() {
        let tmp = TempDir::new().unwrap();
        let sp = mkdirs(tmp.path(), "LIB/site-packages");
        assert_eq!(site_packages_dirs(tmp.path()).unwrap(), [sp]);
    }

    #[test]
    fn site_packages_dirs_ignores_files() {
        let tmp = TempDir::new().unwrap();
        let lib = mkdirs(tmp.path(), "lib/python3.13");
        fs::write(lib.join("site-packages"), "").unwrap();
        fs::write(tmp.path().join("lib").join("site-packages"), "").unwrap();
        fs::write(tmp.path().join("lib").join("python3.12"), "").unwrap();
        assert!(site_packages_dirs(tmp.path()).unwrap().is_empty());

        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Lib"), "").unwrap();
        assert!(site_packages_dirs(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn site_packages_dirs_empty_for_missing_venv() {
        let tmp = TempDir::new().unwrap();
        assert!(
            site_packages_dirs(&tmp.path().join("nope"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn site_packages_dirs_under_bracketed_venv() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join("[wip]").join("venv");
        let sp = mkdirs(&venv, "lib/python3.13/site-packages");
        assert_eq!(site_packages_dirs(&venv).unwrap(), [sp]);
    }

    /// chmod 000 does nothing to root, so these can't fail for it.
    #[cfg(unix)]
    fn is_root() -> bool {
        // SAFETY: geteuid has no preconditions.
        unsafe { libc::geteuid() == 0 }
    }

    #[cfg(unix)]
    fn with_mode<T>(dir: &Path, mode: u32, f: impl FnOnce() -> T) -> T {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(mode)).unwrap();
        let out = f();
        fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).unwrap();
        out
    }

    #[cfg(unix)]
    #[test]
    fn site_packages_dirs_reports_unreadable_lib() {
        if is_root() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        mkdirs(tmp.path(), "lib/python3.13/site-packages");
        let lib = tmp.path().join("lib");
        let err = with_mode(&lib, 0o000, || site_packages_dirs(tmp.path())).unwrap_err();
        assert!(err.path.starts_with(&lib), "{}", err.path.display());
    }

    #[cfg(unix)]
    #[test]
    fn site_packages_dirs_reports_unreadable_venv() {
        if is_root() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join("venv");
        mkdirs(&venv, "lib/python3.13/site-packages");
        let err = with_mode(&venv, 0o000, || site_packages_dirs(&venv)).unwrap_err();
        assert_eq!(err.path, venv);
    }

    #[cfg(unix)]
    #[test]
    fn site_packages_dirs_reports_unsearchable_python_dir() {
        if is_root() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        let sp = mkdirs(tmp.path(), "lib/python3.13/site-packages");
        let py = tmp.path().join("lib/python3.13");
        let err = with_mode(&py, 0o000, || site_packages_dirs(tmp.path())).unwrap_err();
        assert_eq!(err.path, sp);
    }
}
