//! Read and write the on-disk manifest file.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

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
    let bytes = read_retrying(|| fs::read(path), is_transient_read_error)?;
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

/// Windows refuses to open a file while a concurrent `write_manifest` is
/// renaming over it; that window is brief, so the read is retried.
fn is_transient_read_error(e: &std::io::Error) -> bool {
    cfg!(windows) && e.kind() == std::io::ErrorKind::PermissionDenied
}

const READ_ATTEMPTS: u32 = 20;

fn read_retrying(
    mut read: impl FnMut() -> std::io::Result<Vec<u8>>,
    transient: impl Fn(&std::io::Error) -> bool,
) -> std::io::Result<Vec<u8>> {
    for _ in 1..READ_ATTEMPTS {
        match read() {
            Err(e) if transient(&e) => std::thread::sleep(Duration::from_millis(5)),
            result => return result,
        }
    }
    read()
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

#[cfg(test)]
mod read_retry_tests {
    use std::io::{Error, ErrorKind};

    use super::{READ_ATTEMPTS, read_retrying};

    fn denied() -> Error {
        Error::from(ErrorKind::PermissionDenied)
    }

    #[test]
    fn transient_errors_are_retried_until_the_read_succeeds() {
        let mut calls = 0;
        let read = || {
            calls += 1;
            if calls < 4 {
                Err(denied())
            } else {
                Ok(b"ok".to_vec())
            }
        };
        assert_eq!(read_retrying(read, |_| true).unwrap(), b"ok");
        assert_eq!(calls, 4);
    }

    #[test]
    fn persistent_transient_errors_give_up_after_the_last_attempt() {
        let mut calls = 0;
        let read = || {
            calls += 1;
            Err(denied())
        };
        assert!(read_retrying(read, |_| true).is_err());
        assert_eq!(calls, READ_ATTEMPTS);
    }

    #[test]
    fn other_errors_are_not_retried() {
        let mut calls = 0;
        let read = || {
            calls += 1;
            Err(denied())
        };
        assert!(read_retrying(read, |_| false).is_err());
        assert_eq!(calls, 1);
    }
}
