//! Serde-derived types representing a loaded manifest.

use serde::{Deserialize, Serialize};

/// Current manifest schema version. Bump on breaking format changes.
pub const SCHEMA_VERSION: u32 = 1;

/// Top-level manifest document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    /// Hash over `tools/**/*.py` contents — used for fast freshness checks.
    pub static_hash: String,
    /// Hash over the installed package set (versions). Empty until Plan 6
    /// adds dynamic-layer support.
    #[serde(default)]
    pub dynamic_hash: String,
    pub groups: Vec<Group>,
    pub commands: Vec<Command>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    /// Lowercase group name (e.g. "ci").
    pub name: String,
    /// Short title shown in `--help`.
    pub title: String,
    /// Optional longer description.
    #[serde(default)]
    pub description: String,
    /// Where this group entry came from.
    pub origin: Origin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Command {
    /// Lowercase command name (e.g. "generate-build-matrix").
    pub name: String,
    /// Parent group name.
    pub group: String,
    /// Module path used by the runner to import.
    pub module: String,
    /// Python function name within that module.
    pub function: String,
    /// First line of the docstring; used in `--help` summaries.
    #[serde(default)]
    pub summary: String,
    /// Full description (rest of the docstring after the first line).
    #[serde(default)]
    pub description: String,
    /// Ordered list of arguments.
    pub arguments: Vec<Argument>,
    /// Top-level imports recorded by the static parser (used by Plan 7).
    #[serde(default)]
    pub imports: Vec<String>,
    /// Where this command entry came from.
    pub origin: Origin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Argument {
    pub name: String,
    pub kind: ArgumentKind,
    #[serde(default)]
    pub help: String,
    /// String-encoded default. `None` means required.
    #[serde(default)]
    pub default: Option<String>,
    /// Argument's type annotation as written in source (best-effort).
    #[serde(default)]
    pub type_annotation: Option<String>,
    /// For Literal[...] / Enum-backed args, the allowed value strings.
    #[serde(default)]
    pub allowed_values: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArgumentKind {
    Positional,
    Optional,
    Flag,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Static,
    Dynamic,
}
