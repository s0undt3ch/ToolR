//! Advisory lock so N processes that see a stale manifest rebuild it once.
//! Only writers lock; readers never do.

use std::fs::{File, OpenOptions};
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
    file: Option<File>,
}

impl ManifestRebuildLock {
    /// Whether the advisory lock is actually held.
    pub fn is_held(&self) -> bool {
        self.file.is_some()
    }
}

/// Lock file guarding rebuilds of `manifest_path`, kept in toolr's cache dir
/// so the user's `tools/` never gains a file to ignore. `None` when the
/// cache dir can't be resolved.
pub fn rebuild_lock_path(manifest_path: &Path) -> Option<PathBuf> {
    toolr_cache_dir().map(|root| lock_path_in(&root, manifest_path))
}

/// `<cache_root>/locks/<blake3 of the canonical manifest path>.lock`. Never
/// deleted: removing a lock file races with processes about to open it.
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
    ManifestRebuildLock { file }
}

/// Rebuild only if `check` still reports the manifest stale.
///
/// `check` returns `Some(state)` when stale and `None` when fresh. It runs
/// once unlocked, so the common fresh path never touches the lock, and once
/// more under the lock, because another process may have rebuilt while this
/// one waited. `rebuild` gets the state from the locked check. Returns
/// `None` when no rebuild was needed.
pub fn rebuild_if_stale<S, T, E>(
    manifest_path: &Path,
    check: impl FnMut() -> Result<Option<S>, E>,
    rebuild: impl FnOnce(S) -> Result<T, E>,
) -> Result<Option<T>, E> {
    rebuild_if_stale_with(|| acquire_rebuild_lock(manifest_path), check, rebuild)
}

fn rebuild_if_stale_with<S, T, E>(
    lock: impl FnOnce() -> ManifestRebuildLock,
    mut check: impl FnMut() -> Result<Option<S>, E>,
    rebuild: impl FnOnce(S) -> Result<T, E>,
) -> Result<Option<T>, E> {
    if check()?.is_none() {
        return Ok(None);
    }
    let _lock = lock();
    let Some(state) = check()? else {
        return Ok(None);
    };
    rebuild(state).map(Some)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;

    use tempfile::TempDir;

    use super::*;

    const WORKERS: usize = 8;

    /// A lock path under a throwaway cache root, never the real user cache.
    fn temp_lock_path(cache: &TempDir) -> PathBuf {
        lock_path_in(cache.path(), Path::new("tools/.toolr-manifest.json"))
    }

    /// Every worker's unlocked check reports stale, as when N processes
    /// start against the same stale manifest. Later checks read `fresh`.
    fn spawn_workers(
        lock_path: &Path,
        fresh: &Arc<AtomicBool>,
        rebuilds: &Arc<AtomicUsize>,
        barrier: &Arc<Barrier>,
    ) -> Vec<thread::JoinHandle<()>> {
        (0..WORKERS)
            .map(|_| {
                let lock_path = lock_path.to_path_buf();
                let fresh = Arc::clone(fresh);
                let rebuilds = Arc::clone(rebuilds);
                let barrier = Arc::clone(barrier);
                thread::spawn(move || {
                    let mut first_check = true;
                    let check = || -> Result<Option<()>, ()> {
                        if std::mem::take(&mut first_check) {
                            barrier.wait();
                            return Ok(Some(()));
                        }
                        Ok((!fresh.load(Ordering::SeqCst)).then_some(()))
                    };
                    let lock = || lock_at(Some(&lock_path));
                    rebuild_if_stale_with(lock, check, |()| {
                        rebuilds.fetch_add(1, Ordering::SeqCst);
                        fresh.store(true, Ordering::SeqCst);
                        Ok::<_, ()>(())
                    })
                    .unwrap();
                })
            })
            .collect()
    }

    #[test]
    fn concurrent_stale_callers_rebuild_once() {
        let cache = TempDir::new().unwrap();
        let lock_path = temp_lock_path(&cache);
        let fresh = Arc::new(AtomicBool::new(false));
        let rebuilds = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(WORKERS));
        for h in spawn_workers(&lock_path, &fresh, &rebuilds, &barrier) {
            h.join().unwrap();
        }
        assert_eq!(rebuilds.load(Ordering::SeqCst), 1);
        assert!(lock_path.is_file(), "the locks/ dir is created on demand");
    }

    #[test]
    fn waiter_skips_rebuild_when_manifest_turns_fresh_under_the_lock() {
        let cache = TempDir::new().unwrap();
        let lock_path = temp_lock_path(&cache);
        // Stand in for another process that is mid-rebuild.
        let holder = lock_at(Some(&lock_path));
        assert!(holder.is_held());

        let fresh = Arc::new(AtomicBool::new(false));
        let rebuilds = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(WORKERS + 1));
        let handles = spawn_workers(&lock_path, &fresh, &rebuilds, &barrier);
        barrier.wait();
        // The other process finishes its rebuild, then releases the lock.
        fresh.store(true, Ordering::SeqCst);
        drop(holder);
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(rebuilds.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn rebuilds_unlocked_when_the_lock_cannot_be_taken() {
        let cache = TempDir::new().unwrap();
        // `locks/` can't be created under a regular file.
        let not_a_dir = cache.path().join("not-a-dir");
        std::fs::write(&not_a_dir, b"").unwrap();
        let lock_path = lock_path_in(&not_a_dir, Path::new("tools/.toolr-manifest.json"));
        assert!(!lock_at(Some(&lock_path)).is_held());
        assert!(!lock_at(None).is_held());

        let mut rebuilds = 0;
        let out = rebuild_if_stale_with(
            || lock_at(Some(&lock_path)),
            || Ok::<_, ()>(Some(())),
            |()| {
                rebuilds += 1;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(out, Some(()));
        assert_eq!(rebuilds, 1);
    }

    #[test]
    fn fresh_manifest_is_never_locked_or_rebuilt() {
        // One-line closures: they must never run, and a multi-line body would
        // show up as uncovered.
        let lock = || -> ManifestRebuildLock { unreachable!("must not lock") };
        let rebuild = |()| -> Result<(), ()> { unreachable!("must not rebuild") };
        let out = rebuild_if_stale_with(lock, || Ok(None), rebuild).unwrap();
        assert_eq!(out, None);
    }

    #[test]
    fn lock_path_lives_in_the_cache_keyed_by_canonical_manifest_path() {
        let cache = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        let tools = project.path().join("tools");
        std::fs::create_dir(&tools).unwrap();
        let manifest = tools.join(".toolr-manifest.json");
        let dotted = tools.join("..").join("tools").join(".toolr-manifest.json");

        let path = lock_path_in(cache.path(), &manifest);
        assert_eq!(path.parent().unwrap(), cache.path().join(LOCKS_DIR));
        assert_eq!(path, lock_path_in(cache.path(), &dotted));

        let other = TempDir::new().unwrap();
        std::fs::create_dir(other.path().join("tools")).unwrap();
        let elsewhere = other.path().join("tools").join(".toolr-manifest.json");
        assert_ne!(path, lock_path_in(cache.path(), &elsewhere));
    }
}
