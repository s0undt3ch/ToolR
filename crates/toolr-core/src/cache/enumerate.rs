//! Walk the cache root and collect one record per entry.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use walkdir::WalkDir;

use super::meta::{FILE_NAME, Meta};

/// One cache entry plus derived facts.
#[derive(Debug, Clone)]
pub struct CachedVenv {
    /// Subdirectory name under the cache root (the `<repo-key>`).
    pub repo_key: String,
    /// Absolute path to the per-entry cache directory.
    pub cache_dir: PathBuf,
    /// Parsed sidecar.
    pub meta: Meta,
    /// Disk usage of the entire `cache_dir` subtree (`venv/` plus
    /// `meta.json` plus anything else inside).
    pub size_bytes: u64,
    /// True iff `meta.repo_path` does not exist as a directory anymore.
    /// Populated in classification; enumeration defaults to `false`.
    pub is_orphan: bool,
}

/// Sum every regular-file size under `dir`. Returns 0 if `dir` does not
/// exist. Symlinks are not followed.
pub fn dir_size_bytes(dir: &Path) -> Result<u64> {
    if !dir.exists() {
        return Ok(0);
    }
    let mut total: u64 = 0;
    for entry in WalkDir::new(dir).follow_links(false) {
        let entry = entry.with_context(|| format!("walking {}", dir.display()))?;
        if entry.file_type().is_file() {
            match entry.metadata() {
                Ok(md) => total = total.saturating_add(md.len()),
                Err(_) => continue,
            }
        }
    }
    Ok(total)
}

/// Enumerate every cache entry directly under `cache_root`. Missing
/// `cache_root` returns an empty vector. Entries without a `meta.json`
/// are silently skipped — they predate the sidecar format or were
/// partially created.
pub fn enumerate_caches(cache_root: &Path) -> Result<Vec<CachedVenv>> {
    let mut out = Vec::new();
    let read = match std::fs::read_dir(cache_root) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => {
            return Err(e)
                .with_context(|| format!("reading cache root {}", cache_root.display()));
        }
    };

    for entry in read {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let cache_dir = entry.path();
        let sidecar = cache_dir.join(FILE_NAME);
        if !sidecar.is_file() {
            continue;
        }
        let meta = match Meta::load(&cache_dir) {
            Ok(m) => m,
            Err(_) => continue, // malformed sidecars are ignored
        };
        let size_bytes = dir_size_bytes(&cache_dir)?;
        let repo_key = entry.file_name().to_string_lossy().into_owned();
        out.push(CachedVenv {
            repo_key,
            cache_dir,
            meta,
            size_bytes,
            is_orphan: false,
        });
    }
    Ok(out)
}
