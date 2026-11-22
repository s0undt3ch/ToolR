//! Resolve the absolute path where the tools venv should live.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result};

use super::config::{
    ToolrConfig, VENV_LOCATION_ENV, VenvLocation, load_toolr_config, read_requires_python,
};
use super::repo_key::compute_repo_key;
use crate::uv::toolr_cache_dir;

/// Output of venv resolution.
#[derive(Debug, Clone)]
pub struct ResolvedVenv {
    /// Where the venv directory lives.
    pub venv_dir: PathBuf,
    /// `<venv>/bin/python` (or `Scripts\\python.exe` on Windows).
    pub python: PathBuf,
    /// The repo-key used in the cache layout (empty when in-tree).
    pub repo_key: String,
    /// Python version string used as a hash input (best-effort).
    pub python_version: String,
    /// Source `tools/pyproject.toml` config.
    pub config: ToolrConfig,
}

impl ResolvedVenv {
    /// True when the venv lives in the repo (`tools/.venv`), not the cache.
    pub fn is_in_tree(&self) -> bool {
        self.repo_key.is_empty()
    }

    /// The venv directory, unless it is an in-tree venv toolr didn't build (see
    /// [`super::is_foreign`]): for callers that only read the venv's files, such as the plugin
    /// fragment glob. Callers that act on a foreign venv use the freshness check instead.
    pub fn usable_venv_dir(&self, root: &Path) -> Option<&Path> {
        (!super::sync::is_foreign(self, root)).then_some(self.venv_dir.as_path())
    }
}

/// Suffix of the cache slot that holds only an in-tree venv's `meta.json`.
/// It keeps that slot apart from the one the repo's cache venv uses, so a
/// repo that moved to in-tree stops refreshing its abandoned `venv/`.
pub const IN_TREE_SLOT_SUFFIX: &str = "-in-tree";

/// The repo's slot under `cache_root` (toolr's cache dir): `<repo-key>/` for a
/// cache venv and its sidecar, `<repo-key>-in-tree/` for an in-tree venv's sidecar.
pub fn cache_slot_dir(
    cache_root: &Path,
    repo_root: &Path,
    python_version: &str,
    location: VenvLocation,
) -> Result<PathBuf> {
    let key = compute_repo_key(repo_root, python_version)?;
    Ok(match location {
        VenvLocation::Cache => cache_root.join(key),
        VenvLocation::InTree => cache_root.join(format!("{key}{IN_TREE_SLOT_SUFFIX}")),
    })
}

/// Resolve the tools venv path for the given repo root.
pub fn resolve_venv_path(repo_root: &Path) -> Result<ResolvedVenv> {
    let tools = repo_root.join("tools");
    let config = load_toolr_config(&tools)
        .with_context(|| format!("loading tools/pyproject.toml under {}", repo_root.display()))?;
    let python_version = config
        .python_version
        .clone()
        .or(read_requires_python(&tools).ok().flatten())
        .unwrap_or_default();

    // Env-var override of the `[tool.toolr] venv-location` setting.
    // Primary use case: CI workflows that want to opt every repo into
    // in-tree venvs without rewriting each one's `tools/pyproject.toml`.
    // An invalid spelling is a hard error so a typo doesn't silently
    // fall back to the file-configured value.
    let venv_location = match std::env::var(VENV_LOCATION_ENV) {
        Ok(raw) if !raw.is_empty() => VenvLocation::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("{VENV_LOCATION_ENV}={raw:?} is invalid: {e}"))?,
        _ => config.venv_location,
    };

    let (venv_dir, repo_key) = match venv_location {
        VenvLocation::InTree => (tools.join(".venv"), String::new()),
        VenvLocation::Cache => {
            let base = toolr_cache_dir()
                .ok_or_else(|| anyhow::anyhow!("could not resolve toolr cache directory"))?;
            let slot = cache_slot_dir(&base, repo_root, &python_version, VenvLocation::Cache)?;
            let key = compute_repo_key(repo_root, &python_version)?;
            (slot.join("venv"), key)
        }
    };

    let python = if cfg!(windows) {
        venv_dir.join("Scripts").join("python.exe")
    } else {
        venv_dir.join("bin").join("python")
    };

    Ok(ResolvedVenv {
        venv_dir,
        python,
        repo_key,
        python_version,
        config,
    })
}

#[cfg(test)]
mod tests;
