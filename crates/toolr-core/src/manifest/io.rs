//! Read and write the on-disk manifest file.

use std::fs;
use std::path::Path;

use thiserror::Error;

use super::model::{Manifest, SCHEMA_VERSION};

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unknown manifest schema_version {0}; this toolr supports up to {max}", max = SCHEMA_VERSION)]
    UnknownSchemaVersion(u32),
}

pub fn load_manifest(path: &Path) -> Result<Manifest, ManifestError> {
    let bytes = fs::read(path)?;
    let raw: serde_json::Value = serde_json::from_slice(&bytes)?;
    let version = raw
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    if version > SCHEMA_VERSION {
        return Err(ManifestError::UnknownSchemaVersion(version));
    }
    let manifest: Manifest = serde_json::from_value(raw)?;
    Ok(manifest)
}

/// Serialize the manifest to JSON and write it to `path`.
///
/// NOTE: The write is not atomic. A crash between truncate and close can
/// leave a zero-length or partial file on disk. The next dispatch reads
/// this file via `load_manifest`, which returns `Err` on a malformed
/// file; the freshness check then treats the cache as absent and runs a
/// full rebuild, so the failure mode is self-healing within one
/// invocation. If atomicity becomes required, switch to
/// `tempfile::NamedTempFile::persist`.
pub fn write_manifest(path: &Path, manifest: &Manifest) -> Result<(), ManifestError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(manifest)?;
    fs::write(path, bytes)?;
    Ok(())
}
