//! Walk upward to locate the project root (parent of `tools/`).

use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("I/O error resolving {0}: {1}")]
    Io(PathBuf, #[source] std::io::Error),
    #[error("no `tools/` directory found above {0}")]
    NotFound(PathBuf),
}

/// Walk up from `start` until a directory containing `tools/` is found.
/// Returns that directory (the project root). The check stops at the
/// filesystem root.
///
/// `start` is canonicalized before the walk, so relative paths are
/// resolved against the process cwd up front and the returned path is
/// always absolute. If `start` cannot be canonicalized (e.g. it doesn't
/// exist), returns `DiscoveryError::Io`.
pub fn discover_project_root(start: &Path) -> Result<PathBuf, DiscoveryError> {
    let mut current = start
        .canonicalize()
        .map_err(|e| DiscoveryError::Io(start.to_path_buf(), e))?;
    loop {
        if current.join("tools").is_dir() {
            return Ok(current);
        }
        if !current.pop() {
            return Err(DiscoveryError::NotFound(start.to_path_buf()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn finds_tools_in_current_dir() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("tools")).unwrap();
        let root = discover_project_root(tmp.path()).unwrap();
        assert_eq!(root, tmp.path().canonicalize().unwrap());
    }

    #[test]
    fn finds_tools_in_ancestor() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("tools")).unwrap();
        let nested = tmp.path().join("a").join("b").join("c");
        std::fs::create_dir_all(&nested).unwrap();
        let root = discover_project_root(&nested).unwrap();
        assert_eq!(root, tmp.path().canonicalize().unwrap());
    }

    #[test]
    fn returns_not_found_when_no_tools_dir_exists() {
        let tmp = TempDir::new().unwrap();
        // GHA Windows runners ship with `C:\tools\` populated, so the
        // walk-up-and-find-tools/ algorithm rightfully succeeds when
        // it crawls past the drive root. Same hazard on any Unix host
        // that happens to have `/tools`. Skip the assertion in that
        // case — the test only makes sense on a clean filesystem.
        if any_ancestor_has_tools(tmp.path()) {
            eprintln!(
                "skipping: an ancestor of {} has a tools/ dir; \
                 this host violates the test precondition.",
                tmp.path().display(),
            );
            return;
        }
        let start = tmp.path().to_path_buf();
        let err = discover_project_root(&start).expect_err("should not find");
        assert!(
            matches!(&err, DiscoveryError::NotFound(p) if p == &start),
            "unexpected error: {err:?}"
        );
    }

    fn any_ancestor_has_tools(start: &Path) -> bool {
        let mut current = match start.canonicalize() {
            Ok(p) => p,
            Err(_) => return false,
        };
        loop {
            if current.join("tools").is_dir() {
                return true;
            }
            if !current.pop() {
                return false;
            }
        }
    }

    #[test]
    fn returns_io_error_when_start_does_not_exist() {
        let bogus = std::path::Path::new("/this/path/definitely/does/not/exist/anywhere");
        let err = discover_project_root(bogus).expect_err("should fail to canonicalize");
        assert!(matches!(err, DiscoveryError::Io(p, _) if p == bogus));
    }
}
