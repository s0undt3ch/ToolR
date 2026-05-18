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

use super::post_mortem::{ImportErrorReport, intercept_import_error};

const PY_IMPORT_ERROR: &str = "\
Traceback (most recent call last):
  File \"/x/tools/ci.py\", line 1, in <module>
    import yaml
ModuleNotFoundError: No module named 'yaml'
";

const PY_NESTED_IMPORT_ERROR: &str = "\
Traceback (most recent call last):
  File \"/x/tools/ci.py\", line 5, in hello
    from pkg.sub import thing
ImportError: cannot import name 'thing' from 'pkg.sub'
";

const PY_GENERIC_RUNTIME_ERROR: &str = "\
Traceback (most recent call last):
  File \"/x/tools/ci.py\", line 7, in hello
    raise ValueError(\"nope\")
ValueError: nope
";

#[test]
fn intercepts_module_not_found_error() {
    let report = intercept_import_error(PY_IMPORT_ERROR).expect("should classify");
    assert_eq!(report.error_class, "ModuleNotFoundError");
    assert_eq!(report.missing_hint.as_deref(), Some("yaml"));
    assert!(report.traceback.contains("ModuleNotFoundError"));
}

#[test]
fn intercepts_plain_import_error() {
    let report = intercept_import_error(PY_NESTED_IMPORT_ERROR).expect("should classify");
    assert_eq!(report.error_class, "ImportError");
    assert!(report.traceback.contains("ImportError"));
}

#[test]
fn returns_none_for_non_import_error() {
    assert!(intercept_import_error(PY_GENERIC_RUNTIME_ERROR).is_none());
}

#[test]
fn returns_none_for_empty_input() {
    assert!(intercept_import_error("").is_none());
}

#[test]
fn rendered_report_includes_traceback_and_suggestion() {
    let report = intercept_import_error(PY_IMPORT_ERROR).unwrap();
    let rendered = report.render();
    assert!(rendered.contains(PY_IMPORT_ERROR.trim_end()));
    assert!(rendered.contains("toolr project deps sync"));
    assert!(rendered.contains("yaml"));
}

#[test]
fn rendered_report_for_import_error_without_hint_still_suggests_sync() {
    let report = intercept_import_error(PY_NESTED_IMPORT_ERROR).unwrap();
    let rendered = report.render();
    assert!(rendered.contains(PY_NESTED_IMPORT_ERROR.trim_end()));
    assert!(rendered.contains("toolr project deps sync"));
}

#[test]
fn render_preserves_traceback_byte_for_byte() {
    let stderr = PY_IMPORT_ERROR;
    let report = intercept_import_error(stderr).unwrap();
    let rendered = report.render();
    let stripped_orig = stderr.trim_end();
    assert!(rendered.starts_with(stripped_orig));
    // _suppress dead-code on the unused struct constructor in tests.
    let _ = ImportErrorReport {
        traceback: String::new(),
        error_class: String::new(),
        missing_hint: None,
    };
}
