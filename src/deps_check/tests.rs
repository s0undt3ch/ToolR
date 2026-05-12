use std::fs;

use tempfile::TempDir;

use super::probe::{ProbeOutcome, probe_module, site_packages_dir};

/// Build a fake unix-shaped venv with the requested module shapes.
/// Each entry is either ("foo", "package") or ("bar", "single").
fn fake_venv(shapes: &[(&str, &str)]) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let sp = tmp
        .path()
        .join("lib")
        .join("python3.13")
        .join("site-packages");
    fs::create_dir_all(&sp).unwrap();
    for (name, kind) in shapes {
        match *kind {
            "package" => {
                let pkg = sp.join(name);
                fs::create_dir(&pkg).unwrap();
                fs::write(pkg.join("__init__.py"), "").unwrap();
            }
            "single" => {
                fs::write(sp.join(format!("{name}.py")), "").unwrap();
            }
            other => panic!("unknown shape {other}"),
        }
    }
    tmp
}

#[test]
fn site_packages_dir_finds_python_subdir() {
    let venv = fake_venv(&[]);
    let sp = site_packages_dir(venv.path()).expect("should find site-packages");
    assert!(sp.ends_with("site-packages"));
}

#[test]
fn site_packages_dir_returns_none_when_absent() {
    let tmp = TempDir::new().unwrap();
    assert!(site_packages_dir(tmp.path()).is_none());
}

#[test]
fn probe_module_finds_a_package() {
    let venv = fake_venv(&[("packaging", "package")]);
    let sp = site_packages_dir(venv.path()).unwrap();
    let outcome = probe_module(&sp, "packaging");
    assert!(matches!(outcome, ProbeOutcome::Package(_)));
}

#[test]
fn probe_module_finds_a_single_file_module() {
    let venv = fake_venv(&[("six", "single")]);
    let sp = site_packages_dir(venv.path()).unwrap();
    let outcome = probe_module(&sp, "six");
    assert!(matches!(outcome, ProbeOutcome::SingleFile(_)));
}

#[test]
fn probe_module_returns_missing_when_absent() {
    let venv = fake_venv(&[]);
    let sp = site_packages_dir(venv.path()).unwrap();
    assert_eq!(probe_module(&sp, "nope"), ProbeOutcome::Missing);
}

#[test]
fn probe_module_only_checks_top_level_segment() {
    // We pass a dotted name; only `pkg/__init__.py` matters.
    let venv = fake_venv(&[("pkg", "package")]);
    let sp = site_packages_dir(venv.path()).unwrap();
    assert!(matches!(
        probe_module(&sp, "pkg.sub"),
        ProbeOutcome::Package(_)
    ));
}

#[test]
fn probe_module_treats_empty_name_as_missing() {
    let venv = fake_venv(&[]);
    let sp = site_packages_dir(venv.path()).unwrap();
    assert_eq!(probe_module(&sp, ""), ProbeOutcome::Missing);
}
