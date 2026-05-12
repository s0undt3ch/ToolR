//! Drive `uv sync` to materialise the tools venv.

use std::fs;
use std::path::Path;
use std::process::{Command, ExitStatus};
use std::time::SystemTime;

use anyhow::{Context, Result};

use crate::uv::{UvBinary, UvError};

use super::resolve::ResolvedVenv;

/// Marker file written into the venv after each successful sync.
/// Its mtime is compared against `tools/uv.lock`'s mtime to decide
/// whether re-sync is needed.
const FRESHNESS_MARKER: &str = ".toolr-sync-stamp";

/// Decision returned by `is_fresh`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// Venv has never been synced (marker absent or venv missing).
    Missing,
    /// Lock has been edited since last sync.
    Stale,
    /// Marker mtime >= lock mtime.
    Fresh,
}

pub fn check_freshness(resolved: &ResolvedVenv, tools_dir: &Path) -> Freshness {
    let marker = resolved.venv_dir.join(FRESHNESS_MARKER);
    let lock = tools_dir.join("uv.lock");
    let (Ok(marker_meta), Ok(lock_meta)) = (fs::metadata(&marker), fs::metadata(&lock)) else {
        return Freshness::Missing;
    };
    let marker_t = marker_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let lock_t = lock_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    if marker_t >= lock_t {
        Freshness::Fresh
    } else {
        Freshness::Stale
    }
}

/// Run `uv sync --project <tools>` synchronously, inheriting stdio.
pub fn run_uv_sync(
    uv: &UvBinary,
    tools_dir: &Path,
    resolved: &ResolvedVenv,
) -> Result<ExitStatus> {
    // Ensure the parent of an off-tree venv exists so uv can write into it.
    if let Some(parent) = resolved.venv_dir.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut cmd = Command::new(&uv.path);
    cmd.arg("sync")
        .arg("--project")
        .arg(tools_dir)
        .env("UV_PROJECT_ENVIRONMENT", &resolved.venv_dir);
    if let Some(version) = resolved.config.python_version.as_ref() {
        cmd.arg("--python").arg(version);
    }
    let status = cmd
        .status()
        .with_context(|| format!("spawning uv at {}", uv.path.display()))?;
    if status.success() {
        touch_marker(&resolved.venv_dir)?;
    }
    Ok(status)
}

/// Convenience wrapper that maps a failure to `UvError::SyncFailed`.
pub fn sync_if_needed(
    uv: &UvBinary,
    tools_dir: &Path,
    resolved: &ResolvedVenv,
    force: bool,
) -> Result<(), UvError> {
    if !force && matches!(check_freshness(resolved, tools_dir), Freshness::Fresh) {
        return Ok(());
    }
    let status = run_uv_sync(uv, tools_dir, resolved)
        .map_err(|e| UvError::Http(e.to_string()))?;
    if !status.success() {
        return Err(UvError::SyncFailed(status.code()));
    }
    Ok(())
}

fn touch_marker(venv_dir: &Path) -> Result<()> {
    fs::create_dir_all(venv_dir)?;
    fs::write(venv_dir.join(FRESHNESS_MARKER), b"")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::TempDir;

    fn dummy_resolved(venv_dir: std::path::PathBuf) -> ResolvedVenv {
        ResolvedVenv {
            venv_dir: venv_dir.clone(),
            python: venv_dir.join("bin").join("python"),
            repo_key: "x".into(),
            python_version: "3.13".into(),
            config: Default::default(),
        }
    }

    #[test]
    fn missing_marker_or_lock_reports_missing() {
        let tmp = TempDir::new().unwrap();
        let resolved = dummy_resolved(tmp.path().join("venv"));
        assert_eq!(
            check_freshness(&resolved, tmp.path()),
            Freshness::Missing
        );
    }

    #[test]
    fn marker_older_than_lock_reports_stale() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join("venv");
        fs::create_dir_all(&venv).unwrap();
        touch_marker(&venv).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(tmp.path().join("uv.lock"), b"locks").unwrap();
        let resolved = dummy_resolved(venv);
        assert_eq!(
            check_freshness(&resolved, tmp.path()),
            Freshness::Stale
        );
    }

    #[test]
    fn marker_newer_than_lock_reports_fresh() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join("venv");
        fs::create_dir_all(&venv).unwrap();
        std::fs::write(tmp.path().join("uv.lock"), b"locks").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        touch_marker(&venv).unwrap();
        let resolved = dummy_resolved(venv);
        assert_eq!(
            check_freshness(&resolved, tmp.path()),
            Freshness::Fresh
        );
    }
}
