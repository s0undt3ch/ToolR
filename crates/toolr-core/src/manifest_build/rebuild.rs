//! The one manifest rebuild pipeline: [`refresh`] for the automatic paths
//! (dispatch, `--help`, TAB) and [`rebuild_manifest_full`] for
//! `project manifest rebuild`. Between them they own the rebuild lock, the
//! snapshot clock, the third-party hash and what a build carries forward
//! from the cache.

use std::ops::ControlFlow;
use std::path::Path;

use anyhow::{Context, Result};

use super::hash::{compute_third_party_hash, empty_third_party_hash};
use crate::freshness::{FreshnessVerdict, compare};
use crate::hash::{build_to_persist, context_unless_symlinked, is_symlinked_modules};
use crate::manifest::{
    Manifest, ManifestError, Origin, acquire_rebuild_lock, load_manifest, manifest_path,
    rebuild_if_stale, write_manifest,
};
use crate::parser::{
    BuildError, BuiltManifest, build_static_manifest, build_static_manifest_with_venv,
};

/// Whether [`refresh`] writes what it builds. Callers only reach it for a
/// trusted repository: an untrusted one is never refreshed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Persist {
    /// Rebuild under the rebuild lock (re-checked once held) and write.
    Write,
    /// Build in memory: no lock, no snapshot clock, no write. For Tab, which
    /// must leave the tree as found.
    InMemory,
}

/// What [`refresh`] served.
#[derive(Debug)]
pub enum Refresh {
    /// The manifest on disk matched the live state.
    Fresh(Manifest),
    /// A new build, written under [`Persist::Write`].
    Built(BuiltManifest),
    /// The build failed; `cached` is the manifest on disk, stale or not,
    /// when it loaded. Symlinked modules are an `Err` from [`refresh`]
    /// instead, since the cache would hide the links again.
    Failed {
        error: anyhow::Error,
        cached: Option<Manifest>,
    },
}

/// [`refresh`]'s result, with the first error that made the manifest on
/// disk unreadable (a missing file isn't one).
#[derive(Debug)]
pub struct Refreshed {
    pub refresh: Refresh,
    pub unreadable: Option<ManifestError>,
}

/// Serve `project_root`'s manifest, rebuilding it only when it's stale.
///
/// `venv` decides how a build treats third-party plugins: with one, their
/// fragments are globbed and hashed afresh; without one (TAB, or a venv
/// that doesn't resolve), the cached plugin entries, warnings and hash are
/// carried forward.
pub fn refresh(project_root: &Path, venv: Option<&Path>, persist: Persist) -> Result<Refreshed> {
    let tools = project_root.join("tools");
    let manifest_path = manifest_path(project_root);
    let mut unreadable = None;
    // Under `Persist::Write` a stale manifest is checked twice, before and under the lock.
    let mut check = || -> Result<ControlFlow<Refresh, Option<Manifest>>> {
        let cached = match load_manifest(&manifest_path) {
            Ok(m) => Some(m),
            Err(ManifestError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                unreadable.get_or_insert(e);
                None
            }
        };
        Ok(match (compare(cached.as_ref(), &tools, venv)?, cached) {
            (FreshnessVerdict::Fresh, Some(fresh)) => ControlFlow::Break(Refresh::Fresh(fresh)),
            (_, cached) => ControlFlow::Continue(cached),
        })
    };
    let rebuild = |cached: Option<Manifest>| -> Result<Refresh> {
        let built = match persist {
            Persist::Write => build_to_persist(&tools, || build(&tools, venv, cached.as_ref())),
            Persist::InMemory => build(&tools, venv, cached.as_ref()),
        };
        match built {
            Ok(built) => {
                if persist == Persist::Write {
                    write_manifest(&manifest_path, &built.manifest)
                        .with_context(|| format!("writing {}", manifest_path.display()))?;
                }
                Ok(Refresh::Built(built))
            }
            Err(e) if is_symlinked_modules(&e) => Err(e),
            Err(error) => Ok(Refresh::Failed { error, cached }),
        }
    };
    let refresh = match persist {
        Persist::Write => rebuild_if_stale(&manifest_path, check, rebuild)?,
        Persist::InMemory => match check()? {
            ControlFlow::Break(fresh) => fresh,
            ControlFlow::Continue(cached) => rebuild(cached)?,
        },
    };
    Ok(Refreshed {
        refresh,
        unreadable,
    })
}

/// Always rebuild `project_root`'s manifest from the tree and the venv's
/// plugin fragments, and write it; with no `venv`, it has no plugin commands.
/// The lock only serialises this with concurrent auto-rebuilds.
pub fn rebuild_manifest_full(project_root: &Path, venv: Option<&Path>) -> Result<BuiltManifest> {
    let tools = project_root.join("tools");
    let manifest_path = manifest_path(project_root);
    let _lock = acquire_rebuild_lock(&manifest_path);
    let built = build_to_persist(&tools, || build(&tools, venv, None)).map_err(|e| {
        context_unless_symlinked(e, "building static manifest (incl. third-party fragments)")
    })?;
    write_manifest(&manifest_path, &built.manifest)?;
    Ok(built)
}

/// Build from `tools` and stamp the third-party hash. With a `venv`, plugin
/// entries come from its fragments; without one, from `cached`.
fn build(tools: &Path, venv: Option<&Path>, cached: Option<&Manifest>) -> Result<BuiltManifest> {
    let Some(venv) = venv else {
        let mut built = build_static_manifest(tools)?;
        match cached {
            Some(cached) => carry_forward(&mut built.manifest, cached),
            None => built.manifest.third_party_hash = empty_third_party_hash(),
        }
        return Ok(built);
    };
    let mut built =
        build_static_manifest_with_venv(tools, venv).map_err(BuildError::into_anyhow)?;
    built.manifest.third_party_hash =
        compute_third_party_hash(venv).context("hashing third-party manifests")?;
    Ok(built)
}

/// Carry `cached`'s plugin entries that no fresh entry shadows, its plugin
/// warnings (a stale warning beats a silently hidden command) and its
/// third-party hash into a build that didn't glob the venv.
fn carry_forward(fresh: &mut Manifest, cached: &Manifest) {
    for group in &cached.groups {
        if group.origin == Origin::ThirdParty
            && !fresh
                .groups
                .iter()
                .any(|g| g.full_path() == group.full_path())
        {
            fresh.groups.push(group.clone());
        }
    }
    for cmd in &cached.commands {
        if cmd.origin == Origin::ThirdParty
            && !fresh
                .commands
                .iter()
                .any(|c| c.group == cmd.group && c.name == cmd.name)
        {
            fresh.commands.push(cmd.clone());
        }
    }
    fresh.plugin_warnings.clone_from(&cached.plugin_warnings);
    fresh.third_party_hash.clone_from(&cached.third_party_hash);
}

#[cfg(test)]
mod tests;
