use super::locks::{LockSweep, sweep_one, sweep_rebuild_locks};
use super::meta::{Meta, MetaError, SCHEMA_VERSION};
use crate::manifest::{LOCKS_DIR, lock_path_for_repo};
use chrono::{TimeZone, Utc};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn sample_meta() -> Meta {
    Meta {
        schema_version: SCHEMA_VERSION,
        repo_path: PathBuf::from("/home/u/repo"),
        toolr_version: "1.0.0".into(),
        python_version: "3.13.1".into(),
        created_at: Utc.with_ymd_and_hms(2026, 5, 11, 12, 0, 0).unwrap(),
        last_used_at: Utc.with_ymd_and_hms(2026, 5, 11, 12, 34, 56).unwrap(),
        interpreter_path: None,
        interpreter_hash: None,
    }
}

#[test]
fn loads_v1_sidecar_without_interpreter_fields() {
    let tmp = TempDir::new().unwrap();
    let v1 = r#"{"schema_version":1,"repo_path":"/r","toolr_version":"0","python_version":"3.13","created_at":"2026-01-01T00:00:00Z","last_used_at":"2026-01-01T00:00:00Z"}"#;
    std::fs::write(tmp.path().join("meta.json"), v1).unwrap();
    let m = Meta::load(tmp.path()).unwrap();
    assert!(m.interpreter_path.is_none());
    assert!(m.interpreter_hash.is_none());
}

#[test]
fn meta_round_trips_through_json() {
    let m = sample_meta();
    let s = serde_json::to_string_pretty(&m).expect("serialize");
    let back: Meta = serde_json::from_str(&s).expect("deserialize");
    assert_eq!(m, back);
}

#[test]
fn meta_write_then_load_round_trips() {
    let tmp = TempDir::new().unwrap();
    let m = sample_meta();
    m.write(tmp.path()).expect("write");
    let loaded = Meta::load(tmp.path()).expect("load");
    assert_eq!(m, loaded);
}

/// Regression for #541: writers sharing one cache entry used to clobber a fixed
/// `.meta.json.tmp`, failing with ENOENT or leaving a torn `meta.json`.
#[test]
fn meta_write_survives_concurrent_writers() {
    const WRITERS: usize = 16;
    const ROUNDS: usize = 200;

    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(WRITERS));

    let handles: Vec<_> = (0..WRITERS)
        .map(|i| {
            let cache_dir = cache_dir.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut m = sample_meta();
                m.toolr_version = format!("1.0.{i}");
                barrier.wait();
                (0..ROUNDS)
                    .filter_map(|_| m.write(&cache_dir).err().map(|e| e.to_string()))
                    .collect::<Vec<_>>()
            })
        })
        .collect();

    let errors: Vec<String> = handles
        .into_iter()
        .flat_map(|h| h.join().expect("writer thread panicked"))
        .collect();
    assert!(
        errors.is_empty(),
        "{} concurrent writes failed, e.g. {:?}",
        errors.len(),
        errors.first()
    );

    let loaded = Meta::load(&cache_dir).expect("meta.json must parse after concurrent writes");
    assert_eq!(loaded.repo_path, sample_meta().repo_path);

    let leftovers: Vec<_> = std::fs::read_dir(&cache_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .filter(|n| n != "meta.json")
        .collect();
    assert!(
        leftovers.is_empty(),
        "stray temp files left behind: {leftovers:?}"
    );
}

#[test]
fn meta_load_rejects_unknown_schema_version() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("meta.json"),
        r#"{
          "schema_version": 999,
          "repo_path": "/x",
          "toolr_version": "1.0.0",
          "python_version": "3.13.1",
          "created_at": "2026-05-11T12:00:00Z",
          "last_used_at": "2026-05-11T12:00:00Z"
        }"#,
    )
    .unwrap();
    let err = Meta::load(tmp.path()).expect_err("should reject");
    assert!(matches!(err, MetaError::UnknownSchemaVersion(999)));
}

#[test]
fn meta_load_missing_returns_io_error() {
    let tmp = TempDir::new().unwrap();
    let err = Meta::load(tmp.path()).expect_err("should be missing");
    assert!(matches!(err, MetaError::Io(_)));
}

#[test]
fn meta_new_sets_created_and_last_used_equal() {
    let m = Meta::new("/x", "1.0.0", "3.13.1");
    assert_eq!(m.created_at, m.last_used_at);
    assert_eq!(m.schema_version, SCHEMA_VERSION);
}

use super::init::write_meta_for_new_venv;

