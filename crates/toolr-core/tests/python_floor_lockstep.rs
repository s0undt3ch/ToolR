//! CI gate: `MIN_PYTHON_MINOR` must match the lower bound of toolr-py's
//! `requires-python`.
//!
//! The PATH fallback rejects interpreters older than `MIN_PYTHON_MINOR`. If
//! toolr-py raises its floor and the constant stays behind, the probe accepts
//! an interpreter that then fails with a raw `ModuleNotFoundError`.

use std::fs;
use std::path::PathBuf;

use toolr_core::execute::python::MIN_PYTHON_MINOR;

#[test]
fn min_python_minor_matches_toolr_py_requires_python() {
    let pyproject = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("toolr-core lives under crates/")
        .join("toolr-py")
        .join("pyproject.toml");
    let text = fs::read_to_string(&pyproject)
        .unwrap_or_else(|e| panic!("read {}: {e}", pyproject.display()));
    let doc: toml::Table = text.parse().expect("toolr-py pyproject.toml parses");
    let requires_python = doc["project"]["requires-python"]
        .as_str()
        .expect("toolr-py declares project.requires-python");
    let floor = python3_floor(requires_python)
        .unwrap_or_else(|| panic!("no `>=3.N` clause in requires-python {requires_python:?}"));

    assert_eq!(
        MIN_PYTHON_MINOR, floor,
        "MIN_PYTHON_MINOR={MIN_PYTHON_MINOR} but toolr-py's requires-python is \
         {requires_python:?}. Update crates/toolr-core/src/execute/python.rs to match.",
    );
}

/// The `N` of the `>=3.N` clause in a PEP 440 specifier set.
fn python3_floor(spec: &str) -> Option<u32> {
    spec.split(',')
        .find_map(|clause| clause.trim().strip_prefix(">=")?.trim().strip_prefix("3."))
        .and_then(|minor| minor.split('.').next()?.parse().ok())
}

#[test]
fn python3_floor_reads_the_lower_bound() {
    assert_eq!(python3_floor(">=3.11,<3.16"), Some(11));
    assert_eq!(python3_floor("<3.16, >= 3.12"), Some(12));
    assert_eq!(python3_floor(">=3.13.1"), Some(13));
    assert_eq!(python3_floor("~=3.11"), None);
}
