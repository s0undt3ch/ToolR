//! Update `last_used_at` on every toolr invocation.

use std::path::Path;

use chrono::Utc;

use super::meta::{Meta, MetaError};

/// Re-write `meta.json` with a fresh `last_used_at`. Missing sidecars
/// are silently ignored — this entry point intentionally has no context
/// to backfill from. Call sites with access to the owning repo path
/// should prefer [`touch_or_backfill`], which self-heals pre-Plan-8
/// cache entries so they become visible to `toolr self cache list`.
pub fn touch_last_used(cache_dir: &Path) -> Result<(), MetaError> {
    let mut meta = match Meta::load(cache_dir) {
        Ok(m) => m,
        Err(MetaError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    meta.last_used_at = Utc::now();
    meta.write(cache_dir)?;
    Ok(())
}

/// Update `last_used_at` if a sidecar exists; otherwise synthesise a
/// fresh `meta.json` from the supplied context.
///
/// This is the entry point dispatch uses on every invocation against a
/// cached venv. The backfill branch heals the inventory for cache
/// entries created by an older toolr binary (or any path that produced
/// the venv without writing the sidecar) so subsequent `toolr self
/// cache list / prune` calls see them.
///
/// `created_at` on backfilled entries reflects the time of backfill,
/// not the time the venv directory was actually materialised — the
/// real birth time is not recoverable. Callers that care about
/// historical accuracy should keep this in mind; everything in
/// `cache list / prune` works fine with the approximation.
pub fn touch_or_backfill(
    cache_dir: &Path,
    repo_path: &Path,
    toolr_version: &str,
    python_version: &str,
) -> Result<(), MetaError> {
    match Meta::load(cache_dir) {
        Ok(mut meta) => {
            meta.last_used_at = Utc::now();
            meta.write(cache_dir)?;
            Ok(())
        }
        Err(MetaError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            let meta = Meta::new(repo_path.to_path_buf(), toolr_version, python_version);
            meta.write(cache_dir)?;
            Ok(())
        }
        Err(e) => Err(e),
    }
}
