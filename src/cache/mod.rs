//! Toolr venv cache: per-venv metadata sidecar, enumeration, pruning,
//! and passive size hints.

pub mod init;
pub mod meta;

pub use init::write_meta_for_new_venv;
pub use meta::{Meta, MetaError, SCHEMA_VERSION};

#[cfg(test)]
mod tests;
