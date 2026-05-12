//! Toolr venv cache: per-venv metadata sidecar, enumeration, pruning,
//! and passive size hints.

pub mod init;
pub mod meta;
pub mod touch;

pub use init::write_meta_for_new_venv;
pub use meta::{Meta, MetaError, SCHEMA_VERSION};
pub use touch::touch_last_used;

#[cfg(test)]
mod tests;
