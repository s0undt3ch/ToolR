#![allow(non_local_definitions)]

pub mod argparse;
pub mod build_fragment;
pub mod cache;
mod command;
pub mod complete;
pub mod discovery;
pub mod docstrings;
pub mod execute;
pub mod freshness;
pub mod glob_path;
pub mod hash;
pub mod manifest;
pub mod manifest_build;
pub mod parser;
pub mod path_search;
pub mod project;
#[cfg(test)]
mod test_env;
#[cfg(test)]
mod test_fs;
pub mod text;
pub mod third_party;
pub mod trust;
pub mod uv;
pub mod venv;

// Re-export the core functionality for direct Rust usage
pub use command::{
    CommandConfig, CommandExecutionError, CommandNoOutputTimeoutError, CommandTimeoutExceededError,
    run_command_internal,
};

#[cfg(windows)]
pub use command::ThreadSafeHandle;

// Re-export docstring parsing functionality
pub use docstrings::{Docstring, Example, ParseError, SimpleDocstringParser};

pub use build_fragment::{BuildFragmentError, build_third_party_fragment, serialise_fragment};
