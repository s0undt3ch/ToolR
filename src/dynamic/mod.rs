//! Dynamic manifest layer: spawn a Python introspection helper inside
//! the tools venv and merge the result into the manifest.

pub mod payload;

pub use payload::{DynamicPayload, PAYLOAD_SCHEMA_VERSION};

#[cfg(test)]
mod tests;
