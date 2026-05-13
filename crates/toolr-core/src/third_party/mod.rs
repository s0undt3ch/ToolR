//! Third-party static manifest fragment discovery, parsing, and merging.
//!
//! Packages ship a `toolr-manifest.json` at the root of their installed
//! Python package directory. This module globs for those files, validates
//! the mandatory `toolr_schema_version`, applies migrations, and merges
//! the resulting fragments into the project's static manifest.

pub mod glob;
pub mod merge;
pub mod migrate;
pub mod model;
pub mod parse;

pub use glob::glob_manifests;
pub use merge::merge_into_manifest;
pub use migrate::migrate_to_current;
pub use model::{
    FRAGMENT_SCHEMA_VERSION, FragmentArgument, FragmentCommand, FragmentGroup, ManifestFragment,
};
pub use parse::{ThirdPartyError, parse_fragment};

#[cfg(test)]
mod tests;

use std::path::Path;

use crate::manifest::Manifest;

/// Glob for fragments under `tools_venv`, parse + migrate each, and merge
/// them into `base`. Returns the augmented manifest.
///
/// Failure modes (any one fragment failing aborts the whole merge so
/// users see the broken package immediately rather than silently missing
/// commands):
/// - Malformed JSON in any fragment → `ThirdPartyError::Json`.
/// - Missing/invalid `toolr_schema_version` → `MissingVersion`.
/// - Version newer than this binary → `UnknownVersion`.
/// - Third-party-to-third-party command collision → `DuplicateCommand`.
pub fn discover_and_merge(tools_venv: &Path, base: Manifest) -> Result<Manifest, ThirdPartyError> {
    let paths = glob_manifests(tools_venv)?;
    let mut fragments = Vec::with_capacity(paths.len());
    for path in paths {
        fragments.push(parse_fragment(&path)?);
    }
    merge_into_manifest(base, fragments)
}
