//! Integration tests for `toolr self cache list`.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use chrono::Utc;
use tempfile::TempDir;

fn write_entry(cache_root: &Path, key: &str, repo_path: &str, bytes: usize) {
    let cache_dir = cache_root.join(key);
    fs::create_dir_all(cache_dir.join("venv")).unwrap();
    fs::write(cache_dir.join("venv/blob.bin"), vec![0u8; bytes]).unwrap();

    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let json = format!(
        r#"{{
          "schema_version": 1,
          "repo_path": "{repo_path}",
          "toolr_version": "1.0.0",
          "python_version": "3.13.1",
          "created_at": "{now}",
          "last_used_at": "{now}"
        }}"#
    );
    fs::write(cache_dir.join("meta.json"), json).unwrap();
}

#[test]
fn list_reports_no_caches_when_empty() {
    let tmp = TempDir::new().unwrap();
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .env("XDG_CACHE_HOME", tmp.path())
        .env_remove("HOME")
        .env("TOOLR_NO_CACHE_HINT", "1")
        .args(["self", "cache", "list"])
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("no cached virtualenvs"), "stdout:\n{stdout}");
}

#[test]
fn list_renders_entries_with_size_and_last_used() {
    let tmp = TempDir::new().unwrap();
    let cache_root = tmp.path().join("toolr");
    fs::create_dir_all(&cache_root).unwrap();
    write_entry(&cache_root, "key-a", "/repo/a", 4096);

    let output = Command::cargo_bin("toolr")
        .unwrap()
        .env("XDG_CACHE_HOME", tmp.path())
        .env_remove("HOME")
        .env("TOOLR_NO_CACHE_HINT", "1")
        .args(["self", "cache", "list"])
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("/repo/a"), "stdout:\n{stdout}");
    assert!(stdout.contains("REPO"));
    assert!(stdout.contains("SIZE"));
    assert!(stdout.contains("LAST USED"));
}
