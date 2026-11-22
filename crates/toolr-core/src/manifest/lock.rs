//! Advisory lock so N processes that see a stale manifest rebuild it once.
//! Only writers lock; readers never do.

use std::fs::{File, OpenOptions};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use crate::uv::toolr_cache_dir;

/// Subdirectory of the toolr cache root that holds rebuild locks.
pub const LOCKS_DIR: &str = "locks";

/// Held while a manifest rebuild runs; dropping it releases the lock.
///
/// Holds `None` when the lock couldn't be taken (no cache dir, read-only
/// filesystem, ...); the caller then rebuilds unlocked, as it did before.
#[derive(Debug)]
pub struct ManifestRebuildLock {
    // Held so the advisory lock lives until drop.
    _file: Option<File>,
}

impl ManifestRebuildLock {
    /// Whether the advisory lock is actually held.
    #[cfg(test)]
    fn is_held(&self) -> bool {
        self._file.is_some()
    }
}

/// Lock file guarding rebuilds of `manifest_path`, kept in toolr's cache dir
/// so the user's `tools/` never gains a file to ignore. `None` when the
/// cache dir can't be resolved.
pub fn rebuild_lock_path(manifest_path: &Path) -> Option<PathBuf> {
    toolr_cache_dir().map(|root| lock_path_in(&root, manifest_path))
}

/// Lock file guarding rebuilds of `repo_root`'s manifest, under `cache_root`
/// (the toolr cache dir holding `locks/`). Shares one canonicalisation with rebuilds.
pub fn lock_path_for_repo(cache_root: &Path, repo_root: &Path) -> PathBuf {
    lock_path_in(cache_root, &super::manifest_path(repo_root))
}

/// `<cache_root>/locks/<blake3 of the canonical manifest path>.lock`. Prune may
/// delete one no kept entry needs; a process opening it just before the unlink
/// can then rebuild alongside another, the same cost as the unlocked fallback.
fn lock_path_in(cache_root: &Path, manifest_path: &Path) -> PathBuf {
    let canonical = canonical_manifest_path(manifest_path);
    let key = blake3::hash(canonical.as_os_str().as_encoded_bytes());
    cache_root
        .join(LOCKS_DIR)
        .join(format!("{}.lock", key.to_hex()))
}

/// Canonicalise via the parent, since the manifest itself may not exist yet.
fn canonical_manifest_path(manifest_path: &Path) -> PathBuf {
    let parent = manifest_path.parent().and_then(|p| p.canonicalize().ok());
    match (parent, manifest_path.file_name()) {
        (Some(parent), Some(name)) => parent.join(name),
        _ => manifest_path.to_path_buf(),
    }
}

/// Block until the rebuild lock for `manifest_path` is held, or return an
/// unheld guard if it can't be taken.
pub fn acquire_rebuild_lock(manifest_path: &Path) -> ManifestRebuildLock {
    lock_at(rebuild_lock_path(manifest_path).as_deref())
}

fn lock_at(lock_path: Option<&Path>) -> ManifestRebuildLock {
    let file = lock_path.and_then(|path| {
        std::fs::create_dir_all(path.parent()?).ok()?;
        // A fresh handle per call: flock/LockFileEx locks are per handle, so a
        // shared one would not exclude threads in the same process.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .ok()?;
        file.lock().ok().map(|()| file)
    });
    ManifestRebuildLock { _file: file }
}

/// Generic check, lock, re-check: rebuild only if `check` still reports the
/// manifest stale once the rebuild lock is held.
///
/// `check` returns `Continue(state)` when stale and `Break(fresh)` when
/// fresh. It runs once unlocked, so the common fresh path never touches the
/// lock, and once more under the lock, because another process may have
/// rebuilt while this one waited. `rebuild` gets the state from the locked
/// check. Returns the fresh value or the rebuild's.
pub fn rebuild_if_stale<S, T, E>(
    manifest_path: &Path,
    check: impl FnMut() -> Result<ControlFlow<T, S>, E>,
    rebuild: impl FnOnce(S) -> Result<T, E>,
) -> Result<T, E> {
    rebuild_if_stale_with(|| acquire_rebuild_lock(manifest_path), check, rebuild)
}

fn rebuild_if_stale_with<S, T, E>(
    lock: impl FnOnce() -> ManifestRebuildLock,
    mut check: impl FnMut() -> Result<ControlFlow<T, S>, E>,
    rebuild: impl FnOnce(S) -> Result<T, E>,
) -> Result<T, E> {
    if let ControlFlow::Break(fresh) = check()? {
        return Ok(fresh);
    }
    let _lock = lock();
    match check()? {
        ControlFlow::Break(fresh) => Ok(fresh),
        ControlFlow::Continue(state) => rebuild(state),
    }
}

#[cfg(test)]
mod tests;
