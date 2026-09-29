//! Third-party static manifest fragment discovery, parsing, and merging.
//!
//! Packages ship a `toolr-manifest.json` at the root of their installed
//! Python package directory. This module globs for those files, applies the
//! `toolr_schema_version` load rule, and merges the resulting fragments into
//! the project's static manifest.

pub mod glob;
pub mod merge;
pub mod model;
pub mod parse;

pub use glob::glob_manifests;
pub use merge::merge_into_manifest;
pub use model::ManifestFragment;
pub use parse::{ParsedFragment, ThirdPartyError, parse_fragment};

#[cfg(test)]
mod tests;

use std::path::Path;

use crate::manifest::Manifest;

/// Glob for fragments under `tools_venv`, parse each, and merge them
/// into `base`. Returns the augmented manifest, with every skipped plugin
/// and then every shadowed plugin command recorded in `plugin_warnings`.
///
/// A plugin outside the load rule, or with an invalid argument, is skipped
/// so it can't break the CLI. These still abort the whole merge:
/// - Malformed JSON in any fragment → `ThirdPartyError::Json`.
/// - Missing/invalid `toolr_schema_version` → `MissingVersion`.
/// - Third-party-to-third-party command collision → `DuplicateCommand`.
pub fn discover_and_merge(
    tools_venv: &Path,
    mut base: Manifest,
) -> Result<Manifest, ThirdPartyError> {
    let paths = glob_manifests(tools_venv)?;
    let mut fragments = Vec::with_capacity(paths.len());
    for path in paths {
        match parse_fragment(&path)? {
            ParsedFragment::Loaded(fragment, path) => fragments.push((fragment, path)),
            ParsedFragment::Skipped(warning) => base.plugin_warnings.push(warning),
        }
    }
    merge_into_manifest(base, fragments)
}