#[test]
fn write_meta_for_new_venv_creates_sidecar() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().join("repo-key");
    std::fs::create_dir_all(cache_dir.join("venv")).unwrap();

    let meta = write_meta_for_new_venv(&cache_dir, "/abs/repo".as_ref(), "1.2.3", "3.13.1")
        .expect("write meta");

    let loaded = Meta::load(&cache_dir).expect("load meta");
    assert_eq!(meta, loaded);
    assert_eq!(loaded.repo_path, PathBuf::from("/abs/repo"));
    assert_eq!(loaded.toolr_version, "1.2.3");
    assert_eq!(loaded.python_version, "3.13.1");
    assert_eq!(loaded.created_at, loaded.last_used_at);
}

#[test]
fn write_meta_for_new_venv_overwrites_existing() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().join("repo-key");
    std::fs::create_dir_all(&cache_dir).unwrap();

    let first =
        write_meta_for_new_venv(&cache_dir, "/abs/repo".as_ref(), "1.0.0", "3.12.0").unwrap();
    let second =
        write_meta_for_new_venv(&cache_dir, "/abs/repo".as_ref(), "1.0.0", "3.13.0").unwrap();
    assert_ne!(first.python_version, second.python_version);
    let loaded = Meta::load(&cache_dir).expect("load");
    assert_eq!(loaded.python_version, "3.13.0");
}

use super::touch::{touch_last_used, touch_or_backfill};

#[test]
fn touch_last_used_updates_only_last_used_at() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().join("repo-key");
    std::fs::create_dir_all(&cache_dir).unwrap();

    let original =
        write_meta_for_new_venv(&cache_dir, "/abs/repo".as_ref(), "1.0.0", "3.13.1").unwrap();

    std::thread::sleep(std::time::Duration::from_millis(20));

    touch_last_used(&cache_dir).expect("touch");
    let after = Meta::load(&cache_dir).expect("load");

    assert_eq!(after.created_at, original.created_at);
    assert_eq!(after.repo_path, original.repo_path);
    assert_eq!(after.toolr_version, original.toolr_version);
    assert_eq!(after.python_version, original.python_version);
    assert!(after.last_used_at > original.last_used_at);
}

#[test]
fn touch_last_used_is_a_noop_when_sidecar_is_missing() {
    let tmp = TempDir::new().unwrap();
    let result = touch_last_used(tmp.path());
    assert!(result.is_ok());
    assert!(
        !tmp.path().join("meta.json").exists(),
        "touch_last_used must not create a sidecar — use touch_or_backfill for that",
    );
}

#[test]
fn touch_or_backfill_updates_existing_sidecar() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().join("repo-key");
    std::fs::create_dir_all(&cache_dir).unwrap();

    let original =
        write_meta_for_new_venv(&cache_dir, "/abs/repo".as_ref(), "1.0.0", "3.13.1").unwrap();

    std::thread::sleep(std::time::Duration::from_millis(20));

    touch_or_backfill(&cache_dir, "/different/repo".as_ref(), "9.9.9", "3.99.0")
        .expect("touch_or_backfill");
    let after = Meta::load(&cache_dir).expect("load");

    // Existing sidecar must be respected; backfill context is ignored.
    assert_eq!(after.created_at, original.created_at);
    assert_eq!(after.repo_path, original.repo_path);
    assert_eq!(after.toolr_version, original.toolr_version);
    assert_eq!(after.python_version, original.python_version);
    assert!(after.last_used_at > original.last_used_at);
}

#[test]
fn touch_or_backfill_writes_sidecar_when_missing() {
    // Regression for the user-visible inconsistency where
    // `toolr project venv path` showed a venv directory but
    // `toolr self cache list` reported nothing — the entry existed
    // without a `meta.json` sidecar, so `enumerate_caches` skipped it.
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().join("repo-key");
    std::fs::create_dir_all(&cache_dir).unwrap();
    assert!(!cache_dir.join("meta.json").exists());

    touch_or_backfill(&cache_dir, "/abs/repo".as_ref(), "2.0.0", "3.13.1").expect("backfill");
    let meta = Meta::load(&cache_dir).expect("load");

    assert_eq!(meta.repo_path, std::path::PathBuf::from("/abs/repo"));
    assert_eq!(meta.toolr_version, "2.0.0");
    assert_eq!(meta.python_version, "3.13.1");
    assert_eq!(meta.last_used_at, meta.created_at);
}

use super::enumerate::{CachedVenv, enumerate_caches, enumerate_caches_unsized};

