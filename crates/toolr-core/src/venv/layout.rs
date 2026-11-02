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
/// Probes those names rather than globbing, so nothing in `venv_dir` is read as a pattern, and
/// never lists the venv root itself. A missing venv or `lib` gives an empty list. An unreadable
/// `lib`, `Lib/site-packages`, or `site-packages` of the Python named by `pyvenv.cfg` is an error,
/// so an unreadable venv never looks like one with nothing installed. Another `python*`
/// directory that can't be read, such as one left by an older Python, is skipped.
pub fn site_packages_dirs(venv_dir: &Path) -> Result<Vec<PathBuf>, SitePackagesError> {
    let own = pyvenv_cfg_version(venv_dir);
    let mut out = Vec::new();

    let lib = venv_dir.join("lib");
    if is_dir(&lib)? {
        for child in entries(&lib)? {
            if !name_is(&child, |n| n.starts_with("python")) || !child.is_dir() {
                continue;
            }
            match push_if_dir(&mut out, child.join("site-packages")) {
                Err(e) if own.is_none() || dir_version(&e.path) != own => {}
                result => {
                    result?;
                }
            }
        }
    }

    // The Windows layout, and the same without a `pythonX.Y` level. On a case-insensitive file
    // system both names are one directory, kept once under the first spelling.
    let upper = venv_dir.join("Lib").join("site-packages");
    let lower = venv_dir.join("lib").join("site-packages");
    let had_upper = push_if_dir(&mut out, upper.clone())?;
    if !(had_upper && is_dir(&lower)? && same_dir(&upper, &lower)) {
        push_if_dir(&mut out, lower)?;
    }

    out.sort();
    out.dedup();
    Ok(out)
}

/// [`site_packages_dirs`], best match for the venv's own interpreter first: the
/// `pythonX.Y` named by `pyvenv.cfg`, then the rest by descending version, then the
/// unversioned `Lib/site-packages`. For lookups that stop at the first hit, so a stale
/// `python3.12` left next to `python3.13` doesn't win.
pub fn preferred_site_packages_dirs(venv_dir: &Path) -> Result<Vec<PathBuf>, SitePackagesError> {
    let own = pyvenv_cfg_version(venv_dir);
    let mut dirs = site_packages_dirs(venv_dir)?;
    dirs.sort_by_key(|sp| {
        let version = dir_version(sp);
        std::cmp::Reverse((own.is_some() && version == own, version))
    });
    Ok(dirs)
}

/// The `major.minor` in a `.../pythonX.Y/site-packages` path.
fn dir_version(site_packages: &Path) -> Option<(u32, u32)> {
    site_packages
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix("python"))
        .and_then(parse_major_minor)
}

/// `major.minor` of the venv's interpreter from `pyvenv.cfg` (`version_info`, as uv writes it,
/// or `version`, as `venv` does).
fn pyvenv_cfg_version(venv_dir: &Path) -> Option<(u32, u32)> {
    let cfg = fs::read_to_string(venv_dir.join("pyvenv.cfg")).ok()?;
    cfg.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        matches!(key.trim(), "version_info" | "version")
            .then(|| parse_major_minor(value.trim()))
            .flatten()
    })
}

/// `3.13` from `3.13`, `3.13.1` or `3.14t`.
fn parse_major_minor(version: &str) -> Option<(u32, u32)> {
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

/// Push `dir` when it is a directory, and say whether it was.
fn push_if_dir(out: &mut Vec<PathBuf>, dir: PathBuf) -> Result<bool, SitePackagesError> {
    let found = is_dir(&dir)?;
    if found {
        out.push(dir);
    }
    Ok(found)
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
fn same_dir(_: &Path, _: &Path) -> bool {
    true
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

    /// An upper-case `LIB`, as some Windows tools write it, is the `Lib` a case-insensitive file
    /// system finds; once, though `lib` names it too.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn site_packages_dirs_matches_lib_ignoring_case() {
        let tmp = TempDir::new().unwrap();
        mkdirs(tmp.path(), "LIB/site-packages");
        assert_eq!(
            site_packages_dirs(tmp.path()).unwrap(),
            [tmp.path().join("Lib").join("site-packages")]
        );
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

    /// Run `f` with `dir` at `mode`. `None` as root, which chmod 000 doesn't stop.
    #[cfg(unix)]
    fn with_mode<T>(dir: &Path, mode: u32, f: impl FnOnce() -> T) -> Option<T> {
        use std::os::unix::fs::PermissionsExt;
        // SAFETY: geteuid has no preconditions.
        let is_root = unsafe { libc::geteuid() == 0 };
        (!is_root).then(|| {
            fs::set_permissions(dir, fs::Permissions::from_mode(mode)).unwrap();
            let out = f();
            fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).unwrap();
            out
        })
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
