//! Tab-time manifest freshness logic.

use std::path::PathBuf;

use crate::manifest::Manifest;

/// Outcome of resolving the manifest for a completion request.
pub struct ResolvedManifest {
    pub manifest: Manifest,
    /// `true` if the cached on-disk manifest matched the live tools hash.
    pub from_cache: bool,
    /// The directory that contained `tools/` (the project root).
    pub project_root: PathBuf,
}

/// Resolve the manifest to serve for a completion request rooted at
/// `cwd`. Filled in by Task 3.
pub fn resolve_manifest_at_tab(_cwd: &std::path::Path) -> anyhow::Result<ResolvedManifest> {
    anyhow::bail!("resolve_manifest_at_tab not implemented yet")
}
