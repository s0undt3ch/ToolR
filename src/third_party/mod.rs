//! Third-party static manifest fragment discovery, parsing, and merging.
//!
//! Packages ship a `toolr-manifest.json` at the root of their installed
//! Python package directory. This module globs for those files, validates
//! the mandatory `toolr_schema_version`, applies migrations, and merges
//! the resulting fragments into the project's static manifest.

pub mod glob;
pub mod migrate;
pub mod model;
pub mod parse;

pub use glob::glob_manifests;
pub use migrate::migrate_to_current;
pub use model::{
    FRAGMENT_SCHEMA_VERSION, FragmentArgument, FragmentCommand, FragmentGroup, ManifestFragment,
};
pub use parse::{ThirdPartyError, parse_fragment};

#[cfg(test)]
mod tests;
