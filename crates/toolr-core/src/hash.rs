//! Stable hashing over `tools/**/*.py` content.

use std::path::{Path, PathBuf};

use anyhow::Result;
use blake3::Hasher;
use walkdir::WalkDir;

use crate::text::escape_control_chars;

/// Hash all `*.py` files under `tools_dir`. Path order is deterministic
/// (sorted) so the result is reproducible across runs and machines.
pub fn hash_tools_dir(tools_dir: &Path) -> Result<String> {
    hash_paths(tools_dir, &list_python_files(tools_dir)?)
}

/// Every `*.py` under `tools_dir`, sorted. Shared by the hasher and the
/// static parser so both agree on what makes up `tools/`.
///
/// Prunes `__pycache__` and dot-prefixed directories during the walk. The
/// in-tree venv always resolves to `tools/.venv`, so this keeps
/// site-packages out of both the hash and the manifest (#544); a module
/// under `.venv` would otherwise become `tools..venv.lib.<pkg>`. The root
/// itself is never pruned, even if its basename starts with a dot.
///
/// A `*.py` symlink fails the walk (F-17); symlinked directories are not
/// followed. The symlink check reads the directory entry, so it costs no
/// extra syscall.
///
/// `toolr.testing._discovery._reject_symlinked_modules` mirrors this walk in
/// Python; change both together.
pub(crate) fn list_python_files(tools_dir: &Path) -> Result<Vec<PathBuf>, SymlinkedModules> {
    let mut paths = Vec::new();
    let mut links = Vec::new();
    let walk = WalkDir::new(tools_dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            e.depth() == 0 || !e.file_type().is_dir() || !is_pruned_dir(e.file_name())
        })
        .filter_map(|e| e.ok());
    for e in walk {
        if e.path().extension().is_none_or(|x| x != "py") {
            continue;
        }
        if e.path_is_symlink() {
            // A `.py`-named link to a directory is a directory link: not
            // followed, not rejected. Only links pay for this stat.
            if !std::fs::metadata(e.path()).is_ok_and(|m| m.is_dir()) {
                links.push(e.into_path());
            }
        } else if e.file_type().is_file() {
            paths.push(e.into_path());
        }
    }
    if !links.is_empty() {
        return Err(SymlinkedModules::new(tools_dir, links));
    }
    paths.sort();
    Ok(paths)
}

/// `*.py` symlinks found by [`list_python_files`]. toolr supports modules
/// living under `tools/` itself, not modules shared into it by links.
#[derive(Debug, thiserror::Error)]
#[error(
    "symlinked modules are not supported ({count}):\n{details}\nPut the file itself under {root}/, or share it as a Python package.",
    count = .paths.len(),
    details = .paths.iter().map(|p| format!("  - {p}")).collect::<Vec<_>>().join("\n"),
)]
pub struct SymlinkedModules {
    /// The walked directory's name, e.g. `tools` or a plugin package.
    root: String,
    /// Each link, relative to the walked directory's parent, sorted.
    paths: Vec<String>,
}

impl SymlinkedModules {
    fn new(tools_dir: &Path, mut links: Vec<PathBuf>) -> Self {
        links.sort();
        let root = root_name(tools_dir);
        let paths = links
            .iter()
            .map(|p| {
                let rel = p.strip_prefix(tools_dir).unwrap_or(p);
                let parts: Vec<_> = std::iter::once(root.clone())
                    .chain(
                        rel.components()
                            .map(|c| c.as_os_str().to_string_lossy().into()),
                    )
                    .collect();
                escape_control_chars(&parts.join("/"))
            })
            .collect();
        Self {
            root: escape_control_chars(&root),
            paths,
        }
    }
}

