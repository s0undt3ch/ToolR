//! Wire format for the dynamic-layer introspection payload.

use serde::{Deserialize, Serialize};

use crate::manifest::{Command, Group, Origin};

/// Wire-protocol version between `toolr._introspect` and the Rust side.
/// Bump on breaking changes to `DynamicPayload`.
pub const PAYLOAD_SCHEMA_VERSION: u32 = 1;

/// JSON payload written to stdout by `python -m toolr._introspect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DynamicPayload {
    /// Schema version of the payload itself (NOT the manifest version).
    pub payload_schema_version: u32,
    /// Groups discovered by importing `tools.*` modules. Third-party
    /// packages contribute via the static layer, not via this payload.
    pub groups: Vec<Group>,
    /// Commands discovered the same way.
    pub commands: Vec<Command>,
    /// Non-fatal warnings the helper wants to surface (e.g. a tools
    /// module that failed to import). Each is a single human-readable
    /// line. Rust prints them to stderr after a successful merge.
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl DynamicPayload {
    /// Force every group / command in the payload to `Origin::Dynamic`,
    /// regardless of what the Python side emitted. The Rust side owns
    /// origin tagging — defence-in-depth against a misbehaving helper.
    pub fn retag_as_dynamic(mut self) -> Self {
        for g in &mut self.groups {
            g.origin = Origin::Dynamic;
        }
        for c in &mut self.commands {
            c.origin = Origin::Dynamic;
        }
        self
    }
}
