//! Walk upward to locate the project root: the nearest directory whose
//! `tools/` holds a `pyproject.toml`. A `tools/` without one is not toolr's,
//! whatever else it contains.

use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("I/O error resolving {0}: {1}")]
    Io(PathBuf, #[source] std::io::Error),
    #[error(
        "no toolr project found above {0}: no `tools/pyproject.toml`. \
         Run `toolr project init` to create one"
    )]
    NotFound(PathBuf),
    #[error(
        "no toolr project found above {start}: {} has no `pyproject.toml`. \
         toolr needs `tools/pyproject.toml`; run `toolr project init` there to create it",
        tools.display()
    )]
    MissingPyproject { start: PathBuf, tools: PathBuf },
}

/// Walk up from `start` to the nearest directory whose `tools/pyproject.toml`
/// is a file and return it as the project root.
///
/// `start` is canonicalized before the walk, so relative paths are
/// resolved against the process cwd up front and the returned path is
/// always absolute. If `start` cannot be canonicalized (e.g. it doesn't
/// exist), returns `DiscoveryError::Io`. If no project is found, returns
/// `DiscoveryError::MissingPyproject` naming the nearest `tools/` seen on
/// the way, or `DiscoveryError::NotFound` when there was none.
pub fn discover_project_root(start: &Path) -> Result<PathBuf, DiscoveryError> {
    discover_below(start, None)
}

/// [`discover_project_root`], stopping after `ceiling` when given, so tests
/// can ignore `tools/` directories on the host above their tempdir.
fn discover_below(start: &Path, ceiling: Option<&Path>) -> Result<PathBuf, DiscoveryError> {
    let mut current = start
        .canonicalize()
        .map_err(|e| DiscoveryError::Io(start.to_path_buf(), e))?;
    let mut first_bare: Option<PathBuf> = None;
    loop {
        let tools = current.join("tools");
        if tools.join("pyproject.toml").is_file() {
            return Ok(current);
        }
        if first_bare.is_none() && tools.is_dir() {
            first_bare = Some(tools);
        }
        if Some(current.as_path()) == ceiling || !current.pop() {
            let start = start.to_path_buf();
            return Err(match first_bare {
                Some(tools) => DiscoveryError::MissingPyproject { start, tools },
                None => DiscoveryError::NotFound(start),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// A tempdir and its canonical path, used as the walk's ceiling so a
    /// host `tools/` (the Windows runners' `C:\tools`) can't leak in.
    fn sandbox() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        (tmp, root)
    }

    fn mkfile(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    fn mkdir(path: &Path) -> PathBuf {
        std::fs::create_dir_all(path).unwrap();
        path.to_path_buf()
    }

    fn find(start: &Path, ceiling: &Path) -> Result<PathBuf, DiscoveryError> {
        discover_below(start, Some(ceiling))
    }

    #[test]
    fn finds_project_in_current_dir() {
        let (_tmp, root) = sandbox();
        mkfile(&root.join("tools/pyproject.toml"));
        assert_eq!(discover_project_root(&root).unwrap(), root);
    }

    #[test]
    fn finds_project_in_ancestor() {
        let (_tmp, root) = sandbox();
        mkfile(&root.join("tools/pyproject.toml"));
        let nested = mkdir(&root.join("a/b/c"));
        assert_eq!(discover_project_root(&nested).unwrap(), root);
    }

    #[test]
    fn skips_unrelated_package_tools_dir() {
        let (_tmp, root) = sandbox();
        mkfile(&root.join("tools/pyproject.toml"));
        mkfile(&root.join("tools/demo.py"));
        mkfile(&root.join("src/pkg/tools/__init__.py"));
        assert_eq!(discover_project_root(&root.join("src/pkg")).unwrap(), root);
    }

    #[test]
    fn package_tools_dir_without_pyproject_is_skipped_even_when_nearest() {
        let (_tmp, root) = sandbox();
        mkfile(&root.join("tools/pyproject.toml"));
        mkfile(&root.join("proj/tools/__init__.py"));
        mkfile(&root.join("proj/tools/demo.py"));
        assert_eq!(discover_project_root(&root.join("proj")).unwrap(), root);
    }

    #[test]
    fn project_wins_over_bare_ancestor_tools_dir() {
        let (_tmp, root) = sandbox();
        mkdir(&root.join("tools"));
        mkfile(&root.join("proj/tools/__init__.py"));
        mkfile(&root.join("proj/tools/pyproject.toml"));
        let proj = root.join("proj");
        assert_eq!(discover_project_root(&proj).unwrap(), proj);
    }

    #[test]
    fn nearest_project_wins_over_an_outer_one() {
        let (_tmp, root) = sandbox();
        mkfile(&root.join("tools/pyproject.toml"));
        mkfile(&root.join("sub/tools/pyproject.toml"));
        let start = mkdir(&root.join("sub/x"));
        assert_eq!(discover_project_root(&start).unwrap(), root.join("sub"));
    }

    #[test]
    fn directory_named_pyproject_toml_does_not_count() {
        let (_tmp, root) = sandbox();
        mkdir(&root.join("tools/pyproject.toml"));
        let err = find(&root, &root).expect_err("should not find");
        assert!(
            matches!(&err, DiscoveryError::MissingPyproject { tools, .. } if tools == &root.join("tools")),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn bare_tools_dir_without_project_names_it() {
        let (_tmp, root) = sandbox();
        mkfile(&root.join("tools/__init__.py"));
        mkfile(&root.join("a/tools/demo.py"));
        let start = mkdir(&root.join("a/b"));
        let err = find(&start, &root).expect_err("should not find");
        let tools = root.join("a/tools");
        assert!(
            matches!(&err, DiscoveryError::MissingPyproject { start: s, tools: t } if s == &start && t == &tools),
            "unexpected error: {err:?}"
        );
        let message = err.to_string();
        assert!(message.contains(&tools.display().to_string()), "{message}");
        assert!(
            message.contains("toolr needs `tools/pyproject.toml`"),
            "{message}"
        );
        assert!(message.contains("toolr project init"), "{message}");
    }

    #[test]
    fn returns_not_found_when_no_tools_dir_exists() {
        let (_tmp, root) = sandbox();
        let err = find(&root, &root).expect_err("should not find");
        assert!(
            matches!(&err, DiscoveryError::NotFound(p) if p == &root),
            "unexpected error: {err:?}"
        );
        assert!(err.to_string().contains("toolr project init"), "{err}");
    }

    #[test]
    fn ceiling_stops_the_walk() {
        let (_tmp, root) = sandbox();
        mkfile(&root.join("tools/pyproject.toml"));
        let inner = mkdir(&root.join("inner"));
        let err = find(&inner, &inner).expect_err("should stop at the ceiling");
        assert!(matches!(err, DiscoveryError::NotFound(p) if p == inner));
    }

    #[test]
    fn returns_io_error_when_start_does_not_exist() {
        let bogus = std::path::Path::new("/this/path/definitely/does/not/exist/anywhere");
        let err = discover_project_root(bogus).expect_err("should fail to canonicalize");
        assert!(matches!(err, DiscoveryError::Io(p, _) if p == bogus));
    }
}
