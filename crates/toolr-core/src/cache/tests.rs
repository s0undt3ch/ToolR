use super::meta::{Meta, MetaError, SCHEMA_VERSION};
use chrono::{TimeZone, Utc};
use std::path::PathBuf;
use tempfile::TempDir;

fn sample_meta() -> Meta {
    Meta {
        schema_version: SCHEMA_VERSION,
        repo_path: PathBuf::from("/home/u/repo"),
        toolr_version: "1.0.0".into(),
        python_version: "3.13.1".into(),
        created_at: Utc.with_ymd_and_hms(2026, 5, 11, 12, 0, 0).unwrap(),
        last_used_at: Utc.with_ymd_and_hms(2026, 5, 11, 12, 34, 56).unwrap(),
    }
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

    let meta = write_meta_for_new_venv(
        &cache_dir,
        "/abs/repo".as_ref(),
        "1.2.3",
        "3.13.1",
    )
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

use super::touch::touch_last_used;

#[test]
fn touch_last_used_updates_only_last_used_at() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().join("repo-key");
    std::fs::create_dir_all(&cache_dir).unwrap();

    let original = write_meta_for_new_venv(
        &cache_dir,
        "/abs/repo".as_ref(),
        "1.0.0",
        "3.13.1",
    )
    .unwrap();

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
}

use super::enumerate::{CachedVenv, enumerate_caches};

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
    make_entry(&cache_root, "a", live_repo.to_str().unwrap(), now_fixture(), 1024);

    let hint = compute_hint(&cache_root, &HintConfig::default(), now_fixture()).unwrap();
    assert!(hint.is_none(), "expected no hint, got {hint:?}");
}

#[test]
fn hint_fires_when_total_size_exceeds_threshold() {
    let tmp = TempDir::new().unwrap();
    let live_repo = tmp.path().join("live");
    std::fs::create_dir_all(&live_repo).unwrap();
    let cache_root = tmp.path().join("toolr-cache");
    std::fs::create_dir_all(&cache_root).unwrap();
    make_entry(&cache_root, "a", live_repo.to_str().unwrap(), now_fixture(), 4096);

    let cfg = HintConfig {
        size_threshold_bytes: 1024,
        orphan_threshold: 10,
    };
    let hint = compute_hint(&cache_root, &cfg, now_fixture()).unwrap();
    assert!(hint.is_some());
    let s = hint.unwrap();
    assert!(s.contains("Run `toolr self cache prune`"), "got: {s}");
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
        size_threshold_bytes: 100 * 1024 * 1024 * 1024,
        orphan_threshold: 2,
    };
    let hint = compute_hint(&cache_root, &cfg, now_fixture()).unwrap();
    assert!(hint.is_some());
    assert!(hint.as_ref().unwrap().contains("orphan"));
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