/// The walked directory's own name, even when it was given as `.` or `..`
/// (`toolr self build-manifest --source-dir .`).
fn root_name(dir: &Path) -> String {
    let named = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned());
    named(dir)
        .or_else(|| std::path::absolute(dir).ok().as_deref().and_then(named))
        .or_else(|| dir.canonicalize().ok().as_deref().and_then(named))
        .unwrap_or_else(|| dir.display().to_string())
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
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

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

    fn file_link(target: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(target, link).unwrap();
    }

    /// F-17 repro tree: `tools/linked.py -> ../shared/linked.py` next to a
    /// regular `tools/demo.py`, plus a nested and a dangling link. Every link
    /// is reported at once, sorted, relative to the project root.
    #[test]
    fn symlinked_modules_are_rejected_all_at_once() {
        let tmp = setup(&[
            ("tools/demo.py", "x"),
            ("shared/linked.py", "y"),
            ("shared/other.py", "z"),
        ]);
        let tools = tmp.path().join("tools");
        // Windows runners cover this when they may create file symlinks.
        #[cfg(windows)]
        if std::os::windows::fs::symlink_file("demo.py", tools.join("probe")).is_err() {
            eprintln!("skipping: no permission to create file symlinks");
            return;
        }
        let up = Path::new("..");
        file_link(
            &up.join("shared").join("linked.py"),
            &tools.join("linked.py"),
        );
        std::fs::create_dir(tools.join("sub")).unwrap();
        let shared_other = up.join("..").join("shared").join("other.py");
        file_link(&shared_other, &tools.join("sub").join("other.py"));
        file_link(
            &up.join("shared").join("missing.py"),
            &tools.join("gone.py"),
        );

        let err = list_python_files(&tools).unwrap_err();
        assert_eq!(
            err.to_string(),
            "symlinked modules are not supported (3):\n  - tools/gone.py\n  - tools/linked.py\n  - tools/sub/other.py\nPut the file itself under tools/, or share it as a Python package."
        );
        let err = hash_tools_dir(&tools).unwrap_err();
        assert!(
            err.to_string()
                .starts_with("symlinked modules are not supported (3):"),
            "{err}"
        );
    }

    /// A walk rooted at `.` or `..` still names the directory, not `.`.
    #[cfg(unix)]
    #[test]
    fn symlinked_modules_name_a_relative_root() {
        let tmp = setup(&[("my_plugin/sub/x.py", "x")]);
        let links = vec![PathBuf::from("./linked.py"), PathBuf::from("./sub/l.py")];
        let err = SymlinkedModules::new(Path::new("."), links);
        let cwd = std::env::current_dir().unwrap();
        let name = cwd.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(
            err.paths,
            [format!("{name}/linked.py"), format!("{name}/sub/l.py")]
        );
        assert_eq!(err.root, name);

        let sub = tmp.path().join("my_plugin/sub");
        assert_eq!(root_name(&sub.join("..")), "my_plugin");
        assert_eq!(root_name(Path::new("/")), "/");
    }

    /// Directory links are still not followed, and links under pruned
    /// directories are never looked at.
    #[cfg(unix)]
    #[test]
    fn symlinked_directories_and_pruned_links_are_not_rejected() {
        use std::os::unix::fs::symlink;
        let tmp = setup(&[
            ("tools/demo.py", "x"),
            ("venv/lib/site-packages/pkg/mod.py", "y"),
            ("shared/lib/mod.py", "z"),
            ("shared/linked.py", "w"),
        ]);
        let tools = tmp.path().join("tools");
        symlink("../venv", tools.join(".venv")).unwrap();
        symlink("../shared/lib", tools.join("lib")).unwrap();
        symlink("../shared/lib", tools.join("pkg.py")).unwrap();
        symlink("../shared/linked.py", tools.join("notes.txt")).unwrap();
        for pruned in ["__pycache__", ".cache"] {
            std::fs::create_dir(tools.join(pruned)).unwrap();
            symlink("../../shared/linked.py", tools.join(pruned).join("x.py")).unwrap();
        }

        assert_eq!(
            list_python_files(&tools).unwrap(),
            vec![tools.join("demo.py")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_module_names_are_escaped() {
        let tmp = setup(&[("tools/demo.py", "x")]);
        let tools = tmp.path().join("tools");
        std::os::unix::fs::symlink("demo.py", tools.join("a\x1b]0;x\x07.py")).unwrap();
        let err = list_python_files(&tools).unwrap_err().to_string();
        assert!(err.contains("  - tools/a\\u{1b}]0;x\\u{7}.py\n"), "{err}");
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
