//! Compute the passive "your cache is big, consider pruning" message.

use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Utc};
use humansize::{BINARY, format_size};

use super::classify::{Classification, classify_entries};
use super::enumerate::enumerate_caches;

/// Tunables for hint emission.
#[derive(Debug, Clone, Copy)]
pub struct HintConfig {
    /// Aggregate cache size threshold in bytes. Default 1 GiB.
    pub size_threshold_bytes: u64,
    /// Orphan-entry count threshold. Default 10.
    pub orphan_threshold: usize,
}

impl Default for HintConfig {
    fn default() -> Self {
        Self {
            size_threshold_bytes: 1024 * 1024 * 1024,
            orphan_threshold: 10,
        }
    }
}

/// Inspect the cache and return a single-line message if either
/// threshold is exceeded. Returns `Ok(None)` when nothing should be
/// printed.
pub fn compute_hint(
    cache_root: &Path,
    config: &HintConfig,
    now: DateTime<Utc>,
) -> Result<Option<String>> {
    let entries = enumerate_caches(cache_root)?;
    if entries.is_empty() {
        return Ok(None);
    }

    let total_bytes: u64 = entries.iter().map(|e| e.size_bytes).sum();
    let Classification {
        keep: _,
        orphan,
        stale,
    } = classify_entries(entries, now, 30);

    let prune_target_count = orphan.len() + stale.len();
    let oversized = total_bytes >= config.size_threshold_bytes;
    let too_many_orphans = orphan.len() > config.orphan_threshold;

    if !oversized && !too_many_orphans {
        return Ok(None);
    }

    let pretty_size = format_size(total_bytes, BINARY);
    let msg = if oversized && too_many_orphans {
        format!(
            "toolr: cache has {} orphan entries (~{}). Run `toolr self cache prune` to clean up.",
            orphan.len(),
            pretty_size,
        )
    } else if oversized {
        format!(
            "toolr: cache has {} entr{} (~{}). Run `toolr self cache prune` to clean up.",
            prune_target_count.max(1),
            if prune_target_count.max(1) == 1 {
                "y"
            } else {
                "ies"
            },
            pretty_size,
        )
    } else {
        format!(
            "toolr: cache has {} orphan entries (~{}). Run `toolr self cache prune` to clean up.",
            orphan.len(),
            pretty_size,
        )
    };
    Ok(Some(msg))
}
