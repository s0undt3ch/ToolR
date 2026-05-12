//! High-level orchestration: find repo, ensure uv, sync venv, validate.

use std::path::Path;

use anyhow::{Context, Result};

use crate::discovery::discover_project_root;
use crate::uv::{UvBinary, UvError, ensure_uv, install::ConsentMode};
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
    let uv = match ensure_uv(consent) {
        Ok(uv) => uv,
        Err(e @ UvError::UserRefusedInstall) => return Err(anyhow::anyhow!(e)),
        Err(e) => return Err(anyhow::anyhow!(e)),
    };
    let tools = repo_root.join("tools");
    sync_if_needed(&uv, &tools, &resolved, force_sync)
        .with_context(|| format!("uv sync against {}", tools.display()))?;
    validate_venv(&resolved.venv_dir, &resolved.python)
        .context("validating the synced venv")?;
    // Drop a meta.json sidecar next to the venv (cache-located venvs only).
    // Non-fatal: meta writes never block the user's command.
    if let Some(cache_dir) = resolved.venv_dir.parent() {
        if let Err(e) = crate::cache::write_meta_for_new_venv(
            cache_dir,
            &repo_root,
            env!("CARGO_PKG_VERSION"),
            &resolved.python_version,
        ) {
            eprintln!("toolr: warning: failed to write cache meta.json: {e}");
        }
    }
    let outcomes = perform_editable_installs(
        &uv,
        &resolved.config,
        &repo_root,
        &resolved.python,
    );
    warn_failures(&outcomes);
    Ok((resolved, uv))
}
