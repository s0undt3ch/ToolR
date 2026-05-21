//! Single-source-of-truth freshness comparison.

use std::path::Path;

use anyhow::{Context, Result};

use crate::dynamic::{compute_third_party_hash, empty_third_party_hash};
use crate::hash::hash_tools_dir;
use crate::manifest::Manifest;

/// Outcome of comparing the live filesystem state to a cached manifest.
///
/// Variants are ordered by "stronger rebuild needed." When both axes
/// drift simultaneously, `compare` returns `ThirdPartyDrift` because the
/// third-party rebuild path is a superset that also re-parses
/// `tools/*.py`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessVerdict {
    /// Cached manifest matches the live state on both axes.
    Fresh,
    /// `tools/*.py` content has drifted; third-party manifests are unchanged.
    StaticDrift,
    /// At least one third-party `toolr-manifest.json` changed (and possibly
    /// the local tools too). Static-drift is a subset; callers should treat
    /// this as the stronger condition.
    ThirdPartyDrift,
}

/// Compare cached manifest hashes against the live filesystem.
///
/// `venv_dir` is optional. When `None`, the third-party hash is treated
/// as `empty_third_party_hash()` — equivalent to running the live
/// computation against an empty venv. A `None` `cached` always returns
/// `ThirdPartyDrift` so the caller produces a fresh manifest from
/// scratch.
///
/// On `StaticDrift`, call `build_static_manifest` and preserve the cached
/// third-party and dynamic entries. On `ThirdPartyDrift`, call
/// `build_static_manifest_with_venv`; third-party entries come from the
/// fresh glob.
pub fn compare(
    cached: Option<&Manifest>,
    tools_dir: &Path,
    venv_dir: Option<&Path>,
) -> Result<FreshnessVerdict> {
    let Some(cached) = cached else {
        return Ok(FreshnessVerdict::ThirdPartyDrift);
    };

    let live_static = hash_tools_dir(tools_dir)
        .with_context(|| format!("hashing {}", tools_dir.display()))?;
    let live_third_party = match venv_dir {
        Some(v) => compute_third_party_hash(v)?,
        None => empty_third_party_hash(),
    };

    if cached.third_party_hash != live_third_party {
        return Ok(FreshnessVerdict::ThirdPartyDrift);
    }
    if cached.static_hash != live_static {
        return Ok(FreshnessVerdict::StaticDrift);
    }
    Ok(FreshnessVerdict::Fresh)
}
