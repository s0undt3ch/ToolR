//! CI gate: `toolr.MANIFEST_SCHEMA_VERSION` in Python must equal the Rust manifest
//! `SCHEMA_VERSION`, so the public Python constant can't drift from the reader it describes.

use std::fs;
use std::path::PathBuf;

use toolr_core::manifest::SCHEMA_VERSION;

#[test]
fn rust_and_python_manifest_schema_versions_match() {
    let decorators_py = locate_decorators_py();
    let text = fs::read_to_string(&decorators_py)
        .unwrap_or_else(|e| panic!("read {}: {e}", decorators_py.display()));
    let python_version = extract_python_manifest_schema_version(&text).unwrap_or_else(|| {
        panic!(
            "could not find `MANIFEST_SCHEMA_VERSION: int = N` in {}",
            decorators_py.display()
        )
    });

    assert_eq!(
        SCHEMA_VERSION, python_version,
        "manifest schema-version mismatch: Rust SCHEMA_VERSION={SCHEMA_VERSION}, \
         Python MANIFEST_SCHEMA_VERSION={python_version}. Bump both in lock-step \
         (crates/toolr-core/src/manifest/model.rs and \
         crates/toolr-py/python/toolr/_decorators.py).",
    );
}

fn locate_decorators_py() -> PathBuf {
    // `CARGO_MANIFEST_DIR` for this crate points at `crates/toolr-core/`.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent() // crates/
        .expect("toolr-core lives under crates/")
        .join("toolr-py")
        .join("python")
        .join("toolr")
        .join("_decorators.py")
}

/// Pull the integer literal out of a line like `MANIFEST_SCHEMA_VERSION: int = 2`.
/// Tolerates whitespace variations and trailing comments. Returns `None`
/// if the line isn't present or the right-hand side isn't a `u32`.
fn extract_python_manifest_schema_version(source: &str) -> Option<u32> {
    for line in source.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("MANIFEST_SCHEMA_VERSION") {
            continue;
        }
        let line_no_comment = trimmed.split('#').next().unwrap_or("");
        let rhs = line_no_comment.split('=').nth(1)?.trim();
        // A bare annotation (`MANIFEST_SCHEMA_VERSION: int`) has no value; skip it.
        let digits: String = rhs.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return digits.parse().ok();
        }
    }
    None
}

#[test]
fn extract_python_manifest_schema_version_parses_canonical_form() {
    let src = "MANIFEST_SCHEMA_VERSION: int = 7\n";
    assert_eq!(extract_python_manifest_schema_version(src), Some(7));
}

#[test]
fn extract_python_manifest_schema_version_tolerates_trailing_comment() {
    let src = "MANIFEST_SCHEMA_VERSION: int = 42  # bumped 2026-01-15\n";
    assert_eq!(extract_python_manifest_schema_version(src), Some(42));
}

#[test]
fn extract_python_manifest_schema_version_returns_none_for_no_value() {
    let src = "MANIFEST_SCHEMA_VERSION: int\n";
    assert_eq!(extract_python_manifest_schema_version(src), None);
}
