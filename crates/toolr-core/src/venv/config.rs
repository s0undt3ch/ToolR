//! Parse the `[tool.toolr]` table out of `tools/pyproject.toml`.

use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

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

/// Strongly-typed view of the `[tool.toolr]` table.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ToolrConfig {
    #[serde(default)]
    pub venv_location: VenvLocation,
    /// Opt-in editable installs run post-`uv sync`. E.g. `["."]`.
    #[serde(default)]
    pub editable_install: Vec<String>,
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
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_pyproject(tools: &Path, body: &str) {
        std::fs::create_dir_all(tools).unwrap();
        std::fs::write(tools.join("pyproject.toml"), body).unwrap();
    }

    #[test]
    fn defaults_when_table_is_absent() {
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        write_pyproject(&tools, "[project]\nname=\"x\"\nversion=\"0\"\n");
        let cfg = load_toolr_config(&tools).unwrap();
        assert_eq!(cfg.venv_location, VenvLocation::Cache);
        assert!(cfg.editable_install.is_empty());
        assert!(cfg.python_version.is_none());
    }

    #[test]
    fn parses_in_tree_venv_location() {
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        write_pyproject(
            &tools,
            r#"
[project]
name = "x"
version = "0"

[tool.toolr]
venv-location = "in-tree"
editable-install = ["."]
python-version = "3.13"
"#,
        );
        let cfg = load_toolr_config(&tools).unwrap();
        assert_eq!(cfg.venv_location, VenvLocation::InTree);
        assert_eq!(cfg.editable_install, vec![".".to_string()]);
        assert_eq!(cfg.python_version.as_deref(), Some("3.13"));
    }

    #[test]
    fn reports_missing_pyproject() {
        let tmp = TempDir::new().unwrap();
        let err = load_toolr_config(&tmp.path().join("tools")).unwrap_err();
        assert!(matches!(err, ConfigError::Missing(_)));
    }

    #[test]
    fn reads_requires_python() {
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        write_pyproject(
            &tools,
            "[project]\nname=\"x\"\nversion=\"0\"\nrequires-python = \">=3.11\"\n",
        );
        let v = read_requires_python(&tools).unwrap();
        assert_eq!(v.as_deref(), Some(">=3.11"));
    }
}
