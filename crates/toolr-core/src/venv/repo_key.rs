//! Compute the cache-slot key for a repo's tools venv.

use std::path::Path;

use anyhow::{Context, Result};
use blake3::Hasher;

/// Toolr's own major version, baked in at build time.
/// `CARGO_PKG_VERSION_MAJOR` is always set by cargo.
pub const TOOLR_MAJOR: &str = env!("CARGO_PKG_VERSION_MAJOR");

/// Compute the stable repo-key. Inputs:
/// - canonical repo path (symlinks resolved)
/// - python version (e.g. "3.13"); empty string allowed when unknown
/// - toolr major version
pub fn compute_repo_key(repo_root: &Path, python_version: &str) -> Result<String> {
    let canonical = repo_root
        .canonicalize()
        .with_context(|| format!("canonicalising {}", repo_root.display()))?;
    let mut hasher = Hasher::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    hasher.update(b"\0");
    hasher.update(python_version.as_bytes());
    hasher.update(b"\0");
    hasher.update(TOOLR_MAJOR.as_bytes());
    // Truncate to 16 hex chars — enough to avoid collisions, short enough
    // for nice on-disk paths.
    let hex = hasher.finalize().to_hex().to_string();
    Ok(hex[..16].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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
