//! Pre-flight: check that all of a command's top-level imports exist
//! in the tools venv's `site-packages`.

use std::fmt;
use std::path::Path;

use super::probe::{ProbeOutcome, probe_module};

/// One or more imports were not found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingDeps {
    /// Missing module names, preserved in the order they were probed.
    pub missing: Vec<String>,
}

impl std::error::Error for MissingDeps {}

impl fmt::Display for MissingDeps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.missing.as_slice() {
            [] => write!(f, "no missing imports"),
            [one] => write!(
                f,
                "import `{one}` not found in tools venv. \
                 A dependency may be missing - run \
                 `toolr project deps sync` and check tools/pyproject.toml."
            ),
            many => {
                let joined = many
                    .iter()
                    .map(|m| format!("`{m}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(
                    f,
                    "imports {joined} not found in tools venv. \
                     Dependencies may be missing - run \
                     `toolr project deps sync` and check tools/pyproject.toml."
                )
            }
        }
    }
}

/// Probe each import in order; collect those that come back `Missing`.
/// Returns `Ok(())` if every import resolves to a package or
/// single-file module under `site-packages`.
pub fn check_imports(site_packages: &Path, imports: &[String]) -> Result<(), MissingDeps> {
    let mut missing = Vec::new();
    for name in imports {
        if matches!(probe_module(site_packages, name), ProbeOutcome::Missing) {
            missing.push(name.clone());
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(MissingDeps { missing })
    }
}
