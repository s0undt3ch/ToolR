//! Compute the passive "you have orphaned caches" message.

use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Utc};

use super::classify::classify_entries;
use super::enumerate::enumerate_caches_unsized;

/// Orphan count above which the hint fires; a handful is normal churn.
pub const HINT_ORPHAN_THRESHOLD: usize = 10;

/// Return a single-line message when more than `orphan_threshold` cache
/// entries point at repos that no longer exist, else `Ok(None)`. Runs on
/// ordinary commands, so it reads `meta.json` sidecars only and never walks
/// the venvs for sizes.
pub fn compute_hint(
    cache_root: &Path,
    orphan_threshold: usize,
    now: DateTime<Utc>,
) -> Result<Option<String>> {
    let entries = enumerate_caches_unsized(cache_root)?;
    if entries.is_empty() {
        return Ok(None);
    }
    // The stale-days argument is unused: only `.orphan` is read.
    let unused_stale_days = 30;
    let orphans = classify_entries(entries, now, unused_stale_days)
        .orphan
        .len();
    if orphans <= orphan_threshold {
        return Ok(None);
    }
    Ok(Some(format!(
        "toolr: cache has {orphans} orphan entries. Run `toolr self cache prune` to clean up."
    )))
}
