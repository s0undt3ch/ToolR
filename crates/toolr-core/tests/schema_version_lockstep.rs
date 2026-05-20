//! CI gate: the Rust `RUNNER_SCHEMA_VERSION` and Python `SCHEMA_VERSION`
//! constants must match exactly.
//!
//! The toolr binary and the toolr-py Python package communicate over a
//! versioned JSON spec; a mismatch silently produces wrong-on-the-wire
//! behaviour. We declare the version in two source-of-truth files (one
//! per language); this test reads both at build time and refuses to
//! pass when they diverge.
//!
//! See `crates/toolr-core/src/execute/spec.rs` for the bump policy.

use std::fs;
use std::path::PathBuf;

use toolr_core::execute::spec::RUNNER_SCHEMA_VERSION;

#[test]
fn rust_and_python_schema_versions_match() {
    let runner_py = locate_runner_py();
    let text = fs::read_to_string(&runner_py)
        .unwrap_or_else(|e| panic!("read {}: {e}", runner_py.display()));
    let python_version = extract_python_schema_version(&text).unwrap_or_else(|| {
        panic!(
            "could not find `SCHEMA_VERSION: int = N` in {}",
            runner_py.display()
        )
    });

    assert_eq!(
        RUNNER_SCHEMA_VERSION, python_version,
        "schema-version mismatch: Rust RUNNER_SCHEMA_VERSION={RUNNER_SCHEMA_VERSION}, \
         Python SCHEMA_VERSION={python_version}. Bump both in lock-step per the policy \
         documented above each constant (see crates/toolr-core/src/execute/spec.rs and \
         crates/toolr-py/python/toolr/_runner.py).",
    );
}

fn locate_runner_py() -> PathBuf {
    // `CARGO_MANIFEST_DIR` for this crate points at `crates/toolr-core/`.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent() // crates/
        .expect("toolr-core lives under crates/")
        .join("toolr-py")
        .join("python")
        .join("toolr")
        .join("_runner.py")
}

/// Pull the integer literal out of a line like `SCHEMA_VERSION: int = 1`.
/// Tolerates whitespace variations and trailing comments. Returns `None`
/// if the line isn't present or the right-hand side isn't a `u32`.
fn extract_python_schema_version(source: &str) -> Option<u32> {
    for line in source.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("SCHEMA_VERSION") {
            continue;
        }
        // Drop any trailing `# comment`.
        let line_no_comment = trimmed.split('#').next().unwrap_or("");
        // Split on `=` to get the RHS, then trim everything non-digit.
        let rhs = line_no_comment.split('=').nth(1)?.trim();
        // The line might be just the type-annotated declaration with no
        // value (e.g. `SCHEMA_VERSION: int`) — skip those.
        let digits: String = rhs.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return digits.parse().ok();
        }
    }
    None
}

#[test]
fn extract_python_schema_version_parses_canonical_form() {
    let src = "SCHEMA_VERSION: int = 7\n";
    assert_eq!(extract_python_schema_version(src), Some(7));
}

#[test]
fn extract_python_schema_version_tolerates_trailing_comment() {
    let src = "SCHEMA_VERSION: int = 42  # bumped 2026-01-15\n";
    assert_eq!(extract_python_schema_version(src), Some(42));
}

#[test]
fn extract_python_schema_version_returns_none_for_no_value() {
    let src = "SCHEMA_VERSION: int\n";
    assert_eq!(extract_python_schema_version(src), None);
}
