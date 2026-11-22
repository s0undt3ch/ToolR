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
    let py_dir = tmp
        .path()
        .join("lib")
        .join("python3.13")
        .join("site-packages");
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
    assert!(
        pkg.ends_with("toolr"),
        "expected path ending in toolr, got {pkg:?}"
    );
}

#[test]
#[cfg(unix)]
fn reads_installed_toolr_py_version_from_dist_info() {
    let tmp = TempDir::new().unwrap();
    fake_unix_venv(tmp.path(), true, true);
    let sp = tmp
        .path()
        .join("lib")
        .join("python3.13")
        .join("site-packages");
    std::fs::create_dir_all(sp.join("toolr_py-0.32.0.dist-info")).unwrap();
    std::fs::create_dir_all(sp.join("toolr_example_plugin-1.0.0.dist-info")).unwrap();
    assert_eq!(
        installed_toolr_py_version(tmp.path()).unwrap().as_deref(),
        Some("0.32.0")
    );
}

#[test]
#[cfg(unix)]
fn no_toolr_py_dist_info_means_no_version() {
    let tmp = TempDir::new().unwrap();
    fake_unix_venv(tmp.path(), true, true);
    assert_eq!(installed_toolr_py_version(tmp.path()).unwrap(), None);
}

/// The Windows `Lib/site-packages` layout is found on every platform.
#[test]
fn locates_toolr_package_and_version_in_windows_layout() {
    let tmp = tempfile::TempDir::new().unwrap();
    let sp = tmp.path().join("Lib").join("site-packages");
    std::fs::create_dir_all(sp.join("toolr")).unwrap();
    std::fs::write(sp.join("toolr").join("__init__.py"), b"").unwrap();
    std::fs::create_dir_all(sp.join("toolr_py-0.40.0.dist-info")).unwrap();
    assert_eq!(
        super::locate_toolr_package(tmp.path()).unwrap(),
        Some(sp.join("toolr"))
    );
    assert_eq!(
        super::installed_toolr_py_version(tmp.path())
            .unwrap()
            .as_deref(),
        Some("0.40.0")
    );
}

/// A stale `python3.9` dir mustn't shadow the venv's `python3.13`, though it sorts first.
#[test]
fn toolr_py_version_comes_from_the_venvs_own_python() {
    let tmp = tempfile::TempDir::new().unwrap();
    for (py, version) in [("python3.9", "0.30.0"), ("python3.13", "0.40.0")] {
        let sp = tmp.path().join("lib").join(py).join("site-packages");
        std::fs::create_dir_all(sp.join("toolr")).unwrap();
        std::fs::write(sp.join("toolr").join("__init__.py"), b"").unwrap();
        std::fs::create_dir_all(sp.join(format!("toolr_py-{version}.dist-info"))).unwrap();
    }
    let py313 = tmp.path().join("lib/python3.13/site-packages/toolr");
    assert_eq!(
        super::locate_toolr_package(tmp.path()).unwrap(),
        Some(py313.clone())
    );
    assert_eq!(
        super::installed_toolr_py_version(tmp.path())
            .unwrap()
            .as_deref(),
        Some("0.40.0")
    );

    std::fs::write(tmp.path().join("pyvenv.cfg"), "version_info = 3.9.20\n").unwrap();
    assert_eq!(
        super::installed_toolr_py_version(tmp.path())
            .unwrap()
            .as_deref(),
        Some("0.30.0")
    );
}

/// A permissions problem names the path and the OS error, not a missing `toolr-py` pin.
/// The one test that pins this wording; others match on the error kind.
#[cfg(unix)]
#[test]
fn unreadable_lib_is_an_io_error() {
    let tmp = TempDir::new().unwrap();
    fake_unix_venv(tmp.path(), true, true);
    let python = tmp.path().join("bin").join("python");
    let lib = tmp.path().join("lib");
    let result = crate::test_fs::with_mode(&lib, 0o000, || {
        (
            validate_venv(tmp.path(), &python),
            installed_toolr_py_version(tmp.path()).map_err(ValidationError::from),
        )
    });
    assert!(
        result.as_ref().is_none_or(|(validated, version)| {
            matches!(version, Err(ValidationError::Io { path, .. }) if path.starts_with(&lib))
                && validated.as_ref().is_err_and(|e| {
                    e.to_string()
                        == format!(
                            "toolr: I/O error reading {}: Permission denied (os error 13)",
                            lib.display()
                        )
                })
        }),
        "{result:?}"
    );
}

#[test]
fn minor_version_order_ignores_patch_level() {
    use std::cmp::Ordering::{Equal, Greater, Less};
    assert_eq!(
        super::compare_minor_versions("0.33.0", "0.33.4"),
        Some(Equal)
    );
    assert_eq!(
        super::compare_minor_versions("0.32.0", "0.33.0"),
        Some(Less)
    );
    assert_eq!(
        super::compare_minor_versions("1.0.0", "0.33.0"),
        Some(Greater)
    );
}

#[test]
fn unparsable_versions_have_no_order() {
    assert_eq!(super::compare_minor_versions("garbage", "0.33.0"), None);
    assert_eq!(super::compare_minor_versions("0.33.0", ""), None);
}
