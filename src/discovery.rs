//! Walk upward to locate the project root (parent of `tools/`).

use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("no `tools/` directory found above {0}")]
    NotFound(PathBuf),
}

/// Walk up from `start` until a directory containing `tools/` is found.
/// Returns that directory (the project root). The check stops at the
/// filesystem root.
pub fn discover_project_root(start: &Path) -> Result<PathBuf, DiscoveryError> {
    let mut current = start.to_path_buf();
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
        assert_eq!(root, tmp.path());
    }

    #[test]
    fn finds_tools_in_ancestor() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("tools")).unwrap();
        let nested = tmp.path().join("a").join("b").join("c");
        std::fs::create_dir_all(&nested).unwrap();
        let root = discover_project_root(&nested).unwrap();
        assert_eq!(root, tmp.path());
    }

    #[test]
    fn returns_not_found_when_no_tools_dir_exists() {
        let tmp = TempDir::new().unwrap();
        let err = discover_project_root(tmp.path()).expect_err("should not find");
        assert!(matches!(err, DiscoveryError::NotFound(_)));
    }
}
