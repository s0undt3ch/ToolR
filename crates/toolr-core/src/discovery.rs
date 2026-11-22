//! Walk upward to locate the project root: the nearest directory whose
//! `tools/` holds a `pyproject.toml`. A `tools/` without one is not toolr's,
//! whatever else it contains.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::trust::shown;

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("I/O error resolving {}: {}", shown(.0), .1)]
    Io(PathBuf, #[source] std::io::Error),
    #[error(
        "no toolr project found above {}: no `tools/pyproject.toml`. \
         Run `toolr project init` to create one",
        shown(.0)
    )]
    NotFound(PathBuf),
    #[error(
        "no toolr project found above {}: {} has no `pyproject.toml`. \
         toolr needs `tools/pyproject.toml`; run `toolr project init` there to create it",
        shown(start),
        shown(tools)
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
    // `dunce` drops Windows' `\\?\` verbatim prefix when the plain form is
    // valid: this root reaches Python as `ctx.repo_root` and is the runner's cwd.
    let mut current =
        dunce::canonicalize(start).map_err(|e| DiscoveryError::Io(start.to_path_buf(), e))?;
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
mod tests;
