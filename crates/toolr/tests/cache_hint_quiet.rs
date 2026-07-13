//! The passive "your cache is big, consider pruning" hint is
//! *non-error output*. `--quiet` promises to "suppress non-error
//! output", so the hint must not leak under `--quiet` — even when the
//! cache is dirty enough to trip the emission threshold.
//!
//! This is the contract the mise enter-hook recipe depends on: the hook
//! runs `toolr project venv sync --quiet` on every `cd`, and must stay
//! silent in non-toolr directories regardless of the developer's cache
//! state.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use chrono::Utc;
use tempfile::TempDir;

/// Write a single orphan cache entry (its `repo_path` points nowhere,
/// so the classifier counts it as an orphan).
fn write_orphan(cache_root: &Path, key: &str) {
    let cache_dir = cache_root.join(key);
    fs::create_dir_all(cache_dir.join("venv")).unwrap();
    fs::write(cache_dir.join("venv/blob.bin"), vec![0u8; 1024]).unwrap();
    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let meta = serde_json::json!({
        "schema_version": 1,
        "repo_path": format!("/no/such/repo/{key}"),
        "toolr_version": "1.0.0",
        "python_version": "3.13.1",
        "created_at": now,
        "last_used_at": now,
    });
    fs::write(cache_dir.join("meta.json"), meta.to_string()).unwrap();
}

/// Build an `XDG_CACHE_HOME` whose `toolr/` cache root holds enough
/// orphans to trip the hint (default threshold is `> 10`).
fn dirty_cache() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let cache_root = tmp.path().join("toolr");
    fs::create_dir_all(&cache_root).unwrap();
    for i in 0..12 {
        write_orphan(&cache_root, &format!("orphan-{i}"));
    }
    tmp
}

fn toolr(xdg_cache: &Path, work: &Path, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(work)
        .env("XDG_CACHE_HOME", xdg_cache)
        .env_remove("HOME")
        // Deliberately NOT setting TOOLR_NO_CACHE_HINT — this suite is
        // about the hint actually firing (or being suppressed by --quiet).
        .env_remove("TOOLR_NO_CACHE_HINT")
        .args(args)
        .output()
        .unwrap()
}

/// Control: without `--quiet`, the dirty cache DOES surface the hint on
/// stderr. Proves the fixture trips the threshold, so the suppression
/// test below can't pass vacuously.
#[test]
fn dirty_cache_surfaces_hint_without_quiet() {
    let cache = dirty_cache();
    let work = TempDir::new().unwrap();
    let out = toolr(cache.path(), work.path(), &["project", "venv", "sync"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Run `toolr self cache prune`"),
        "expected the passive cache hint on stderr, got:\n{stderr}"
    );
}

/// Regression: `--quiet` suppresses the passive cache hint even with a
/// dirty cache. Before the fix the hint leaked to stderr and broke the
/// "`--quiet` must be silent" contract whenever the cache was dirty.
#[test]
fn quiet_suppresses_passive_cache_hint() {
    let cache = dirty_cache();
    let work = TempDir::new().unwrap();
    let out = toolr(cache.path(), work.path(), &["project", "venv", "sync", "--quiet"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("cache has"),
        "--quiet must suppress the passive cache hint, got stderr:\n{stderr}"
    );
}
