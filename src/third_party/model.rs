//! Serde model for a third-party manifest fragment.
//!
//! Distinct from `crate::manifest::Manifest` because fragments lack
//! `static_hash` / `dynamic_hash` / `origin` and instead carry the
//! mandatory `toolr_schema_version` discriminator.

use serde::{Deserialize, Serialize};

/// Current fragment schema version. The Rust binary accepts fragments at
/// version `<= FRAGMENT_SCHEMA_VERSION`, applying migrations as needed.
/// Fragments at a higher version are rejected.
pub const FRAGMENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestFragment {
    pub toolr_schema_version: u32,
    /// The Python package name this fragment came from. Used for
    /// diagnostic messages and de-duplication.
    pub package: String,
    #[serde(default)]
    pub groups: Vec<FragmentGroup>,
    #[serde(default)]
    pub commands: Vec<FragmentCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FragmentGroup {
    pub name: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FragmentCommand {
    pub name: String,
    pub group: String,
    pub module: String,
    pub function: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub arguments: Vec<FragmentArgument>,
    #[serde(default)]
    pub imports: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FragmentArgument {
    pub name: String,
    pub kind: crate::manifest::ArgumentKind,
    #[serde(default)]
    pub help: String,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub type_annotation: Option<String>,
    #[serde(default)]
    pub allowed_values: Vec<String>,
}
