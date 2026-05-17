//! High-level orchestration: find repo, ensure uv, sync venv, validate.

use std::path::Path;

use anyhow::{Context, Result};

use crate::discovery::discover_project_root;
use crate::uv::{UvBinary, ensure_uv, install::ConsentMode};
use crate::venv::{
    ResolvedVenv, perform_editable_installs, resolve_venv_path,
    sync::sync_if_needed, validate::validate_venv, warn_failures,
};

/// One-stop "make the venv ready" entrypoint. Returns the resolved venv
/// + the chosen uv binary on success.
pub fn ensure_venv_ready(
    cwd: &Path,
    consent: ConsentMode,
    force_sync: bool,
) -> Result<(ResolvedVenv, UvBinary)> {
    let repo_root = discover_project_root(cwd)
        .context("locating project root for the tools venv")?;
    let resolved = resolve_venv_path(&repo_root)
        .context("resolving the tools venv path")?;
    let uv = ensure_uv(consent).map_err(anyhow::Error::from)?;
    let tools = repo_root.join("tools");
    sync_if_needed(&uv, &tools, &resolved, force_sync)
        .with_context(|| format!("uv sync against {}", tools.display()))?;
    validate_venv(&resolved.venv_dir, &resolved.python)
        .context("validating the synced venv")?;
    write_cache_meta_best_effort(&resolved, &repo_root);
    let outcomes = perform_editable_installs(
        &uv,
        &resolved.config,
        &repo_root,
        &resolved.python,
    );
    warn_failures(&outcomes);
    Ok((resolved, uv))
}

/// Drop a `meta.json` sidecar next to the venv (cache-located venvs only).
///
/// Best-effort: meta writes never block the user's command, since the
/// data is purely informational for `toolr self cache list/prune`. Any
/// failure is surfaced as a warning on stderr and otherwise ignored.
fn write_cache_meta_best_effort(resolved: &ResolvedVenv, repo_root: &Path) {
    let Some(cache_dir) = resolved.venv_dir.parent() else {
        return;
    };
    if let Err(e) = crate::cache::write_meta_for_new_venv(
        cache_dir,
        repo_root,
        env!("CARGO_PKG_VERSION"),
        &resolved.python_version,
    ) {
        eprintln!("toolr: warning: failed to write cache meta.json: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::venv::config::ToolrConfig;
    use std::path::PathBuf;

    fn make_resolved(venv_dir: PathBuf) -> ResolvedVenv {
        ResolvedVenv {
            python: venv_dir.join("bin").join("python"),
            venv_dir,
            repo_key: String::new(),
            python_version: "3.13.1".to_string(),
            config: ToolrConfig::default(),
        }
    }

    #[test]
    fn ensure_venv_ready_reports_missing_project_root() {
        // No pyproject.toml or marker - `discover_project_root` errors and
        // the error is annotated with our "locating project root" context.
        let tmp = tempfile::tempdir().unwrap();
        let err = ensure_venv_ready(tmp.path(), ConsentMode::default(), false)
            .expect_err("expected discovery to fail in an empty dir");
        let chain: Vec<String> = err.chain().map(|e| e.to_string()).collect();
        assert!(
            chain.iter().any(|m| m.contains("locating project root")),
            "expected context message in error chain, got: {chain:?}"
        );
    }

    #[test]
    fn write_cache_meta_no_parent_is_a_silent_noop() {
        // `venv_dir` of just "/" - parent() returns None on Linux/macOS, so
        // the function returns early without touching the cache module.
        let resolved = make_resolved(PathBuf::from("/"));
        let repo_root = PathBuf::from("/tmp");
        write_cache_meta_best_effort(&resolved, &repo_root);
    }

    #[test]
    fn write_cache_meta_writes_sidecar_next_to_venv() {
        let tmp = tempfile::tempdir().unwrap();
        let venv_dir = tmp.path().join("cache-entry").join("venv");
        std::fs::create_dir_all(&venv_dir).unwrap();
        let repo_root = tmp.path().join("my-project");
        std::fs::create_dir_all(&repo_root).unwrap();

        let resolved = make_resolved(venv_dir.clone());
        write_cache_meta_best_effort(&resolved, &repo_root);

        let meta = venv_dir.parent().unwrap().join("meta.json");
        assert!(meta.is_file(), "meta.json should have been written");
        let body = std::fs::read_to_string(&meta).unwrap();
        assert!(body.contains("3.13.1"), "meta.json should contain python_version, got {body}");
    }
}
