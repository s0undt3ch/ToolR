//! Compute the passive "you have orphaned caches" message.

use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Utc};

use super::classify::classify_entries;
use super::enumerate::enumerate_caches_unsized;

/// Tunables for hint emission.
#[derive(Debug, Clone, Copy)]
pub struct HintConfig {
    /// Orphan-entry count threshold. Default 10.
    pub orphan_threshold: usize,
}

impl Default for HintConfig {
    fn default() -> Self {
        Self {
            orphan_threshold: 10,
        }
    }
}

/// Return a single-line message when more than `orphan_threshold` cache
/// entries point at repos that no longer exist, else `Ok(None)`. Runs on
/// ordinary commands, so it reads `meta.json` sidecars only and never walks
/// the venvs for sizes.
pub fn compute_hint(
    cache_root: &Path,
    config: &HintConfig,
    now: DateTime<Utc>,
) -> Result<Option<String>> {
    let entries = enumerate_caches_unsized(cache_root)?;
    if entries.is_empty() {
        return Ok(None);
    }
    let orphans = classify_entries(entries, now, 30).orphan.len();
    if orphans <= config.orphan_threshold {
        return Ok(None);
    }
    Ok(Some(format!(
        "toolr: cache has {orphans} orphan entries. Run `toolr self cache prune` to clean up."
    )))
}
