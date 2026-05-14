#![allow(non_local_definitions)]

pub mod cache;
mod command;
pub mod complete;
pub mod deps_check;
pub mod discovery;
pub mod dynamic;
mod docstrings;
pub mod execute;
pub mod hash;
pub mod manifest;
pub mod parser;
pub mod project;
pub mod third_party;
pub mod uv;
pub mod venv;

// Re-export the core functionality for direct Rust usage
pub use command::{
    CommandConfig, run_command_internal,
    CommandExecutionError, CommandTimeoutExceededError, CommandNoOutputTimeoutError
};

#[cfg(windows)]
pub use command::ThreadSafeHandle;

// Re-export docstring parsing functionality
pub use docstrings::{
    Docstring, Example, ParseError,
    SimpleDocstringParser
};
