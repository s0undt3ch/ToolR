//! Parse `[tool.toolr.argparse.*]` blocks from `tools/pyproject.toml`.

use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

use crate::manifest::ArgumentKind;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ArgparseBlock {
    /// Block key under `[tool.toolr.argparse.<name>]`. Set by the parser
    /// from the table key, not from a field on the table.
    #[serde(skip_deserializing, default)]
    pub name: String,
    #[serde(default)]
    pub scan_paths: Vec<String>,
    #[serde(default)]
    pub common_args: Vec<CommonArg>,
    #[serde(default)]
    pub attach: Vec<Attachment>,
}

impl ArgparseBlock {
    pub fn attachments(&self) -> &[Attachment] {
        &self.attach
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CommonArg {
    pub name: String,
    pub kind: ArgumentKind,
    #[serde(default)]
    pub help: String,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub choices: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Attachment {
    pub parent: String,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to parse tool.toolr.argparse: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("failed to read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Public: parse blocks from a raw TOML string. Convenient for tests.
pub fn parse_blocks(toml_text: &str) -> Result<Vec<ArgparseBlock>, ConfigError> {
    #[derive(Deserialize)]
    struct Root {
        #[serde(default)]
        tool: Tool,
    }
    #[derive(Default, Deserialize)]
    struct Tool {
        #[serde(default)]
        toolr: Toolr,
    }
    #[derive(Default, Deserialize)]
    struct Toolr {
        #[serde(default)]
        argparse: std::collections::BTreeMap<String, ArgparseBlock>,
    }
    let root: Root = toml::from_str(toml_text)?;
    Ok(root
        .tool
        .toolr
        .argparse
        .into_iter()
        .map(|(name, mut block)| {
            block.name = name;
            block
        })
        .collect())
}

/// Public: read `pyproject.toml` from disk and parse.
pub fn parse_blocks_from_pyproject(
    pyproject: &Path,
) -> Result<Vec<ArgparseBlock>, ConfigError> {
    let text = std::fs::read_to_string(pyproject).map_err(|source| ConfigError::Io {
        path: pyproject.display().to_string(),
        source,
    })?;
    parse_blocks(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_one_block_two_attachments() {
        let toml_text = r#"
            [tool.toolr.argparse.django]
            scan_paths = ["apps/*/management/commands/*.py"]
            common_args = [
              { name = "verbosity", kind = "optional", default = "1" },
            ]

            [[tool.toolr.argparse.django.attach]]
            parent = "django"

            [[tool.toolr.argparse.django.attach]]
            parent = "jenkins.job"
        "#;
        let blocks = parse_blocks(toml_text).unwrap();
        assert_eq!(blocks.len(), 1);
        let block = &blocks[0];
        assert_eq!(block.name, "django");
        assert_eq!(block.scan_paths, vec!["apps/*/management/commands/*.py"]);
        assert_eq!(block.common_args.len(), 1);
        assert_eq!(
            block.attachments().iter().map(|a| a.parent.as_str()).collect::<Vec<_>>(),
            vec!["django", "jenkins.job"],
        );
    }

    #[test]
    fn empty_table_returns_empty() {
        assert!(parse_blocks("[project]\nname = 'x'\n").unwrap().is_empty());
    }
}