fn make_entry(
    root: &std::path::Path,
    key: &str,
    repo_path: &str,
    last_used: chrono::DateTime<Utc>,
    venv_byte_count: usize,
) {
    let cache_dir = root.join(key);
    std::fs::create_dir_all(cache_dir.join("venv")).unwrap();
    std::fs::write(cache_dir.join("venv/blob.bin"), vec![0u8; venv_byte_count]).unwrap();
    let m = Meta {
        schema_version: SCHEMA_VERSION,
        repo_path: PathBuf::from(repo_path),
        toolr_version: "1.0.0".into(),
        python_version: "3.13.1".into(),
        created_at: last_used,
        last_used_at: last_used,
        interpreter_path: None,
        interpreter_hash: None,
    };
    m.write(&cache_dir).unwrap();
}

#[test]
fn enumerate_caches_returns_empty_when_root_missing() {
    let tmp = TempDir::new().unwrap();
    let caches = enumerate_caches(&tmp.path().join("no-such-dir")).expect("ok");
    assert!(caches.is_empty());
}

#[test]
fn enumerate_caches_finds_all_meta_sidecars() {
    let tmp = TempDir::new().unwrap();
    let when = Utc.with_ymd_and_hms(2026, 5, 11, 12, 0, 0).unwrap();
    make_entry(tmp.path(), "key-a", "/repo/a", when, 1024);
    make_entry(tmp.path(), "key-b", "/repo/b", when, 2048);

    let mut caches = enumerate_caches(tmp.path()).expect("ok");
    caches.sort_by(|a, b| a.repo_key.cmp(&b.repo_key));
    assert_eq!(caches.len(), 2);
    assert_eq!(caches[0].repo_key, "key-a");
    assert_eq!(caches[1].repo_key, "key-b");
    assert!(caches[0].size_bytes >= 1024);
    assert!(caches[1].size_bytes >= 2048);
    assert!(!caches[0].is_orphan);
    assert!(!caches[1].is_orphan);
    let _: CachedVenv = caches.into_iter().next().unwrap();
}

#[test]
fn enumerate_caches_skips_directories_without_meta() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("partial/venv")).unwrap();
    let caches = enumerate_caches(tmp.path()).expect("ok");
    assert!(caches.is_empty());
}

#[test]
fn enumerate_caches_ignores_the_manifest_rebuild_locks_dir() {
    let tmp = TempDir::new().unwrap();
    let when = Utc.with_ymd_and_hms(2026, 5, 11, 12, 0, 0).unwrap();
    make_entry(tmp.path(), "key-a", "/repo/a", when, 1024);
    let locks = tmp.path().join(crate::manifest::LOCKS_DIR);
    std::fs::create_dir_all(&locks).unwrap();
    std::fs::write(locks.join("0123abcd.lock"), b"").unwrap();

    let caches = enumerate_caches(tmp.path()).expect("ok");
    let keys: Vec<_> = caches.iter().map(|c| c.repo_key.as_str()).collect();
    assert_eq!(keys, ["key-a"]);
}

use super::classify::{PruneReason, classify_entries};
use chrono::Duration as ChronoDuration;

fn now_fixture() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 11, 12, 0, 0).unwrap()
}

fn entry_at(repo: &str, last_used: chrono::DateTime<Utc>) -> CachedVenv {
    CachedVenv {
        repo_key: "k".into(),
        cache_dir: PathBuf::from(format!("/cache/{repo}")),
        meta: Meta {
            schema_version: SCHEMA_VERSION,
            repo_path: PathBuf::from(repo),
            toolr_version: "1.0.0".into(),
            python_version: "3.13.1".into(),
            created_at: last_used,
            last_used_at: last_used,
            interpreter_path: None,
            interpreter_hash: None,
        },
        size_bytes: 1024,
        is_orphan: false,
    }
}

#[test]
fn classify_marks_missing_repo_path_as_orphan() {
    let tmp = TempDir::new().unwrap();
    let missing = tmp.path().join("does-not-exist");
    let mut entry = entry_at("/x", now_fixture());
    entry.meta.repo_path = missing;
    let result = classify_entries(vec![entry], now_fixture(), 30);
    assert_eq!(result.orphan.len(), 1);
    assert_eq!(result.orphan[0].reason, PruneReason::Orphan);
    assert!(result.stale.is_empty());
    assert!(result.keep.is_empty());
}

#[test]
fn classify_marks_old_last_used_as_stale() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("real-repo");
    std::fs::create_dir_all(&repo).unwrap();
    let mut entry = entry_at("ignored", now_fixture() - ChronoDuration::days(45));
    entry.meta.repo_path = repo;
    let result = classify_entries(vec![entry], now_fixture(), 30);
    assert_eq!(result.stale.len(), 1);
    assert_eq!(result.stale[0].reason, PruneReason::Stale);
    assert!(result.orphan.is_empty());
}

