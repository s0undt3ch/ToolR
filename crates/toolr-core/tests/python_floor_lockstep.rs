//! CI gate: `MIN_PYTHON_MINOR` must match the lower bound of toolr-py's
//! `requires-python`.
//!
//! `toolr project init` never writes a `requires-python` below
//! `MIN_PYTHON_MINOR`. If toolr-py raises its floor and the constant stays
//! behind, init scaffolds a project that can't install toolr-py.
#![cfg(feature = "full")]

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
    let (floor, patch) = python3_floor(requires_python)
        .unwrap_or_else(|| panic!("no `>=3.N` clause in requires-python {requires_python:?}"));

    assert_eq!(
        patch, 0,
        "requires-python {requires_python:?} sets a patch-level floor, which \
         MIN_PYTHON_MINOR can't express. The PATH probe only checks the minor.",
    );
    assert_eq!(
        MIN_PYTHON_MINOR, floor,
        "MIN_PYTHON_MINOR={MIN_PYTHON_MINOR} but toolr-py's requires-python is \
         {requires_python:?}. They must agree: change MIN_PYTHON_MINOR in \
         crates/toolr-core/src/execute/python.rs or requires-python in \
         crates/toolr-py/pyproject.toml so the minors match.",
    );
}

/// The `(N, P)` of the `>=3.N[.P]` clause in a PEP 440 specifier set.
fn python3_floor(spec: &str) -> Option<(u32, u32)> {
    let version = spec
        .split(',')
        .find_map(|clause| clause.trim().strip_prefix(">=")?.trim().strip_prefix("3."))?;
    let mut parts = version.split('.');
    let minor = parts.next()?.parse().ok()?;
    let patch: String = parts
        .next()
        .unwrap_or("0")
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    Some((minor, patch.parse().ok()?))
}

#[test]
fn python3_floor_reads_the_lower_bound() {
    assert_eq!(python3_floor(">=3.11,<3.16"), Some((11, 0)));
    assert_eq!(python3_floor("<3.16, >= 3.12"), Some((12, 0)));
    assert_eq!(python3_floor(">=3.11.4"), Some((11, 4)));
    assert_eq!(python3_floor(">=3.12.0rc1"), Some((12, 0)));
    assert_eq!(python3_floor("~=3.11"), None);
}
