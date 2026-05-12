//! Integration tests for `toolr self cache prune` and `--all`.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use chrono::{Duration, Utc};
use tempfile::TempDir;

fn write_entry(
    cache_root: &Path,
    key: &str,
    repo_path: &Path,
    last_used_at: chrono::DateTime<Utc>,
) {
    let cache_dir = cache_root.join(key);
    fs::create_dir_all(cache_dir.join("venv")).unwrap();
    fs::write(cache_dir.join("venv/blob.bin"), vec![0u8; 256]).unwrap();
    let json = format!(
        r#"{{
          "schema_version": 1,
          "repo_path": "{}",
          "toolr_version": "1.0.0",
          "python_version": "3.13.1",
          "created_at": "{}",
          "last_used_at": "{}"
        }}"#,
        repo_path.display(),
        last_used_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        last_used_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    );
    fs::write(cache_dir.join("meta.json"), json).unwrap();
}

#[test]
fn prune_removes_orphan_entries() {
    let tmp = TempDir::new().unwrap();
    let cache_root = tmp.path().join("toolr");
    fs::create_dir_all(&cache_root).unwrap();

    write_entry(
        &cache_root,
        "orphan-key",
        Path::new("/definitely/missing/repo"),
        Utc::now(),
    );
    let live_repo = tmp.path().join("live-repo");
    fs::create_dir_all(&live_repo).unwrap();
    write_entry(&cache_root, "live-key", &live_repo, Utc::now());

    let output = Command::cargo_bin("toolr")
        .unwrap()
        .env("XDG_CACHE_HOME", tmp.path())
        .env_remove("HOME")
        .env("TOOLR_NO_CACHE_HINT", "1")
        .args(["self", "cache", "prune"])
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ORPHAN"), "stdout:\n{stdout}");

    assert!(!cache_root.join("orphan-key").exists());
    assert!(cache_root.join("live-key").exists());
}

#[test]
fn prune_removes_stale_entries() {
    let tmp = TempDir::new().unwrap();
    let cache_root = tmp.path().join("toolr");
    fs::create_dir_all(&cache_root).unwrap();

    let live_repo = tmp.path().join("live-repo");
    fs::create_dir_all(&live_repo).unwrap();
    write_entry(
        &cache_root,
        "stale-key",
        &live_repo,
        Utc::now() - Duration::days(60),
    );
    write_entry(
        &cache_root,
        "fresh-key",
        &live_repo,
        Utc::now() - Duration::days(2),
    );

    let output = Command::cargo_bin("toolr")
        .unwrap()
        .env("XDG_CACHE_HOME", tmp.path())
        .env_remove("HOME")
        .env("TOOLR_NO_CACHE_HINT", "1")
        .args(["self", "cache", "prune"])
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("STALE"), "stdout:\n{stdout}");

    assert!(!cache_root.join("stale-key").exists());
    assert!(cache_root.join("fresh-key").exists());
}

#[test]
fn prune_dry_run_leaves_disk_untouched() {
    let tmp = TempDir::new().unwrap();
    let cache_root = tmp.path().join("toolr");
    fs::create_dir_all(&cache_root).unwrap();
    write_entry(
        &cache_root,
        "orphan-key",
        Path::new("/definitely/missing/repo"),
        Utc::now(),
    );

    let output = Command::cargo_bin("toolr")
        .unwrap()
        .env("XDG_CACHE_HOME", tmp.path())
        .env_remove("HOME")
        .env("TOOLR_NO_CACHE_HINT", "1")
        .args(["self", "cache", "prune", "--dry-run"])
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("DRY-RUN"), "stdout:\n{stdout}");

    assert!(cache_root.join("orphan-key").exists());
}

#[test]
fn prune_all_with_yes_nukes_everything() {
    let tmp = TempDir::new().unwrap();
    let cache_root = tmp.path().join("toolr");
    fs::create_dir_all(&cache_root).unwrap();
    let live_repo = tmp.path().join("live-repo");
    fs::create_dir_all(&live_repo).unwrap();

    write_entry(&cache_root, "a", &live_repo, Utc::now());
    write_entry(&cache_root, "b", &live_repo, Utc::now());

    let output = Command::cargo_bin("toolr")
        .unwrap()
        .env("XDG_CACHE_HOME", tmp.path())
        .env_remove("HOME")
        .env("TOOLR_NO_CACHE_HINT", "1")
        .args(["self", "cache", "prune", "--all", "--yes"])
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));

    assert!(!cache_root.join("a").exists());
    assert!(!cache_root.join("b").exists());
}

#[test]
fn prune_all_refuses_without_yes_when_non_interactive() {
    let tmp = TempDir::new().unwrap();
    let cache_root = tmp.path().join("toolr");
    fs::create_dir_all(&cache_root).unwrap();
    let live_repo = tmp.path().join("live-repo");
    fs::create_dir_all(&live_repo).unwrap();
    write_entry(&cache_root, "a", &live_repo, Utc::now());

    let output = Command::cargo_bin("toolr")
        .unwrap()
        .env("XDG_CACHE_HOME", tmp.path())
        .env_remove("HOME")
        .env("TOOLR_NO_CACHE_HINT", "1")
        .args(["self", "cache", "prune", "--all"])
        .output()
        .unwrap();
    assert!(!output.status.success());

    assert!(cache_root.join("a").exists());
}