#[test]
fn classify_keeps_recently_used_existing_repos() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("real-repo");
    std::fs::create_dir_all(&repo).unwrap();
    let mut entry = entry_at("ignored", now_fixture() - ChronoDuration::days(3));
    entry.meta.repo_path = repo;
    let result = classify_entries(vec![entry], now_fixture(), 30);
    assert_eq!(result.keep.len(), 1);
    assert!(result.orphan.is_empty());
    assert!(result.stale.is_empty());
}

#[test]
fn classify_prefers_orphan_over_stale_when_both_apply() {
    let mut entry = entry_at("/no/such/repo", now_fixture() - ChronoDuration::days(90));
    entry.meta.repo_path = PathBuf::from("/no/such/repo");
    let result = classify_entries(vec![entry], now_fixture(), 30);
    assert_eq!(result.orphan.len(), 1);
    assert!(result.stale.is_empty());
}

use super::hint::{HintConfig, compute_hint};

#[test]
fn hint_is_none_when_cache_is_small_and_clean() {
    let tmp = TempDir::new().unwrap();
    let live_repo = tmp.path().join("live");
    std::fs::create_dir_all(&live_repo).unwrap();
    let cache_root = tmp.path().join("toolr-cache");
    std::fs::create_dir_all(&cache_root).unwrap();
    make_entry(
        &cache_root,
        "a",
        live_repo.to_str().unwrap(),
        now_fixture(),
        1024,
    );

    let hint = compute_hint(&cache_root, &HintConfig::default(), now_fixture()).unwrap();
    assert!(hint.is_none(), "expected no hint, got {hint:?}");
}

#[test]
fn hint_fires_when_orphan_count_exceeds_threshold() {
    let tmp = TempDir::new().unwrap();
    let cache_root = tmp.path().join("toolr-cache");
    std::fs::create_dir_all(&cache_root).unwrap();
    for key in &["a", "b", "c"] {
        make_entry(&cache_root, key, "/missing", now_fixture(), 32);
    }

    let cfg = HintConfig {
        orphan_threshold: 2,
    };
    let hint = compute_hint(&cache_root, &cfg, now_fixture()).unwrap();
    assert!(hint.is_some());
    assert!(hint.as_ref().unwrap().contains("orphan"));
}

#[test]
fn hint_ignores_a_large_cache_when_orphans_are_few() {
    let tmp = TempDir::new().unwrap();
    let live_repo = tmp.path().join("live");
    std::fs::create_dir_all(&live_repo).unwrap();
    let cache_root = tmp.path().join("toolr-cache");
    std::fs::create_dir_all(&cache_root).unwrap();
    make_entry(
        &cache_root,
        "live",
        live_repo.to_str().unwrap(),
        now_fixture(),
        200 * 1024 * 1024,
    );
    make_entry(&cache_root, "a", "/missing", now_fixture(), 32);

    let cfg = HintConfig {
        orphan_threshold: 2,
    };
    assert!(
        compute_hint(&cache_root, &cfg, now_fixture())
            .unwrap()
            .is_none()
    );
}

#[test]
fn enumerate_caches_unsized_skips_the_size_walk() {
    let tmp = TempDir::new().unwrap();
    make_entry(tmp.path(), "a", "/missing", now_fixture(), 4096);
    let caches = enumerate_caches_unsized(tmp.path()).unwrap();
    assert_eq!(caches.len(), 1);
    assert_eq!(caches[0].size_bytes, 0);
    assert!(enumerate_caches(tmp.path()).unwrap()[0].size_bytes >= 4096);
}

#[test]
fn hint_is_none_when_cache_root_is_missing() {
    let tmp = TempDir::new().unwrap();
    let hint = compute_hint(
        &tmp.path().join("no-such-dir"),
        &HintConfig::default(),
        now_fixture(),
    )
    .unwrap();
    assert!(hint.is_none());
}

/// A repo with a `tools/` dir, so its lock path canonicalises.
fn make_repo(parent: &Path, name: &str) -> PathBuf {
    let repo = parent.join(name);
    std::fs::create_dir_all(repo.join("tools")).unwrap();
    repo
}

fn write_lock(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"").unwrap();
}

fn stray_lock(cache: &Path, name: &str) -> PathBuf {
    let path = cache.join(LOCKS_DIR).join(format!("{name}.lock"));
    write_lock(&path);
    path
}

