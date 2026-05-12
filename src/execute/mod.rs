//! Subprocess execution of user commands via `python -m toolr._runner`.

pub mod spec;
pub mod tempfile;

pub use spec::{ContextSpec, ExecutionSpec, RUNNER_SCHEMA_VERSION};
pub use tempfile::write_spec_to_tempfile;
