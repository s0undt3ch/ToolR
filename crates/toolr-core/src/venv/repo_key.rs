//! Compute the cache-slot key for a repo's tools venv.

use std::path::Path;

use anyhow::{Context, Result};
use blake3::Hasher;

/// The last hash input. Frozen at the value toolr's major version had while the key still
/// carried it, so every existing cache key stays valid across the 1.0 bump. A venv layout change
/// is handled by `Meta::layout_version`, not by changing the key.
const KEY_EPOCH: &str = "0";

/// Compute the stable repo-key. Inputs:
/// - canonical repo path (symlinks resolved)
/// - python version (e.g. "3.13"); empty string allowed when unknown
/// - the frozen [`KEY_EPOCH`]
pub fn compute_repo_key(repo_root: &Path, python_version: &str) -> Result<String> {
    let canonical = repo_root
        .canonicalize()
        .with_context(|| format!("canonicalising {}", repo_root.display()))?;
    let mut hasher = Hasher::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    hasher.update(b"\0");
    hasher.update(python_version.as_bytes());
    hasher.update(b"\0");
    hasher.update(KEY_EPOCH.as_bytes());
    // Truncate to 16 hex chars — enough to avoid collisions, short enough
    // for nice on-disk paths.
    let hex = hasher.finalize().to_hex().to_string();
    Ok(hex[..16].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// The key is `blake3(path NUL python_version NUL "0")`, as it was while the major version was
    /// hashed, so upgrading across 1.0 reuses every existing cache slot.
    #[test]
    fn key_matches_the_pre_1_0_derivation() {
        let tmp = TempDir::new().unwrap();
        let canonical = tmp.path().canonicalize().unwrap();
        let mut hasher = Hasher::new();
        hasher.update(canonical.to_string_lossy().as_bytes());
        hasher.update(b"\0");
        hasher.update(b"3.13");
        hasher.update(b"\0");
        hasher.update(b"0");
        let expected = hasher.finalize().to_hex().to_string();
        assert_eq!(
            compute_repo_key(tmp.path(), "3.13").unwrap(),
            expected[..16]
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
}
