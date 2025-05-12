#![allow(non_local_definitions)]

mod command;
#[cfg(feature = "python")]
mod python_bindings;

// Re-export the core functionality for direct Rust usage
pub use command::{
    CommandConfig, run_command_internal,
    CommandExecutionError, CommandTimeoutExceededError, CommandNoOutputTimeoutError
};

// Re-export Python module
#[cfg(feature = "python")]
pub use python_bindings::_command;
