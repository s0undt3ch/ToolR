//! Stable hashing over `tools/**/*.py` content.

use std::path::Path;

use anyhow::Result;
use blake3::Hasher;
use walkdir::WalkDir;

/// blake3-hash the raw bytes of a single file and return the hex digest.
/// Used to fingerprint an interpreter for provenance verification.
pub fn hash_file(path: &Path) -> Result<String> {
    // `path` is an internal interpreter path resolved by toolr, not
    // untrusted external input.
    let bytes = std::fs::read(path)?; // nosemgrep: rust.actix.path-traversal.tainted-path.tainted-path
    let mut hasher = Hasher::new();
    hasher.update(&bytes);
    Ok(hasher.finalize().to_hex().to_string())
}

/// Hash all `*.py` files under `tools_dir`. Path order is deterministic
/// (sorted) so the result is reproducible across runs and machines.
pub fn hash_tools_dir(tools_dir: &Path) -> Result<String> {
    let mut paths: Vec<_> = WalkDir::new(tools_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.path().extension().is_some_and(|x| x == "py")
        })
        .map(|e| e.into_path())
        .collect();
    paths.sort();

    let mut hasher = Hasher::new();
    for path in &paths {
        let rel = path
            .strip_prefix(tools_dir)
            .unwrap_or(path)
            .to_string_lossy();
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        // `path` is enumerated by WalkDir under `tools_dir`, not untrusted input.
        let bytes = std::fs::read(path)?; // nosemgrep: rust.actix.path-traversal.tainted-path.tainted-path
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup(files: &[(&str, &str)]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        for (name, contents) in files {
            let path = tmp.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, contents).unwrap();
        }
        tmp
    }

    #[test]
    fn identical_trees_hash_identically() {
        let a = setup(&[("a.py", "x"), ("b/c.py", "y")]);
        let b = setup(&[("a.py", "x"), ("b/c.py", "y")]);
        assert_eq!(
            hash_tools_dir(a.path()).unwrap(),
            hash_tools_dir(b.path()).unwrap()
        );
    }

    #[test]
    fn different_content_hashes_differently() {
        let a = setup(&[("a.py", "x")]);
        let b = setup(&[("a.py", "y")]);
        assert_ne!(
            hash_tools_dir(a.path()).unwrap(),
            hash_tools_dir(b.path()).unwrap()
        );
    }

    #[test]
    fn hash_file_is_deterministic_and_content_sensitive() {
        let tmp = TempDir::new().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        std::fs::write(&a, b"hello").unwrap();
        std::fs::write(&b, b"hello").unwrap();
        assert_eq!(hash_file(&a).unwrap(), hash_file(&b).unwrap());
        std::fs::write(&b, b"world").unwrap();
        assert_ne!(hash_file(&a).unwrap(), hash_file(&b).unwrap());
    }

    #[test]
    fn ignores_non_py_files() {
        let a = setup(&[("a.py", "x"), ("readme.md", "ignored")]);
        let b = setup(&[("a.py", "x")]);
        assert_eq!(
            hash_tools_dir(a.path()).unwrap(),
            hash_tools_dir(b.path()).unwrap()
        );
    }
}
