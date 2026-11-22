//! Hash the set of third-party `toolr-manifest.json` files in the tools venv.
//!
//! Used as `Manifest.third_party_hash`. When this value differs from the
//! one stamped into the manifest, third-party plugin state has changed
//! (add, remove, or content modification) and the manifest must be
//! regenerated before the next command executes.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use blake3::Hasher;

use crate::third_party::find::find_manifests;

/// Compute a deterministic hash of the third-party plugin manifests
/// installed under `venv_root`.
///
/// The hash covers, in path-sorted order, each
/// `site-packages/<pkg>/toolr-manifest.json` path together with the
/// blake3 of its contents. Adds, removes, or content edits all change
/// the hash. Unrelated `.dist-info` churn does not.
pub fn compute_third_party_hash(venv_root: &Path) -> Result<String> {
    let paths = find_manifests(venv_root).with_context(|| {
        format!(
            "finding third-party manifests under {}",
            venv_root.display()
        )
    })?;
    let mut hasher = Hasher::new();
    for path in &paths {
        hasher.update(path.to_string_lossy().as_bytes());
        hasher.update(b"\0");
        let contents = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let mut file_hasher = Hasher::new();
        file_hasher.update(&contents);
        hasher.update(file_hasher.finalize().as_bytes());
        hasher.update(b"\0");
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// The third-party hash value for a venv that contains no
/// `toolr-manifest.json` files (or no venv at all). Equivalent to
/// `compute_third_party_hash` on an empty `site-packages` tree.
///
/// Callers in `freshness::compare` use this constant when the project
/// has no venv resolved, so a freshly-bootstrapped manifest doesn't
/// falsely register third-party drift.
pub fn empty_third_party_hash() -> String {
    blake3::Hasher::new().finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests;
