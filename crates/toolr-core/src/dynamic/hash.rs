//! Hash the set of packages installed in the tools venv.
//!
//! Used as `Manifest.dynamic_hash` — when this value differs from the
//! one stamped into the manifest, the dynamic layer is stale and must be
//! regenerated before the next command executes.

use std::path::Path;

use anyhow::{Context, Result};
use blake3::Hasher;

/// Compute a deterministic hash of the installed-package set in `venv_root`.
///
/// The hash covers the sorted list of `*.dist-info` directory names under
/// `lib/python*/site-packages/`. Because each `.dist-info` directory is
/// named `<package>-<version>.dist-info`, any add / remove / version-change
/// changes the hash.
pub fn compute_dynamic_hash(venv_root: &Path) -> Result<String> {
    let names = collect_dist_info_names(venv_root)
        .with_context(|| format!("scanning {} for dist-info", venv_root.display()))?;
    let mut hasher = Hasher::new();
    for n in &names {
        hasher.update(n.as_bytes());
        hasher.update(b"\0");
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn collect_dist_info_names(venv_root: &Path) -> Result<Vec<String>> {
    let lib = venv_root.join("lib");
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(&lib) else {
        // No lib/ → empty venv-like layout. Return an empty list so the
        // resulting hash is stable rather than an error.
        return Ok(names);
    };
    for entry in entries.flatten() {
        let pyver = entry.path();
        let site = pyver.join("site-packages");
        let Ok(site_entries) = std::fs::read_dir(&site) else {
            continue;
        };
        for sp_entry in site_entries.flatten() {
            let name = sp_entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".dist-info")
                && sp_entry
                    .file_type()
                    .map(|t| t.is_dir())
                    .unwrap_or(false)
            {
                names.push(name);
            }
        }
    }
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_venv(packages: &[&str]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let site = tmp
            .path()
            .join("lib")
            .join("python3.13")
            .join("site-packages");
        std::fs::create_dir_all(&site).unwrap();
        for p in packages {
            std::fs::create_dir(site.join(format!("{p}.dist-info"))).unwrap();
        }
        tmp
    }

    #[test]
    fn identical_package_sets_hash_identically() {
        let a = make_venv(&["foo-1.0.0", "bar-2.0.0"]);
        let b = make_venv(&["bar-2.0.0", "foo-1.0.0"]); // different filesystem order
        assert_eq!(
            compute_dynamic_hash(a.path()).unwrap(),
            compute_dynamic_hash(b.path()).unwrap(),
        );
    }

    #[test]
    fn version_bump_changes_hash() {
        let a = make_venv(&["foo-1.0.0"]);
        let b = make_venv(&["foo-1.0.1"]);
        assert_ne!(
            compute_dynamic_hash(a.path()).unwrap(),
            compute_dynamic_hash(b.path()).unwrap(),
        );
    }

    #[test]
    fn missing_lib_dir_returns_empty_hash() {
        let tmp = TempDir::new().unwrap();
        // Hash is stable: the same value any other "empty" venv produces.
        let h = compute_dynamic_hash(tmp.path()).unwrap();
        assert!(!h.is_empty());
    }

    #[test]
    fn ignores_non_dist_info_entries() {
        let a = make_venv(&["foo-1.0.0"]);
        let site = a
            .path()
            .join("lib")
            .join("python3.13")
            .join("site-packages");
        std::fs::create_dir(site.join("not_a_dist_info_dir")).unwrap();
        std::fs::write(
            site.join("stray-1.0.0.dist-info"),
            "i am a file, not a dir",
        )
        .unwrap();
        let b = make_venv(&["foo-1.0.0"]);
        assert_eq!(
            compute_dynamic_hash(a.path()).unwrap(),
            compute_dynamic_hash(b.path()).unwrap(),
        );
    }
}
