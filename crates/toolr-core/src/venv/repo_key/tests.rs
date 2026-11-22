use super::*;
use tempfile::TempDir;

/// A literal, so a change to the key's inputs fails here instead of silently orphaning every
/// existing venv: the 1.0 bump must keep producing this key.
#[cfg(unix)]
#[test]
fn key_is_a_golden_literal() {
    assert_eq!(
        compute_repo_key(Path::new("/"), "3.13").unwrap(),
        "43c4783000b0ac2b"
    );
}

#[test]
fn deterministic_for_same_inputs() {
    let tmp = TempDir::new().unwrap();
    let a = compute_repo_key(tmp.path(), "3.13").unwrap();
    let b = compute_repo_key(tmp.path(), "3.13").unwrap();
    assert_eq!(a, b);
}

#[test]
fn differs_with_python_version() {
    let tmp = TempDir::new().unwrap();
    let a = compute_repo_key(tmp.path(), "3.12").unwrap();
    let b = compute_repo_key(tmp.path(), "3.13").unwrap();
    assert_ne!(a, b);
}

#[test]
fn differs_with_path() {
    let a_tmp = TempDir::new().unwrap();
    let b_tmp = TempDir::new().unwrap();
    let a = compute_repo_key(a_tmp.path(), "3.13").unwrap();
    let b = compute_repo_key(b_tmp.path(), "3.13").unwrap();
    assert_ne!(a, b);
}

#[test]
fn errors_on_missing_path() {
    let result = compute_repo_key(Path::new("/no/such/dir-toolr-test"), "3.13");
    assert!(result.is_err());
}
