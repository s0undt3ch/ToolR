//! Build a third-party `ManifestFragment` from a plugin's source tree.
//!
//! Pure-Rust replacement for the legacy Python `toolr.build` module.
//! Walks the package's `.py` files via the same builder as
//! `build_static_manifest`, with the package name as the module-path
//! prefix, and ships the resulting `Group`s and `Command`s unchanged.

use std::path::{Path, PathBuf};

use crate::manifest::Origin;
use crate::parser::BuildError;
use crate::parser::build::build_commands;
use crate::third_party::ManifestFragment;

/// Error type for `build_third_party_fragment`.
#[derive(Debug, thiserror::Error)]
pub enum BuildFragmentError {
    #[error(
        "source directory `{path}` is a namespace package (no __init__.py); namespace packages are not supported"
    )]
    NamespacePackage { path: PathBuf },
    #[error("source directory `{path}` does not exist or is not a directory")]
    MissingSourceDir { path: PathBuf },
    #[error("package `{package}` declares no toolr commands - nothing to write")]
    EmptyPackage { package: String },
    #[error(transparent)]
    Build(#[from] BuildError),
}

/// A plugin's fragment and its build's warnings, which the fragment doesn't
/// keep.
#[derive(Debug)]
pub struct BuiltFragment {
    pub fragment: ManifestFragment,
    pub warnings: Vec<String>,
}

/// Build a `ManifestFragment` for `package_name` by AST-walking
/// `source_dir`.
/// See spec `specs/archive/2026/2026-05-22-rust-build-manifest-design.md`.
pub fn build_third_party_fragment(
    source_dir: &Path,
    package_name: &str,
) -> Result<BuiltFragment, BuildFragmentError> {
    // Reject missing dirs and namespace packages up front.
    if !source_dir.is_dir() {
        return Err(BuildFragmentError::MissingSourceDir {
            path: source_dir.to_path_buf(),
        });
    }
    if !source_dir.join("__init__.py").is_file() {
        return Err(BuildFragmentError::NamespacePackage {
            path: source_dir.to_path_buf(),
        });
    }

    let built = build_commands(source_dir, package_name)?;
    let (mut all_groups, mut all_commands) = (built.groups, built.commands);

    if all_commands.is_empty() {
        return Err(BuildFragmentError::EmptyPackage {
            package: package_name.to_string(),
        });
    }

    // Sort: groups by full path, commands by (group, name).
    all_groups.sort_by_key(|g| g.full_path());
    all_commands.sort_by(|a, b| {
        (a.group.as_str(), a.name.as_str()).cmp(&(b.group.as_str(), b.name.as_str()))
    });

    for group in &mut all_groups {
        group.origin = Origin::ThirdParty;
    }
    for command in &mut all_commands {
        command.origin = Origin::ThirdParty;
    }

    let mut fragment = ManifestFragment {
        toolr_schema_version: 0,
        package: package_name.to_string(),
        groups: all_groups,
        commands: all_commands,
    };
    fragment.toolr_schema_version = fragment.min_schema();
    Ok(BuiltFragment {
        fragment,
        warnings: built.warnings,
    })
}

/// Serialise a fragment to the canonical on-disk form: 2-space indent,
/// keys sorted at every depth, trailing newline. Round-trips through
/// `serde_json::Value` so the default BTreeMap-backed `Map` enforces
/// alphabetical key order — matches Python's
/// `json.dumps(fragment, indent=2, sort_keys=True) + "\n"`.
pub fn serialise_fragment(fragment: &ManifestFragment) -> Result<String, serde_json::Error> {
    let value = serde_json::to_value(fragment)?;
    let mut out = serde_json::to_string_pretty(&value)?;
    out.push('\n');
    Ok(out)
}

#[cfg(test)]
mod tests;
