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
    #[error("unknown manifest schema_version {0}; this toolr supports up to {}", SCHEMA_VERSION)]
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

pub fn write_manifest(path: &Path, manifest: &Manifest) -> Result<(), ManifestError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(manifest)?;
    fs::write(path, bytes)?;
    Ok(())
}
