//! Toolr venv cache: per-venv metadata sidecar, enumeration, pruning,
//! and passive size hints.

pub mod meta;

pub use meta::{Meta, MetaError, SCHEMA_VERSION};

#[cfg(test)]
mod tests;
