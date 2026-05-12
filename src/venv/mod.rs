//! Tools venv resolution, configuration, and lifecycle.

pub mod config;

pub use config::{ToolrConfig, VenvLocation, load_toolr_config};
