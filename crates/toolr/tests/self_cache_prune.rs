//! Integration tests for `toolr self cache prune` and `--all`.

mod common;

use std::fs;
use std::path::Path;

use chrono::{Duration, Utc};
use tempfile::TempDir;
use toolr_core::manifest::{LOCKS_DIR, lock_path_for_repo};

fn write_entry(
    cache_root: &Path,
    key: &str,
    repo_path: &Path,
    last_used_at: chrono::DateTime<Utc>,
) {
    let cache_dir = cache_root.join(key);
    fs::create_dir_all(cache_dir.join("venv")).unwrap();
    fs::write(cache_dir.join("venv/blob.bin"), vec![0u8; 256]).unwrap();
    let stamp = last_used_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    // Build JSON via serde_json so Windows backslash paths get
    // properly escaped — `format!` + `Path::display()` produced
    // invalid JSON on Windows (`"C:\Users\..."` has illegal escapes),
    // and the cache parser silently skipped malformed entries.
    let meta = serde_json::json!({
        "schema_version": 1,
        "repo_path": repo_path.to_string_lossy(),
        "toolr_version": "1.0.0",
        "python_version": "3.13.1",
        "created_at": stamp,
        "last_used_at": stamp,
    });
    fs::write(cache_dir.join("meta.json"), meta.to_string()).unwrap();
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

    let output = common::toolr()
        .env("XDG_CACHE_HOME", tmp.path())
        .env_remove("HOME")
        .env("TOOLR_NO_CACHE_HINT", "1")
        .args(["self", "cache", "prune"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
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

    let output = common::toolr()
        .env("XDG_CACHE_HOME", tmp.path())
        .env_remove("HOME")
        .env("TOOLR_NO_CACHE_HINT", "1")
        .args(["self", "cache", "prune"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
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

    let output = common::toolr()
        .env("XDG_CACHE_HOME", tmp.path())
        .env_remove("HOME")
        .env("TOOLR_NO_CACHE_HINT", "1")
        .args(["self", "cache", "prune", "--dry-run"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
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

    let output = common::toolr()
        .env("XDG_CACHE_HOME", tmp.path())
        .env_remove("HOME")
        .env("TOOLR_NO_CACHE_HINT", "1")
        .args(["self", "cache", "prune", "--all", "--yes"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(!cache_root.join("a").exists());
    assert!(!cache_root.join("b").exists());
}

#[test]
fn passive_hint_appears_when_orphan_count_is_high() {
    let tmp = TempDir::new().unwrap();
    let cache_root = tmp.path().join("toolr");
    fs::create_dir_all(&cache_root).unwrap();
    for i in 0..15 {
        write_entry(
            &cache_root,
            &format!("orphan-{i}"),
            Path::new("/definitely/missing/repo"),
            Utc::now(),
        );
    }

    let output = common::toolr()
        .env("XDG_CACHE_HOME", tmp.path())
        .env_remove("HOME")
        .env_remove("TOOLR_NO_CACHE_HINT")
        .args(["--version"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("toolr self cache prune"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn passive_hint_is_suppressed_by_env_var() {
    let tmp = TempDir::new().unwrap();
    let cache_root = tmp.path().join("toolr");
    fs::create_dir_all(&cache_root).unwrap();
    for i in 0..15 {
        write_entry(
            &cache_root,
            &format!("orphan-{i}"),
            Path::new("/definitely/missing/repo"),
            Utc::now(),
        );
    }
    let output = common::toolr()
        .env("XDG_CACHE_HOME", tmp.path())
        .env_remove("HOME")
        .env("TOOLR_NO_CACHE_HINT", "1")
        .args(["--version"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("toolr self cache prune"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn prune_all_refuses_without_yes_when_non_interactive() {
    let tmp = TempDir::new().unwrap();
    let cache_root = tmp.path().join("toolr");
    fs::create_dir_all(&cache_root).unwrap();
    let live_repo = tmp.path().join("live-repo");
    fs::create_dir_all(&live_repo).unwrap();
    write_entry(&cache_root, "a", &live_repo, Utc::now());

    let output = common::toolr()
        .env("XDG_CACHE_HOME", tmp.path())
        .env_remove("HOME")
        .env("TOOLR_NO_CACHE_HINT", "1")
        .args(["self", "cache", "prune", "--all"])
        .output()
        .unwrap();
    assert!(!output.status.success());

    assert!(cache_root.join("a").exists());
}

/// A cache with one fresh entry for a live repo, that repo's rebuild lock,
/// and one stray lock no entry needs. Returns `(tmp, kept_lock, stray_lock)`.
fn cache_with_kept_and_stray_lock() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let cache_root = tmp.path().join("toolr");
    let live_repo = tmp.path().join("live-repo");
    fs::create_dir_all(live_repo.join("tools")).unwrap();
    write_entry(&cache_root, "live-key", &live_repo, Utc::now());
    let kept = lock_path_for_repo(&cache_root, &live_repo);
    let stray = cache_root.join(LOCKS_DIR).join("stray.lock");
    fs::create_dir_all(cache_root.join(LOCKS_DIR)).unwrap();
    fs::write(&kept, b"").unwrap();
    fs::write(&stray, b"").unwrap();
    (tmp, kept, stray)
}

fn run_prune(xdg_cache_home: &Path, args: &[&str]) -> String {
    let output = common::toolr()
        .env("XDG_CACHE_HOME", xdg_cache_home)
        .env_remove("HOME")
        .env("TOOLR_NO_CACHE_HINT", "1")
        .args(["self", "cache", "prune"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn prune_removes_stray_rebuild_locks() {
    let (tmp, kept, stray) = cache_with_kept_and_stray_lock();
    let stdout = run_prune(tmp.path(), &[]);
    assert!(
        stdout.contains("toolr: removed 1 rebuild lock\n"),
        "stdout:\n{stdout}"
    );
    assert!(!stray.exists());
    assert!(kept.exists());
}

#[test]
fn prune_dry_run_reports_stray_locks() {
    let (tmp, kept, stray) = cache_with_kept_and_stray_lock();
    let stdout = run_prune(tmp.path(), &["--dry-run"]);
    assert!(
        stdout.contains("toolr: would remove 1 rebuild lock\n"),
        "stdout:\n{stdout}"
    );
    assert!(stray.exists());
    assert!(kept.exists());
}

#[test]
fn prune_all_with_no_entries_still_removes_locks() {
    let tmp = TempDir::new().unwrap();
    let locks = tmp.path().join("toolr").join(LOCKS_DIR);
    fs::create_dir_all(&locks).unwrap();
    for name in ["a.lock", "b.lock"] {
        fs::write(locks.join(name), b"").unwrap();
    }
    // No `--yes`: stray locks alone never prompt.
    let stdout = run_prune(tmp.path(), &["--all"]);
    assert!(
        !stdout.contains("no cache entries to remove"),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("toolr: removed 2 rebuild locks\n"),
        "stdout:\n{stdout}"
    );
    assert!(!locks.join("a.lock").exists());
    assert!(!locks.join("b.lock").exists());
}

#[test]
fn prune_all_removes_every_lock_alongside_entries() {
    let (tmp, kept, stray) = cache_with_kept_and_stray_lock();
    let stdout = run_prune(tmp.path(), &["--all", "--yes"]);
    assert!(
        stdout.contains("toolr: removed 2 rebuild locks\n"),
        "stdout:\n{stdout}"
    );
    assert!(!kept.exists());
    assert!(!stray.exists());
}

#[test]
fn prune_with_nothing_to_prune_still_removes_stray_locks() {
    let tmp = TempDir::new().unwrap();
    let locks = tmp.path().join("toolr").join(LOCKS_DIR);
    fs::create_dir_all(&locks).unwrap();
    fs::write(locks.join("stray.lock"), b"").unwrap();
    let stdout = run_prune(tmp.path(), &[]);
    assert!(!stdout.contains("nothing to prune"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("toolr: removed 1 rebuild lock\n"),
        "stdout:\n{stdout}"
    );
    assert!(!locks.join("stray.lock").exists());
}

#[test]
fn prune_warns_and_succeeds_when_lock_sweep_fails() {
    let tmp = TempDir::new().unwrap();
    let cache_root = tmp.path().join("toolr");
    fs::create_dir_all(&cache_root).unwrap();
    // A regular file where `locks/` should be makes the sweep's read_dir fail.
    fs::write(cache_root.join(LOCKS_DIR), b"").unwrap();
    for args in [&[][..], &["--all"][..]] {
        let stdout = run_prune(tmp.path(), args);
        assert!(
            stdout.contains("toolr: warning: failed to sweep rebuild locks"),
            "args {args:?}, stdout:\n{stdout}"
        );
    }
}
