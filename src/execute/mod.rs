//! Subprocess execution of user commands via `python -m toolr._runner`.

pub mod spec;

pub use spec::{ContextSpec, ExecutionSpec, RUNNER_SCHEMA_VERSION};
