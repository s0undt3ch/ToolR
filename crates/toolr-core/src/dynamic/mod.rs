//! Dynamic manifest layer: spawn a Python introspection helper inside
//! the tools venv and merge the result into the manifest.

pub mod hash;
pub mod merge;
pub mod payload;
pub mod rebuild;
pub mod runner;

pub use hash::{compute_third_party_hash, empty_third_party_hash};
pub use merge::merge_dynamic;
pub use payload::{DynamicPayload, PAYLOAD_SCHEMA_VERSION};
pub use rebuild::{RebuildOutcome, rebuild_manifest_full};
pub use runner::{IntrospectError, run_introspect};

#[cfg(test)]
mod tests;
