//! Subprocess execution of user commands via `python -m toolr._runner`.

pub mod build;
pub mod python;
pub mod signals;
pub mod spawn;
pub mod spec;
pub mod tempfile;

pub use build::{OutputOptions, build_spec};
pub use python::{PythonError, resolve_python};
pub use signals::wait_with_signals;
pub use spawn::{StderrCapture, spawn_runner, spawn_runner_capturing_stderr};
pub use spec::{ContextSpec, ExecutionSpec, RUNNER_SCHEMA_VERSION};
pub use tempfile::write_spec_to_tempfile;
