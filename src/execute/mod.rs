//! Subprocess execution of user commands via `python -m toolr._runner`.

pub mod python;
pub mod spawn;
pub mod spec;
pub mod tempfile;

pub use python::{PythonError, resolve_python};
pub use spawn::spawn_runner;
pub use spec::{ContextSpec, ExecutionSpec, RUNNER_SCHEMA_VERSION};
pub use tempfile::write_spec_to_tempfile;
