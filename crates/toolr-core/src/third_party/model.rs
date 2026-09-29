//! Serde model for a third-party manifest fragment.
//!
//! A fragment carries the manifest's own `Group` and `Command` types unchanged, so a plugin
//! command behaves exactly like a local one. It lacks the host manifest's hashes and instead
//! carries the mandatory `toolr_schema_version` discriminator.

use serde::{Deserialize, Serialize};

use crate::manifest::{Command, FRAGMENT_SHAPE_SCHEMA, Group};
use crate::parser::types::SupportedTypeKind;

// region: SkillRefManifestFragment
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestFragment {
    /// Lowest toolr schema a reader needs to load this fragment, not the schema of the toolr
    /// that built it.
    pub toolr_schema_version: u32,
    /// The Python package name this fragment came from. Used for
    /// diagnostic messages and de-duplication.
    pub package: String,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub commands: Vec<Command>,
}
// endregion: SkillRefManifestFragment

impl ManifestFragment {
    /// The lowest reader schema that can load every group and command in this fragment.
    pub fn min_schema(&self) -> u32 {
        self.min_schema_with(&SupportedTypeKind::since_schema)
    }

    /// `min_schema` with the type-kind lookup injected, so tests can fold a kind newer than 2.
    pub fn min_schema_with(&self, since: &impl Fn(SupportedTypeKind) -> u32) -> u32 {
        let groups = self.groups.iter().map(Group::min_schema);
        let commands = self.commands.iter().map(|c| c.min_schema_with(since));
        groups.chain(commands).fold(FRAGMENT_SHAPE_SCHEMA, u32::max)
    }
}