#[test]
fn sweep_keeps_the_lock_of_a_kept_repo() {
    let cache = TempDir::new().unwrap();
    let repos = TempDir::new().unwrap();
    let repo = make_repo(repos.path(), "repo");
    let lock = lock_path_for_repo(cache.path(), &repo);
    write_lock(&lock);

    let sweep = sweep_rebuild_locks(cache.path(), std::slice::from_ref(&repo), false).unwrap();
    assert!(lock.is_file());
    assert_eq!(
        sweep,
        LockSweep {
            removed: 0,
            held: 0
        }
    );
}

#[test]
fn sweep_removes_locks_no_kept_repo_needs() {
    let cache = TempDir::new().unwrap();
    let a = stray_lock(cache.path(), "aaaa");
    let b = stray_lock(cache.path(), "bbbb");

    let sweep = sweep_rebuild_locks(cache.path(), &[], false).unwrap();
    assert!(!a.exists());
    assert!(!b.exists());
    assert_eq!(
        sweep,
        LockSweep {
            removed: 2,
            held: 0
        }
    );
    assert!(cache.path().join(LOCKS_DIR).is_dir(), "locks/ itself stays");
}

#[test]
fn sweep_skips_a_lock_another_handle_holds() {
    let cache = TempDir::new().unwrap();
    let lock = stray_lock(cache.path(), "held");
    let holder = std::fs::OpenOptions::new().write(true).open(&lock).unwrap();
    holder.lock().unwrap();

    let sweep = sweep_rebuild_locks(cache.path(), &[], false).unwrap();
    assert!(lock.is_file());
    assert_eq!(
        sweep,
        LockSweep {
            removed: 0,
            held: 1
        }
    );
}

#[test]
fn sweep_dry_run_removes_nothing() {
    let cache = TempDir::new().unwrap();
    let lock = stray_lock(cache.path(), "stray");

    let sweep = sweep_rebuild_locks(cache.path(), &[], true).unwrap();
    assert!(lock.is_file());
    assert_eq!(
        sweep,
        LockSweep {
            removed: 1,
            held: 0
        }
    );
}

#[test]
fn sweep_without_a_locks_dir_is_a_no_op() {
    let cache = TempDir::new().unwrap();
    let sweep = sweep_rebuild_locks(cache.path(), &[], false).unwrap();
    assert_eq!(sweep, LockSweep::default());
}

#[test]
fn sweep_propagates_an_unreadable_locks_dir() {
    let cache = TempDir::new().unwrap();
    // `locks` as a regular file: read_dir fails with something other than NotFound.
    std::fs::write(cache.path().join(LOCKS_DIR), b"").unwrap();
    assert!(sweep_rebuild_locks(cache.path(), &[], false).is_err());
}

#[test]
fn sweep_ignores_non_lock_entries() {
    let cache = TempDir::new().unwrap();
    let locks = cache.path().join(LOCKS_DIR);
    std::fs::create_dir_all(locks.join("sub.lock")).unwrap();
    std::fs::write(locks.join("notes.txt"), b"").unwrap();

    let sweep = sweep_rebuild_locks(cache.path(), &[], false).unwrap();
    assert!(locks.join("notes.txt").is_file());
    assert!(locks.join("sub.lock").is_dir());
    assert_eq!(sweep, LockSweep::default());
}

#[cfg(unix)]
#[test]
fn sweep_keeps_the_lock_of_a_repo_reached_through_a_symlink() {
    let cache = TempDir::new().unwrap();
    let repos = TempDir::new().unwrap();
    let real = make_repo(repos.path(), "real");
    let link = repos.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let lock = lock_path_for_repo(cache.path(), &real);
    write_lock(&lock);

    let sweep = sweep_rebuild_locks(cache.path(), std::slice::from_ref(&link), false).unwrap();
    assert!(lock.is_file());
    assert_eq!(sweep, LockSweep::default());
}

#[test]
fn sweep_one_counts_nothing_for_a_lock_already_gone() {
    let tmp = TempDir::new().unwrap();
    let mut sweep = LockSweep::default();
    sweep_one(&tmp.path().join("gone.lock"), false, &mut sweep).unwrap();
    assert_eq!(sweep, LockSweep::default());
}

#[cfg(unix)]
#[test]
fn sweep_removes_a_read_only_lock_file() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let locks = tmp.path().join(LOCKS_DIR);
    std::fs::create_dir_all(&locks).unwrap();
    let lock = locks.join("0123abcd.lock");
    std::fs::write(&lock, b"").unwrap();
    std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o444)).unwrap();

    let sweep = sweep_rebuild_locks(tmp.path(), &[], false).unwrap();

    assert_eq!(sweep.removed, 1);
    assert!(!lock.exists());
}
