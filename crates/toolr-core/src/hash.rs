//! Stable hashing over `tools/**/*.py` content.

use std::path::{Path, PathBuf};

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
    hash_paths(tools_dir, &list_python_files(tools_dir))
}

/// Every `*.py` under `tools_dir`, sorted. Shared by the hasher and the
/// static parser so both agree on what makes up `tools/`.
///
/// Prunes `__pycache__` and dot-prefixed directories during the walk. The
/// in-tree venv always resolves to `tools/.venv`, so this keeps
/// site-packages out of both the hash and the manifest (#544); a module
/// under `.venv` would otherwise become `tools..venv.lib.<pkg>`. The root
/// itself is never pruned, even if its basename starts with a dot.
pub(crate) fn list_python_files(tools_dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = WalkDir::new(tools_dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            e.depth() == 0 || !e.file_type().is_dir() || !is_pruned_dir(e.file_name())
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "py"))
        .map(|e| e.into_path())
        .collect();
    paths.sort();
    paths
}

fn is_pruned_dir(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .is_some_and(|n| n.starts_with('.') || n == "__pycache__")
}

fn hash_paths(tools_dir: &Path, paths: &[PathBuf]) -> Result<String> {
    let mut hasher = Hasher::new();
    for path in paths {
        // `path` is enumerated by WalkDir under `tools_dir`, not untrusted input.
        let read = std::fs::read(path); // nosemgrep: rust.actix.path-traversal.tainted-path.tainted-path
        let bytes = match read {
            Ok(bytes) => bytes,
            // Deleted since the walk listed it: hash the tree as it now is.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        let rel = path
            .strip_prefix(tools_dir)
            .unwrap_or(path)
            .to_string_lossy();
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
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

    /// #544: with `venv-location = "in-tree"` the venv lives at
    /// `tools/.venv`; its site-packages must not feed the static hash.
    #[test]
    fn ignores_py_files_under_in_tree_venv() {
        let a = setup(&[
            ("a.py", "x"),
            (".venv/lib/python3.13/site-packages/pkg/__init__.py", "y"),
            (".venv/lib/python3.13/site-packages/pkg/mod.py", "z"),
        ]);
        let b = setup(&[("a.py", "x")]);
        assert_eq!(
            hash_tools_dir(a.path()).unwrap(),
            hash_tools_dir(b.path()).unwrap()
        );
    }

    #[test]
    fn ignores_py_files_under_pycache_and_dot_directories() {
        let a = setup(&[
            ("a.py", "x"),
            ("__pycache__/a.py", "cached"),
            ("sub/__pycache__/b.py", "cached"),
            (".git/hooks/pre-commit.py", "hook"),
            (".tox/py313/lib/x.py", "tox"),
        ]);
        let b = setup(&[("a.py", "x")]);
        assert_eq!(
            hash_tools_dir(a.path()).unwrap(),
            hash_tools_dir(b.path()).unwrap()
        );
    }

    /// A file listed by the walk but gone by the time it is read (e.g. a
    /// user module deleted mid-dispatch) is hashed as absent instead of
    /// failing the whole freshness check.
    #[test]
    fn file_vanishing_between_listing_and_read_is_skipped() {
        let tmp = setup(&[("a.py", "x")]);
        let gone = tmp.path().join("gone.py");
        let with_gone = hash_paths(tmp.path(), &[tmp.path().join("a.py"), gone]).unwrap();
        let without = hash_paths(tmp.path(), &[tmp.path().join("a.py")]).unwrap();
        assert_eq!(with_gone, without);
        assert_eq!(with_gone, hash_tools_dir(tmp.path()).unwrap());
    }

    /// Only `NotFound` is tolerated; any other read error still fails.
    #[test]
    fn unreadable_listed_path_fails_the_hash() {
        let tmp = setup(&[("a.py", "x")]);
        let dir = tmp.path().join("pkg.py");
        std::fs::create_dir(&dir).unwrap();
        assert!(hash_paths(tmp.path(), &[tmp.path().join("a.py"), dir]).is_err());
    }

    /// #544 mechanism: `uv sync` recreating `tools/.venv` while toolr
    /// hashes `tools/` used to fail with `No such file or directory`.
    /// Churn the venv from another thread while hashing repeatedly.
    #[test]
    fn hashing_survives_in_tree_venv_being_recreated_concurrently() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let tmp = setup(&[("a.py", "x"), ("ci/b.py", "y")]);
        let tools = tmp.path().to_path_buf();
        let expected = hash_tools_dir(&tools).unwrap();

        let stop = Arc::new(AtomicBool::new(false));
        let churn = {
            let stop = Arc::clone(&stop);
            let venv = tools.join(".venv");
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let sp = venv.join("lib/python3.13/site-packages/pkg");
                    // Errors ignored: Windows refuses writes into a directory
                    // that is still pending deletion.
                    let _ = std::fs::create_dir_all(&sp);
                    for i in 0..200 {
                        let _ = std::fs::write(sp.join(format!("m{i}.py")), "x = 1\n");
                    }
                    let _ = std::fs::remove_dir_all(&venv);
                }
            })
        };

        let results: Vec<_> = (0..200).map(|_| hash_tools_dir(&tools)).collect();
        stop.store(true, Ordering::Relaxed);
        churn.join().unwrap();

        let errors: Vec<String> = results
            .iter()
            .filter_map(|r| r.as_ref().err().map(|e| format!("{e:#}")))
            .collect();
        assert!(errors.is_empty(), "hashing failed mid-churn: {errors:?}");
        for r in results {
            assert_eq!(r.unwrap(), expected);
        }
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
