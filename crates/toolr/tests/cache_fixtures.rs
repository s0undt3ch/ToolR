//! End-to-end exercise of the toolr self cache surface against a
//! fixture cache root.

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
    bytes: usize,
) {
    let cache_dir = cache_root.join(key);
    fs::create_dir_all(cache_dir.join("venv")).unwrap();
    fs::write(cache_dir.join("venv/blob.bin"), vec![0u8; bytes]).unwrap();
    let stamp = last_used_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    // Build the JSON via serde_json so paths with backslashes (Windows)
    // get properly escaped. Hand-rolled `format!` produced invalid JSON
    // on Windows — `"C:\Users\..."` has illegal escape sequences and the
    // cache-list code silently skipped those entries.
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

fn cmd_in(tmp: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::cargo_bin("toolr").unwrap();
    cmd.env("XDG_CACHE_HOME", tmp)
        .env_remove("HOME")
        .env("TOOLR_NO_CACHE_HINT", "1")
        .args(args);
    cmd
}

fn assert_contains_in_stdout(output: &std::process::Output, needle: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(needle),
        "expected `{needle}` in stdout, got:\n{stdout}"
    );
}

#[test]
fn end_to_end_list_then_prune_then_prune_all() {
    let tmp = TempDir::new().unwrap();
    let cache_root = tmp.path().join("toolr");
    fs::create_dir_all(&cache_root).unwrap();

    let live_repo = tmp.path().join("live-repo");
    fs::create_dir_all(&live_repo).unwrap();

    // 1 orphan, 1 stale, 1 fresh.
    write_entry(
        &cache_root,
        "orphan",
        Path::new("/no/such/repo"),
        Utc::now(),
        256,
    );
    write_entry(
        &cache_root,
        "stale",
        &live_repo,
        Utc::now() - Duration::days(60),
        256,
    );
    write_entry(
        &cache_root,
        "fresh",
        &live_repo,
        Utc::now() - Duration::days(1),
        256,
    );

    // --- list ---
    let out = cmd_in(tmp.path(), &["self", "cache", "list"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_contains_in_stdout(&out, "/no/such/repo");
    assert_contains_in_stdout(&out, &live_repo.to_string_lossy());

    // --- prune --dry-run: reports orphan + stale, deletes nothing ---
    let out = cmd_in(tmp.path(), &["self", "cache", "prune", "--dry-run"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_contains_in_stdout(&out, "DRY-RUN");
    assert!(cache_root.join("orphan").exists());
    assert!(cache_root.join("stale").exists());
    assert!(cache_root.join("fresh").exists());

    // --- prune: removes orphan + stale, keeps fresh ---
    let out = cmd_in(tmp.path(), &["self", "cache", "prune"]).output().unwrap();
    assert!(out.status.success());
    assert!(!cache_root.join("orphan").exists());
    assert!(!cache_root.join("stale").exists());
    assert!(cache_root.join("fresh").exists());

    // --- prune --all --yes: removes the last entry ---
    let out = cmd_in(tmp.path(), &["self", "cache", "prune", "--all", "--yes"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(!cache_root.join("fresh").exists());
}
