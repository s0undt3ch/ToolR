//! Read and write the on-disk manifest file.

use std::fs;
use std::io::Write;
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
    #[error("malformed manifest: {0}")]
    InvalidArgument(String),
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
    manifest.validate_arguments().map_err(ManifestError::InvalidArgument)?;
    Ok(manifest)
}

/// Serialize the manifest to JSON and atomically replace `path` with it.
///
/// Concurrent toolr processes (e.g. parallel pre-commit hooks) rewrite
/// the same manifest, so the bytes go to a uniquely named temp file in
/// the same directory and are renamed over `path`. Readers see either
/// the old or the new manifest, never a truncated or partial one (#542).
pub fn write_manifest(path: &Path, manifest: &Manifest) -> Result<(), ManifestError> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let bytes = serde_json::to_vec_pretty(manifest)?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".toolr-manifest.")
        .suffix(".tmp")
        .tempfile_in(parent)?;
    tmp.write_all(&bytes)?;
    // `tempfile` creates files 0600; keep the existing manifest's mode, or
    // the conventional 0644, so other users of the checkout can still read it.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::metadata(path)
            .map(|m| m.permissions())
            .unwrap_or_else(|_| fs::Permissions::from_mode(0o644));
        tmp.as_file().set_permissions(perms)?;
    }
    // Not `persist`: its Windows `MoveFileExW` denies concurrent replaces;
    // std's rename uses POSIX semantics there.
    let tmp = tmp.into_temp_path();
    fs::rename(&tmp, path)?;
    Ok(())
}
