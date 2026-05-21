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

use crate::third_party::glob::glob_manifests;

/// Compute a deterministic hash of the third-party plugin manifests
/// installed under `venv_root`.
///
/// The hash covers, in glob-sorted order, each
/// `site-packages/<pkg>/toolr-manifest.json` path together with the
/// blake3 of its contents. Adds, removes, or content edits all change
/// the hash. Unrelated `.dist-info` churn does not.
pub fn compute_third_party_hash(venv_root: &Path) -> Result<String> {
    let paths = glob_manifests(venv_root)
        .with_context(|| format!("globbing third-party manifests under {}", venv_root.display()))?;
    let mut hasher = Hasher::new();
    for path in &paths {
        hasher.update(path.to_string_lossy().as_bytes());
        hasher.update(b"\0");
        let contents = fs::read(path)
            .with_context(|| format!("reading {}", path.display()))?;
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
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Create a venv-shaped tree with the named packages, each shipping
    /// a `toolr-manifest.json` whose contents are the supplied string.
    fn venv_with_manifests(entries: &[(&str, &str)]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let site = tmp.path().join("lib").join("python3.13").join("site-packages");
        for (pkg, content) in entries {
            let pkg_dir = site.join(pkg);
            fs::create_dir_all(&pkg_dir).unwrap();
            fs::write(pkg_dir.join("toolr-manifest.json"), content).unwrap();
        }
        tmp
    }

    #[test]
    fn empty_venv_returns_stable_hash() {
        let tmp = TempDir::new().unwrap();
        let h = compute_third_party_hash(tmp.path()).unwrap();
        assert_eq!(h, compute_third_party_hash(tmp.path()).unwrap());
    }

    #[test]
    fn adding_a_manifest_changes_hash() {
        let a = venv_with_manifests(&[]);
        let b = venv_with_manifests(&[("foo", "{}")]);
        assert_ne!(
            compute_third_party_hash(a.path()).unwrap(),
            compute_third_party_hash(b.path()).unwrap()
        );
    }

    #[test]
    fn modifying_manifest_content_changes_hash() {
        let a = venv_with_manifests(&[("foo", r#"{"v":1}"#)]);
        let b = venv_with_manifests(&[("foo", r#"{"v":2}"#)]);
        assert_ne!(
            compute_third_party_hash(a.path()).unwrap(),
            compute_third_party_hash(b.path()).unwrap()
        );
    }

    #[test]
    fn removing_a_manifest_changes_hash() {
        let a = venv_with_manifests(&[("foo", "{}"), ("bar", "{}")]);
        let b = venv_with_manifests(&[("foo", "{}")]);
        assert_ne!(
            compute_third_party_hash(a.path()).unwrap(),
            compute_third_party_hash(b.path()).unwrap()
        );
    }

    #[test]
    fn empty_helper_matches_empty_venv_hash() {
        let tmp = TempDir::new().unwrap();
        let h1 = compute_third_party_hash(tmp.path()).unwrap();
        let h2 = empty_third_party_hash();
        assert_eq!(h1, h2);
    }

    #[test]
    fn unrelated_dist_info_does_not_change_hash() {
        let a = venv_with_manifests(&[("foo", "{}")]);
        let before = compute_third_party_hash(a.path()).unwrap();
        let site = a.path().join("lib").join("python3.13").join("site-packages");
        fs::create_dir(site.join("unrelated_pkg-1.0.0.dist-info")).unwrap();
        let after = compute_third_party_hash(a.path()).unwrap();
        assert_eq!(before, after);
    }
}
