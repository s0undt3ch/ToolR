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
                let check = || -> Result<ControlFlow<(), ()>, ()> {
                    if std::mem::take(&mut first_check) {
                        barrier.wait();
                        return Ok(ControlFlow::Continue(()));
                    }
                    Ok(if fresh.load(Ordering::SeqCst) {
                        ControlFlow::Break(())
                    } else {
                        ControlFlow::Continue(())
                    })
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
        || Ok::<_, ()>(ControlFlow::Continue(())),
        |()| {
            rebuilds += 1;
            Ok("rebuilt")
        },
    )
    .unwrap();
    assert_eq!(out, "rebuilt");
    assert_eq!(rebuilds, 1);
}

#[test]
fn fresh_manifest_is_never_locked_or_rebuilt() {
    // One-line closures: they must never run, and a multi-line body would
    // show up as uncovered.
    let lock = || -> ManifestRebuildLock { unreachable!("must not lock") };
    let rebuild = |()| -> Result<&str, ()> { unreachable!("must not rebuild") };
    let out = rebuild_if_stale_with(lock, || Ok(ControlFlow::Break("fresh")), rebuild).unwrap();
    assert_eq!(out, "fresh");
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
