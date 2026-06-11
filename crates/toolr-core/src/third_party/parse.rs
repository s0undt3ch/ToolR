//! Parse and validate a single third-party manifest fragment file.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::model::{FRAGMENT_SCHEMA_VERSION, ManifestFragment};

#[derive(Debug, Error)]
pub enum ThirdPartyError {
    #[error("non-UTF-8 path: {0}")]
    NonUtf8Path(PathBuf),
    #[error("glob pattern error: {0}")]
    Pattern(#[from] glob::PatternError),
    #[error("glob iteration error: {0}")]
    Glob(#[from] glob::GlobError),
    #[error("I/O error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid JSON in {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error(
        "{path}: missing or non-integer `toolr_schema_version` key — \
         this file is not a valid toolr manifest fragment"
    )]
    MissingVersion { path: PathBuf },
    #[error(
        "{path}: toolr_schema_version {version} is newer than this toolr \
         binary supports (max {max}). Upgrade toolr."
    )]
    UnknownVersion {
        path: PathBuf,
        version: u32,
        max: u32,
    },
    #[error(
        "duplicate command `{group}/{name}` declared by both `{first_package}` \
         and `{second_package}`"
    )]
    DuplicateCommand {
        group: String,
        name: String,
        first_package: String,
        second_package: String,
    },
}

/// Parse one fragment file, validating `toolr_schema_version` matches
/// `FRAGMENT_SCHEMA_VERSION`. Returns the ready-to-merge fragment.
///
/// There are no schema migrations: the only accepted version is the
/// current one. A future migration function is the day-v2-ships change.
pub fn parse_fragment(path: &Path) -> Result<ManifestFragment, ThirdPartyError> {
    let bytes = fs::read(path).map_err(|e| ThirdPartyError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    let raw: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| ThirdPartyError::Json {
            path: path.to_path_buf(),
            source: e,
        })?;

    let version = raw
        .as_object()
        .and_then(|m| m.get("toolr_schema_version"))
        .and_then(|v| v.as_u64())
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| *v >= 1)
        .ok_or_else(|| ThirdPartyError::MissingVersion {
            path: path.to_path_buf(),
        })?;

    if version > FRAGMENT_SCHEMA_VERSION {
        return Err(ThirdPartyError::UnknownVersion {
            path: path.to_path_buf(),
            version,
            max: FRAGMENT_SCHEMA_VERSION,
        });
    }

    // At this point `version == FRAGMENT_SCHEMA_VERSION`: the `>= 1` filter
    // above rejects 0/older as MissingVersion and the check just above
    // rejects anything newer. There are no migrations — when a v2 schema
    // ships, reintroduce a migration step here for the older versions.

    serde_json::from_value(raw).map_err(|e| ThirdPartyError::Json {
        path: path.to_path_buf(),
        source: e,
    })
}
