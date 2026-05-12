//! Parse and validate a single third-party manifest fragment file.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::migrate::migrate_to_current;
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
    #[error("{path}: migration from v{version} failed: {reason}")]
    Migration {
        path: PathBuf,
        version: u32,
        reason: String,
    },
}

/// Parse one fragment file, validating `toolr_schema_version` and
/// migrating older fragments to the current shape. Returns the
/// migrated, ready-to-merge fragment.
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

    let migrated =
        migrate_to_current(raw, version).map_err(|reason| ThirdPartyError::Migration {
            path: path.to_path_buf(),
            version,
            reason,
        })?;

    serde_json::from_value(migrated).map_err(|e| ThirdPartyError::Json {
        path: path.to_path_buf(),
        source: e,
    })
}
