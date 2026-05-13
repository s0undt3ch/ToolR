//! Sort enumerated cache entries into keep / orphan / stale.

use chrono::{DateTime, Duration as ChronoDuration, Utc};

use super::enumerate::CachedVenv;

/// Why a single entry is up for pruning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruneReason {
    /// `meta.repo_path` is not a directory anymore.
    Orphan,
    /// `meta.last_used_at` is older than the staleness threshold.
    Stale,
}

/// One entry plus the reason it was selected.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub entry: CachedVenv,
    pub reason: PruneReason,
}

/// Bucketed classification result.
#[derive(Debug, Default)]
pub struct Classification {
    pub keep: Vec<CachedVenv>,
    pub orphan: Vec<Candidate>,
    pub stale: Vec<Candidate>,
}

/// Decide what to do with each entry. `stale_after_days` is the
/// configurable threshold (default 30). Orphan beats stale: an entry
/// whose repo no longer exists is always reported as orphan.
pub fn classify_entries(
    entries: Vec<CachedVenv>,
    now: DateTime<Utc>,
    stale_after_days: u32,
) -> Classification {
    let threshold = ChronoDuration::days(stale_after_days as i64);
    let mut result = Classification::default();
    for entry in entries {
        if !entry.meta.repo_path.is_dir() {
            let mut e = entry;
            e.is_orphan = true;
            result.orphan.push(Candidate {
                entry: e,
                reason: PruneReason::Orphan,
            });
            continue;
        }
        let age = now.signed_duration_since(entry.meta.last_used_at);
        if age >= threshold {
            result.stale.push(Candidate {
                entry,
                reason: PruneReason::Stale,
            });
        } else {
            result.keep.push(entry);
        }
    }
    result
}
