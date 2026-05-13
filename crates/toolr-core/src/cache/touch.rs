//! Update `last_used_at` on every toolr invocation.

use std::path::Path;

use chrono::Utc;

use super::meta::{Meta, MetaError};

/// Re-write `meta.json` with a fresh `last_used_at`. Missing sidecars
/// are silently ignored — older cache entries that predate Plan 8 are
/// allowed to exist without metadata.
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
