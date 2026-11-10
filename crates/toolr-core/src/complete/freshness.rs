//! Tab-time manifest freshness logic.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::discovery::discover_project_root;
use crate::manifest::Manifest;
use crate::manifest_build::{Refresh, refresh};

/// Outcome of resolving the manifest for a completion request.
#[derive(Debug)]
pub struct ResolvedManifest {
    pub manifest: Manifest,
    /// `true` if the cached on-disk manifest was returned verbatim
    /// (`static_hash`, `third_party_hash`, and `toolr_version` all
    /// matched the live state).
    pub from_cache: bool,
    /// The directory that contained `tools/` (the project root).
    pub project_root: PathBuf,
}

/// Resolve the manifest to serve for a completion request rooted at
/// `cwd`. Walks up to find `tools/` and either returns the cached manifest
/// verbatim or re-parses and returns a fresh one in memory (with the
/// cache's third-party entries preserved). A re-parse that fails serves
/// the cache, if any, unless it found symlinked modules.
pub fn resolve_manifest_at_tab(cwd: &Path) -> Result<ResolvedManifest> {
    let project_root = discover_project_root(cwd)
        .with_context(|| format!("walking up from {} to find tools/", cwd.display()))?;
    // No venv: globbing it is too costly on the hot path, so the cached
    // third-party state is canonical; the next dispatch re-globs. Never
    // written, so no lock. Every completion script sends stderr to
    // /dev/null, so an unreadable cache, like a failed build, is left to the
    // next non-TAB run to report.
    let (mut manifest, from_cache) = match refresh(&project_root, None, false)?.refresh {
        Refresh::Fresh(cached) => (cached, true),
        Refresh::Built(fresh) => (fresh.manifest, false),
        // A half-edited pyproject or a broken module must not empty TAB.
        Refresh::Failed {
            cached: Some(cached),
            ..
        } => (cached, true),
        Refresh::Failed {
            error,
            cached: None,
        } => return Err(error),
    };
    // Dispatch drops the same entries in `main.rs`; clap and TAB must agree.
    manifest.drop_unsafe_names();
    Ok(ResolvedManifest {
        manifest,
        from_cache,
        project_root,
    })
}
