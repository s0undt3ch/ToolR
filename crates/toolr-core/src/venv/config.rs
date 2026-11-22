//! Parse the `[tool.toolr]` table out of `tools/pyproject.toml`.

use std::path::Path;
use std::str::FromStr;

use serde::Deserialize;
use thiserror::Error;

/// Environment variable that overrides `[tool.toolr] venv-location`
/// in `tools/pyproject.toml`. Accepts the same kebab-case spellings
/// the TOML key does (`in-tree` / `cache`). Primarily a CI escape
/// hatch — the per-project default belongs in `pyproject.toml`.
pub const VENV_LOCATION_ENV: &str = "TOOLR_VENV_LOCATION";

/// Where the tools venv should be materialised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum VenvLocation {
    /// Default: `$XDG_CACHE_HOME/toolr/<repo-key>/venv/`.
    #[default]
    Cache,
    /// Opt-in: `tools/.venv/`.
    InTree,
}

impl FromStr for VenvLocation {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "in-tree" | "intree" => Ok(VenvLocation::InTree),
            "cache" => Ok(VenvLocation::Cache),
            other => Err(format!(
                "expected `in-tree` or `cache` for VenvLocation, got `{other}`",
            )),
        }
    }
}

/// Strongly-typed view of the `[tool.toolr]` table.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ToolrConfig {
    #[serde(default)]
    pub venv_location: VenvLocation,
    /// Optional explicit Python version override.
    #[serde(default)]
    pub python_version: Option<String>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing tools/pyproject.toml at {0}")]
    Missing(std::path::PathBuf),
    #[error("I/O error reading pyproject.toml: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid TOML in pyproject.toml: {0}")]
    Toml(#[from] toml::de::Error),
}

/// Read `tools/pyproject.toml` and extract `[tool.toolr]` (or defaults).
pub fn load_toolr_config(tools_dir: &Path) -> Result<ToolrConfig, ConfigError> {
    let path = tools_dir.join("pyproject.toml");
    if !path.is_file() {
        return Err(ConfigError::Missing(path));
    }
    let raw = std::fs::read_to_string(&path)?;
    #[derive(Deserialize)]
    struct Root {
        #[serde(default)]
        tool: ToolTable,
    }
    #[derive(Deserialize, Default)]
    struct ToolTable {
        #[serde(default)]
        toolr: ToolrConfig,
    }
    let root: Root = toml::from_str(&raw)?;
    Ok(root.tool.toolr)
}

/// Extract `requires-python` from the `[project]` table. Used as a
/// fallback when `[tool.toolr] python-version` is unset.
pub fn read_requires_python(tools_dir: &Path) -> Result<Option<String>, ConfigError> {
    let path = tools_dir.join("pyproject.toml");
    if !path.is_file() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    #[derive(Deserialize)]
    struct Root {
        #[serde(default)]
        project: ProjectTable,
    }
    #[derive(Deserialize, Default)]
    struct ProjectTable {
        #[serde(default, rename = "requires-python")]
        requires_python: Option<String>,
    }
    let root: Root = toml::from_str(&raw)?;
    Ok(root.project.requires_python)
}

#[cfg(test)]
mod tests;
