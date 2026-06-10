//! Single-source-of-truth freshness comparison.

use std::path::Path;

use anyhow::{Context, Result};

use crate::dynamic::compute_third_party_hash;
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
/// `venv_dir` is optional. When `None`, the third-party axis is skipped
/// entirely — the cache's `third_party_hash` is treated as canonical.
/// Callers that want to detect third-party drift must pass `Some(venv)`.
/// A `None` `cached` always returns `ThirdPartyDrift` so the caller
/// produces a fresh manifest from scratch.
///
/// On `StaticDrift`, call `build_static_manifest` and preserve the cached
/// third-party entries. On `ThirdPartyDrift`, call
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

    // Third-party axis is only checked when the caller actually has a
    // venv. When `venv_dir` is `None`, the caller has explicitly opted
    // out (tab completion's hot path, or dispatch when venv resolution
    // failed). Skip the third-party check — treat cache as canonical.
    if let Some(v) = venv_dir {
        let live_third_party = compute_third_party_hash(v)?;
        if cached.third_party_hash != live_third_party {
            return Ok(FreshnessVerdict::ThirdPartyDrift);
        }
    }

    if cached.static_hash != live_static {
        return Ok(FreshnessVerdict::StaticDrift);
    }
    Ok(FreshnessVerdict::Fresh)
}
