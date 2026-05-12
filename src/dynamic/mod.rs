//! Dynamic manifest layer: spawn a Python introspection helper inside
//! the tools venv and merge the result into the manifest.

pub mod hash;
pub mod payload;
pub mod runner;

pub use hash::compute_dynamic_hash;
pub use payload::{DynamicPayload, PAYLOAD_SCHEMA_VERSION};
pub use runner::{IntrospectError, run_introspect};

#[cfg(test)]
mod tests;
