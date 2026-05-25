use std::fs;

use tempfile::TempDir;

use super::probe::{ProbeOutcome, probe_module, site_packages_dir};

/// Build a fake unix-shaped venv with the requested module shapes.
///
/// Recognised shapes:
/// - `"package"` — `<name>/__init__.py`
/// - `"single"` — `<name>.py`
/// - `"ext_so"` — `<name>.so` (bare extension)
/// - `"ext_pyd"` — `<name>.pyd` (Windows-style extension)
/// - `"ext_abi3"` — `<name>.abi3.so` (stable ABI tag)
/// - `"ext_cpython"` — `<name>.cpython-313-darwin.so` (CPython ABI tag)
/// - `"namespace"` — `<name>/` directory only, no `__init__.py`
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
            "ext_so" => {
                fs::write(sp.join(format!("{name}.so")), "").unwrap();
            }
            "ext_pyd" => {
                fs::write(sp.join(format!("{name}.pyd")), "").unwrap();
            }
            "ext_abi3" => {
                fs::write(sp.join(format!("{name}.abi3.so")), "").unwrap();
            }
            "ext_cpython" => {
                fs::write(sp.join(format!("{name}.cpython-313-darwin.so")), "").unwrap();
            }
            "namespace" => {
                fs::create_dir(sp.join(name)).unwrap();
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

#[test]
fn probe_module_finds_bare_so_extension() {
    let venv = fake_venv(&[("_cffi_backend", "ext_so")]);
    let sp = site_packages_dir(venv.path()).unwrap();
    let outcome = probe_module(&sp, "_cffi_backend");
    assert!(
        matches!(outcome, ProbeOutcome::Extension(ref p) if p.file_name().unwrap() == "_cffi_backend.so"),
        "expected Extension(_cffi_backend.so), got {outcome:?}"
    );
}

#[test]
fn probe_module_finds_windows_pyd_extension() {
    let venv = fake_venv(&[("_winreg_shim", "ext_pyd")]);
    let sp = site_packages_dir(venv.path()).unwrap();
    let outcome = probe_module(&sp, "_winreg_shim");
    assert!(
        matches!(outcome, ProbeOutcome::Extension(ref p) if p.file_name().unwrap() == "_winreg_shim.pyd"),
        "expected Extension(_winreg_shim.pyd), got {outcome:?}"
    );
}

#[test]
fn probe_module_finds_abi3_tagged_extension() {
    let venv = fake_venv(&[("_brotli", "ext_abi3")]);
    let sp = site_packages_dir(venv.path()).unwrap();
    let outcome = probe_module(&sp, "_brotli");
    assert!(
        matches!(outcome, ProbeOutcome::Extension(ref p) if p.file_name().unwrap() == "_brotli.abi3.so"),
        "expected Extension(_brotli.abi3.so), got {outcome:?}"
    );
}

#[test]
fn probe_module_finds_cpython_tagged_extension() {
    let venv = fake_venv(&[("_psutil_osx", "ext_cpython")]);
    let sp = site_packages_dir(venv.path()).unwrap();
    let outcome = probe_module(&sp, "_psutil_osx");
    assert!(
        matches!(outcome, ProbeOutcome::Extension(ref p) if p.file_name().unwrap() == "_psutil_osx.cpython-313-darwin.so"),
        "expected Extension(_psutil_osx.cpython-313-darwin.so), got {outcome:?}"
    );
}

#[test]
fn probe_module_finds_namespace_package() {
    let venv = fake_venv(&[("zope", "namespace")]);
    let sp = site_packages_dir(venv.path()).unwrap();
    let outcome = probe_module(&sp, "zope");
    assert!(
        matches!(outcome, ProbeOutcome::NamespacePackage(_)),
        "expected NamespacePackage, got {outcome:?}"
    );
}

#[test]
fn probe_module_prefers_init_py_over_namespace_package() {
    // A directory with `__init__.py` must classify as Package, not
    // NamespacePackage. `<top>/` is also a directory, but the
    // `__init__.py` short-circuits before the namespace check.
    let venv = fake_venv(&[("pkg", "package")]);
    let sp = site_packages_dir(venv.path()).unwrap();
    assert!(matches!(probe_module(&sp, "pkg"), ProbeOutcome::Package(_)));
}

#[test]
fn probe_module_prefers_single_py_over_extension() {
    // If both `foo.py` and `foo.so` exist, the `.py` wins our check
    // order. (Python's importer actually prefers the extension; the
    // probe only cares that *something* importable exists, so the
    // exact precedence doesn't matter for the preflight outcome.)
    let venv = fake_venv(&[("foo", "single"), ("foo", "ext_so")]);
    let sp = site_packages_dir(venv.path()).unwrap();
    assert!(matches!(
        probe_module(&sp, "foo"),
        ProbeOutcome::SingleFile(_)
    ));
}

#[test]
fn probe_module_does_not_match_prefix_collision() {
    // `foobar.so` must not satisfy a probe for `foo` — the dotted-tag
    // scan checks for `foo.<...>.so`, not `foo<anything>.so`.
    let venv = fake_venv(&[("foobar", "ext_so")]);
    let sp = site_packages_dir(venv.path()).unwrap();
    assert_eq!(probe_module(&sp, "foo"), ProbeOutcome::Missing);
}

use super::preflight::{MissingDeps, check_imports};

#[test]
fn check_imports_passes_when_all_present() {
    let venv = fake_venv(&[("packaging", "package"), ("six", "single")]);
    let sp = site_packages_dir(venv.path()).unwrap();
    let imports = vec!["packaging".to_string(), "six".to_string()];
    assert!(check_imports(&sp, &imports).is_ok());
}

#[test]
fn check_imports_reports_missing_module() {
    let venv = fake_venv(&[("packaging", "package")]);
    let sp = site_packages_dir(venv.path()).unwrap();
    let imports = vec!["packaging".to_string(), "yaml".to_string()];
    let err = check_imports(&sp, &imports).expect_err("should be missing");
    assert_eq!(err.missing, vec!["yaml".to_string()]);
}

#[test]
fn check_imports_reports_all_missing_in_input_order() {
    let venv = fake_venv(&[]);
    let sp = site_packages_dir(venv.path()).unwrap();
    let imports = vec![
        "yaml".to_string(),
        "cv2".to_string(),
        "sklearn".to_string(),
    ];
    let err = check_imports(&sp, &imports).expect_err("should be missing");
    assert_eq!(err.missing, imports);
}

#[test]
fn check_imports_skips_stdlib_like_names() {
    let venv = fake_venv(&[]);
    let sp = site_packages_dir(venv.path()).unwrap();
    assert!(check_imports(&sp, &[]).is_ok());
}

#[test]
fn missing_deps_message_quotes_module_and_suggests_sync() {
    let err = MissingDeps {
        missing: vec!["yaml".to_string()],
    };
    let rendered = err.to_string();
    assert!(rendered.contains("`yaml`"));
    assert!(rendered.contains("toolr project deps sync"));
    assert!(rendered.contains("tools/pyproject.toml"));
}

#[test]
fn missing_deps_message_pluralizes_when_multiple() {
    let err = MissingDeps {
        missing: vec!["yaml".to_string(), "cv2".to_string()],
    };
    let rendered = err.to_string();
    let yaml_idx = rendered.find("yaml").unwrap();
    let cv2_idx = rendered.find("cv2").unwrap();
    assert!(yaml_idx < cv2_idx);
}
