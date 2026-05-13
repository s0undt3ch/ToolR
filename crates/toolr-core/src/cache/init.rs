//! Hook called by the venv-creation path (Plan 3) to drop a `meta.json`
//! sidecar next to the freshly-built venv.

use std::path::Path;

use super::meta::{Meta, MetaError};

/// Write a fresh `meta.json` into `cache_dir`. Replaces any existing
/// sidecar — venv recreation is the only reason this entry point is
/// hit twice for the same `cache_dir`, and the new venv invalidates the
/// old metadata.
pub fn write_meta_for_new_venv(
    cache_dir: &Path,
    repo_path: &Path,
    toolr_version: &str,
    python_version: &str,
) -> Result<Meta, MetaError> {
    let meta = Meta::new(repo_path.to_path_buf(), toolr_version, python_version);
    meta.write(cache_dir)?;
    Ok(meta)
}
