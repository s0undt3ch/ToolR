//! Tab-time manifest freshness logic.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::discovery::discover_project_root;
use crate::hash::hash_tools_dir;
use crate::manifest::{Manifest, Origin, load_manifest};
use crate::parser::build_static_manifest;

/// Outcome of resolving the manifest for a completion request.
#[derive(Debug)]
pub struct ResolvedManifest {
    pub manifest: Manifest,
    /// `true` if the cached on-disk manifest matched the live tools hash.
    pub from_cache: bool,
    /// The directory that contained `tools/` (the project root).
    pub project_root: PathBuf,
}

/// Resolve the manifest to serve for a completion request rooted at
/// `cwd`. Walks up to find `tools/`, hashes its `*.py` files, and either
/// returns the cached manifest verbatim or re-parses and returns a fresh
/// one (with any dynamic-layer entries from the cache preserved).
pub fn resolve_manifest_at_tab(cwd: &Path) -> Result<ResolvedManifest> {
    let project_root = discover_project_root(cwd)
        .with_context(|| format!("walking up from {} to find tools/", cwd.display()))?;
    let tools_dir = project_root.join("tools");
    let manifest_path = tools_dir.join(".toolr-manifest.json");

    let live_hash = hash_tools_dir(&tools_dir)
        .with_context(|| format!("hashing {}", tools_dir.display()))?;
    let cached = load_manifest(&manifest_path).ok();

    if let Some(cached) = cached.as_ref() {
        if cached.static_hash == live_hash {
            return Ok(ResolvedManifest {
                manifest: cached.clone(),
                from_cache: true,
                project_root,
            });
        }
    }

    // Reparse and preserve any dynamic-layer entries from the cache.
    let mut fresh = build_static_manifest(&tools_dir)?;
    if let Some(cached) = cached {
        for group in cached.groups {
            if matches!(group.origin, Origin::Dynamic)
                && !fresh.groups.iter().any(|g| g.name == group.name)
            {
                fresh.groups.push(group);
            }
        }
        for cmd in cached.commands {
            if matches!(cmd.origin, Origin::Dynamic)
                && !fresh
                    .commands
                    .iter()
                    .any(|c| c.group == cmd.group && c.name == cmd.name)
            {
                fresh.commands.push(cmd);
            }
        }
        fresh.dynamic_hash = cached.dynamic_hash;
    }

    Ok(ResolvedManifest {
        manifest: fresh,
        from_cache: false,
        project_root,
    })
}
