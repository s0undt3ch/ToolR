#![allow(non_local_definitions)]

#[cfg(feature = "full")]
pub mod argparse;
#[cfg(feature = "full")]
pub mod build_fragment;
#[cfg(feature = "full")]
pub mod cache;
mod command;
#[cfg(feature = "full")]
pub mod complete;
#[cfg(feature = "full")]
pub mod discovery;
pub mod docstrings;
#[cfg(feature = "full")]
pub mod execute;
#[cfg(feature = "full")]
pub mod freshness;
#[cfg(feature = "full")]
pub mod glob_path;
#[cfg(feature = "full")]
pub mod hash;
#[cfg(feature = "full")]
pub mod manifest;
#[cfg(feature = "full")]
pub mod manifest_build;
#[cfg(feature = "full")]
pub mod parser;
#[cfg(feature = "full")]
pub mod project;
#[cfg(all(test, feature = "full"))]
mod test_env;
#[cfg(feature = "full")]
pub mod text;
#[cfg(feature = "full")]
pub mod third_party;
#[cfg(feature = "full")]
pub mod trust;
#[cfg(feature = "full")]
pub mod uv;
#[cfg(feature = "full")]
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

#[cfg(feature = "full")]
pub use build_fragment::{BuildFragmentError, build_third_party_fragment, serialise_fragment};
