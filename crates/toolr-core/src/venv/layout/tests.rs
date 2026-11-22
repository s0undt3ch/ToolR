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
