//! Resolve the absolute path where the tools venv should live.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::config::{ToolrConfig, VenvLocation, load_toolr_config, read_requires_python};
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

    let (venv_dir, repo_key) = match config.venv_location {
        VenvLocation::InTree => (tools.join(".venv"), String::new()),
        VenvLocation::Cache => {
            let key = compute_repo_key(repo_root, &python_version)?;
            let base = toolr_cache_dir()
                .ok_or_else(|| anyhow::anyhow!("could not resolve toolr cache directory"))?;
            (base.join(&key).join("venv"), key)
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
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_repo(body: &str) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        std::fs::create_dir(&tools).unwrap();
        std::fs::write(tools.join("pyproject.toml"), body).unwrap();
        tmp
    }

    #[test]
    fn cache_default_uses_repo_key_subdir() {
        let tmp = setup_repo("[project]\nname=\"x\"\nversion=\"0\"\n");
        let resolved = resolve_venv_path(tmp.path()).unwrap();
        assert!(resolved.venv_dir.ends_with("venv"));
        assert!(!resolved.repo_key.is_empty());
        assert!(
            resolved
                .venv_dir
                .to_string_lossy()
                .contains(&resolved.repo_key)
        );
    }

    #[test]
    fn in_tree_lands_inside_tools_dot_venv() {
        let tmp = setup_repo(
            "[project]\nname=\"x\"\nversion=\"0\"\n\n[tool.toolr]\nvenv-location = \"in-tree\"\n",
        );
        let resolved = resolve_venv_path(tmp.path()).unwrap();
        assert_eq!(resolved.venv_dir, tmp.path().join("tools").join(".venv"));
        assert!(resolved.repo_key.is_empty());
    }
}
