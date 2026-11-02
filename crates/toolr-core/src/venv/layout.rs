//! Where a venv keeps its `site-packages`.

use std::fs;
use std::path::{Path, PathBuf};

/// Existing `site-packages` directories in `venv_dir`, sorted: `lib/python*/site-packages` and
/// `Lib/site-packages`, on every platform.
///
/// Lists directories rather than globbing, so nothing in `venv_dir` is read as a pattern. An
/// unreadable or missing venv gives an empty list.
pub fn site_packages_dirs(venv_dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = entries(venv_dir)
        .filter(|lib| name_is(lib, |n| n.eq_ignore_ascii_case("lib")))
        .flat_map(|lib| {
            let pythons = entries(&lib).filter(|child| name_is(child, |n| n.starts_with("python")));
            std::iter::once(lib).chain(pythons)
        })
        .map(|dir| dir.join("site-packages"))
        .filter(|sp| sp.is_dir())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Paths of the entries in `dir`; none when it can't be read.
fn entries(dir: &Path) -> impl Iterator<Item = PathBuf> + use<> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
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
        assert_eq!(site_packages_dirs(tmp.path()), [sp]);
    }

    #[test]
    fn site_packages_dirs_finds_windows_layout() {
        let tmp = TempDir::new().unwrap();
        let sp = mkdirs(tmp.path(), "Lib/site-packages");
        assert_eq!(site_packages_dirs(tmp.path()), [sp]);
    }

    #[test]
    fn site_packages_dirs_finds_free_threaded_layout() {
        let tmp = TempDir::new().unwrap();
        let sp = mkdirs(tmp.path(), "lib/python3.14t/site-packages");
        assert_eq!(site_packages_dirs(tmp.path()), [sp]);
    }

    // Needs a case-sensitive file system to hold both `lib` and `Lib`.
    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn site_packages_dirs_returns_both_layouts_sorted() {
        let tmp = TempDir::new().unwrap();
        let py313 = mkdirs(tmp.path(), "lib/python3.13/site-packages");
        let win = mkdirs(tmp.path(), "Lib/site-packages");
        let py312 = mkdirs(tmp.path(), "lib/python3.12/site-packages");
        assert_eq!(site_packages_dirs(tmp.path()), [win, py312, py313]);
    }

    #[test]
    fn site_packages_dirs_returns_every_python_dir_sorted() {
        let tmp = TempDir::new().unwrap();
        let py313 = mkdirs(tmp.path(), "lib/python3.13/site-packages");
        let py312 = mkdirs(tmp.path(), "lib/python3.12/site-packages");
        mkdirs(tmp.path(), "lib/pkgconfig");
        assert_eq!(site_packages_dirs(tmp.path()), [py312, py313]);
    }

    #[test]
    fn site_packages_dirs_matches_lib_ignoring_case() {
        let tmp = TempDir::new().unwrap();
        let sp = mkdirs(tmp.path(), "LIB/site-packages");
        assert_eq!(site_packages_dirs(tmp.path()), [sp]);
    }

    #[test]
    fn site_packages_dirs_ignores_files() {
        let tmp = TempDir::new().unwrap();
        let lib = mkdirs(tmp.path(), "lib/python3.13");
        fs::write(lib.join("site-packages"), "").unwrap();
        fs::write(tmp.path().join("lib").join("site-packages"), "").unwrap();
        fs::write(tmp.path().join("lib").join("python3.12"), "").unwrap();
        assert!(site_packages_dirs(tmp.path()).is_empty());

        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Lib"), "").unwrap();
        assert!(site_packages_dirs(tmp.path()).is_empty());
    }

    #[test]
    fn site_packages_dirs_empty_for_missing_venv() {
        let tmp = TempDir::new().unwrap();
        assert!(site_packages_dirs(&tmp.path().join("nope")).is_empty());
    }

    #[test]
    fn site_packages_dirs_under_bracketed_venv() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join("[wip]").join("venv");
        let sp = mkdirs(&venv, "lib/python3.13/site-packages");
        assert_eq!(site_packages_dirs(&venv), [sp]);
    }
}
