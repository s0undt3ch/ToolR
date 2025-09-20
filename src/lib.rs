#![allow(non_local_definitions)]

mod command;
mod docstrings;
#[cfg(feature = "python")]
mod python_bindings;

// Re-export the core functionality for direct Rust usage
pub use command::{
    CommandConfig, run_command_internal,
    CommandExecutionError, CommandTimeoutExceededError, CommandNoOutputTimeoutError
};

// Re-export docstring parsing functionality
pub use docstrings::{
    Docstring, Example, ParseError,
    SimpleDocstringParser
};

// Re-export Python modules
#[cfg(feature = "python")]
pub use python_bindings::_rust_utils;
