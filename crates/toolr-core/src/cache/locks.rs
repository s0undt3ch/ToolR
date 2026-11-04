//! Sweep of manifest rebuild locks that no kept cache entry needs.

use std::collections::HashSet;
use std::fs::{OpenOptions, TryLockError};
use std::io::ErrorKind;
use std::path::Path;

use crate::manifest::{LOCKS_DIR, lock_path_for_repo};

/// What a sweep did, or would do on a dry run.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LockSweep {
    pub removed: usize,
    /// Skipped because a rebuild holds them right now.
    pub held: usize,
}

/// Delete every `locks/*.lock` under `cache_root` that none of `keep_repos`
/// would lock. Held locks stay; `locks/` itself is never removed.
pub fn sweep_rebuild_locks(
    cache_root: &Path,
    keep_repos: &[&Path],
    dry_run: bool,
) -> std::io::Result<LockSweep> {
    let keep: HashSet<_> = keep_repos
        .iter()
        .map(|repo| lock_path_for_repo(cache_root, repo))
        .collect();
    let mut sweep = LockSweep::default();
    let entries = match std::fs::read_dir(cache_root.join(LOCKS_DIR)) {
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(sweep),
        other => other?,
    };
    for entry in entries {
        let path = entry?.path();
        if !path.is_file()
            || path.extension().is_none_or(|ext| ext != "lock")
            || keep.contains(&path)
        {
            continue;
        }
        let file = OpenOptions::new().read(true).write(true).open(&path)?;
        match file.try_lock() {
            Err(TryLockError::WouldBlock) => sweep.held += 1,
            locked => {
                locked?;
                sweep.removed += 1;
                // Unlink while still holding the lock, so no rebuild starts on it meanwhile.
                if !dry_run {
                    std::fs::remove_file(&path)?;
                }
            }
        }
    }
    Ok(sweep)
}
