//! Compute the cache-slot key for a repo's tools venv.

use std::path::Path;

use anyhow::{Context, Result};
use blake3::Hasher;

/// The last hash input. Frozen at the value toolr's major version had while the key still
/// carried it, so every existing cache key stays valid across the 1.0 bump. Pinned by
/// `key_is_a_golden_literal`, which is unix-only: the key for a Windows path is not pinned.
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
mod tests;
