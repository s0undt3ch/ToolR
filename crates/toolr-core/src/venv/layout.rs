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
mod tests {
    use super::*;
    use crate::test_fs::ignores_case;
    #[cfg(unix)]
    use crate::test_fs::with_mode;
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

    /// On a case-insensitive file system `Lib/site-packages` lands in `lib`, and is still listed
    /// once under the `Lib` spelling.
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

    /// An upper-case `LIB`, as some Windows tools write it, is the `Lib` a case-insensitive file
    /// system finds; once, though `lib` names it too. A case-sensitive one has neither.
    #[test]
    fn site_packages_dirs_matches_lib_by_the_file_systems_case_rule() {
        let tmp = TempDir::new().unwrap();
        mkdirs(tmp.path(), "LIB/site-packages");
        let expected: Vec<_> = ignores_case(tmp.path())
            .then(|| tmp.path().join("Lib").join("site-packages"))
            .into_iter()
            .collect();
        assert_eq!(site_packages_dirs(tmp.path()).unwrap(), expected);
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

    #[cfg(unix)]
    #[test]
    fn site_packages_dirs_reports_unreadable_lib() {
        let tmp = TempDir::new().unwrap();
        mkdirs(tmp.path(), "lib/python3.13/site-packages");
        let lib = tmp.path().join("lib");
        let result = with_mode(&lib, 0o000, || site_packages_dirs(tmp.path()));
        // `None` as root.
        assert!(
            result
                .as_ref()
                .is_none_or(|r| r.as_ref().is_err_and(|err| err.path.starts_with(&lib))),
            "{result:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn site_packages_dirs_reports_unreadable_venv() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join("venv");
        mkdirs(&venv, "lib/python3.13/site-packages");
        let result = with_mode(&venv, 0o000, || site_packages_dirs(&venv));
        // `None` as root.
        assert!(
            result
                .as_ref()
                .is_none_or(|r| r.as_ref().is_err_and(|err| err.path.starts_with(&venv))),
            "{result:?}"
        );
    }

    /// A venv root that can be searched but not listed is fine: nothing lists it.
    #[cfg(unix)]
    #[test]
    fn site_packages_dirs_needs_no_listing_of_the_venv_root() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join("venv");
        let sp = mkdirs(&venv, "lib/python3.13/site-packages");
        let result = with_mode(&venv, 0o311, || site_packages_dirs(&venv));
        assert!(
            result
                .as_ref()
                .is_none_or(|r| r.as_ref().is_ok_and(|dirs| *dirs == [sp.clone()])),
            "{result:?}"
        );
    }

    /// Unreadable leftovers from another Python don't hide the venv's own `site-packages`,
    /// with or without `pyvenv.cfg`.
    #[cfg(unix)]
    #[test]
    fn site_packages_dirs_skips_unreadable_stale_python_dirs() {
        let tmp = TempDir::new().unwrap();
        let sp = mkdirs(tmp.path(), "lib/python3.15/site-packages");
        let stale = mkdirs(tmp.path(), "lib/python3.12/site-packages");
        let old = mkdirs(tmp.path(), "lib/python3.15-old/site-packages");
        let cfg = tmp.path().join("pyvenv.cfg");
        fs::write(&cfg, "version_info = 3.15.0\n").unwrap();
        let stale_parent = stale.parent().unwrap();
        let result = with_mode(stale_parent, 0o000, || {
            with_mode(old.parent().unwrap(), 0o000, || {
                let with_cfg = site_packages_dirs(tmp.path());
                fs::remove_file(&cfg).unwrap();
                (with_cfg, site_packages_dirs(tmp.path()))
            })
        })
        .flatten();
        assert!(
            result.as_ref().is_none_or(|(with_cfg, without_cfg)| {
                with_cfg.as_ref().is_ok_and(|d| *d == [sp.clone()])
                    && without_cfg.as_ref().is_ok_and(|d| *d == [sp.clone()])
            }),
            "{result:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn site_packages_dirs_reports_unsearchable_python_dir() {
        let tmp = TempDir::new().unwrap();
        let sp = mkdirs(tmp.path(), "lib/python3.13/site-packages");
        fs::write(tmp.path().join("pyvenv.cfg"), "version_info = 3.13.2\n").unwrap();
        let py = tmp.path().join("lib/python3.13");
        let result = with_mode(&py, 0o000, || site_packages_dirs(tmp.path()));
        // `None` as root.
        assert!(
            result
                .as_ref()
                .is_none_or(|r| r.as_ref().is_err_and(|err| err.path == sp)),
            "{result:?}"
        );
    }

    #[test]
    fn preferred_site_packages_dirs_follows_pyvenv_cfg() {
        let tmp = TempDir::new().unwrap();
        let py39 = mkdirs(tmp.path(), "lib/python3.9/site-packages");
        let py313 = mkdirs(tmp.path(), "lib/python3.13/site-packages");
        fs::write(
            tmp.path().join("pyvenv.cfg"),
            "home = /usr/bin\nimplementation = CPython\nversion_info = 3.9.20\n",
        )
        .unwrap();
        assert_eq!(
            preferred_site_packages_dirs(tmp.path()).unwrap(),
            [py39, py313]
        );
    }

    #[test]
    fn preferred_site_packages_dirs_reads_venv_style_version_key() {
        let tmp = TempDir::new().unwrap();
        let py39 = mkdirs(tmp.path(), "lib/python3.9/site-packages");
        let py313 = mkdirs(tmp.path(), "lib/python3.13/site-packages");
        fs::write(
            tmp.path().join("pyvenv.cfg"),
            "include-system-site-packages = false\nversion = 3.9.1\n",
        )
        .unwrap();
        assert_eq!(
            preferred_site_packages_dirs(tmp.path()).unwrap(),
            [py39, py313]
        );
    }

    /// Without `pyvenv.cfg`, the highest version wins, not the lexicographic first
    /// (`python3.13` < `python3.9`), and the unversioned ones come last.
    #[test]
    fn preferred_site_packages_dirs_falls_back_to_highest_version() {
        let tmp = TempDir::new().unwrap();
        let py39 = mkdirs(tmp.path(), "lib/python3.9/site-packages");
        let py313 = mkdirs(tmp.path(), "lib/python3.13/site-packages");
        let py314t = mkdirs(tmp.path(), "lib/python3.14t/site-packages");
        let plain = mkdirs(tmp.path(), "Lib/site-packages");
        let odd = mkdirs(tmp.path(), "lib/python-x/site-packages");
        fs::write(
            tmp.path().join("pyvenv.cfg"),
            "home = /usr/bin\nversion_info = garbage\n",
        )
        .unwrap();
        assert_eq!(
            preferred_site_packages_dirs(tmp.path()).unwrap(),
            [py314t, py313, py39, plain, odd]
        );
    }
}
