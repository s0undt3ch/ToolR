//! Pre-clap bootstrap: make sure `tools/.toolr-manifest.json` matches the
//! tree before clap parses the user's command, building it when missing or
//! stale.
//!
//! Only a trusted repository (or `TOOLR_TRUST_ALL=1`) has its `tools/` parsed
//! and its manifest read or written. An untrusted one gets an empty manifest,
//! so toolr shows only its built-in commands and leaves every file exactly as
//! found. [`ensure_manifest`] holds the argv and trust policy;
//! `toolr_core::manifest_build::refresh` does the rest.

use std::path::Path;

use toolr_core::manifest::{Manifest, manifest_path};
use toolr_core::manifest_build::{Persist, Refresh, refresh};
use toolr_core::parser::BuiltManifest;
use toolr_core::venv::resolve_venv_path;

use crate::invocation::Invocation;

/// Build the manifest when it's missing or stale: a pure-Rust parse of
/// `tools/*.py` plus an execution-free glob of the venv's plugin fragments,
/// so no repository code runs. An untrusted repository is not read at all and
/// gets an empty manifest; `--version` and built-in commands skip this step
/// and never read the trust store.
///
/// Returns the manifest this step validated or rebuilt, so the caller
/// dispatches against it instead of re-reading a file another toolr
/// process may be rewriting (#542), with the warnings of a build this step
/// ran; a cached manifest has none. `Ok(None)` means the step didn't
/// apply (no project, skip-list argv) or the rebuild failed with no
/// usable cache; the caller then reads the file itself.
pub(crate) fn ensure_manifest(run: &Invocation) -> anyhow::Result<Option<BuiltManifest>> {
    if run.argv.skips_manifest_build() {
        return Ok(None);
    }
    let Ok(root) = run.root() else {
        return Ok(None);
    };
    if run.untrusted_root().is_some() {
        return Ok(Some(Manifest::empty().into()));
    }
    ensure_manifest_at(root)
}

/// [`ensure_manifest`] for a discovered `root`. A rebuild that fails warns
/// and falls back to the cached manifest, except on a trusted fresh clone,
/// which has none to fall back to and returns the error. Errors also
/// propagate from writing the rebuilt manifest and for symlinked modules
/// (F-17), which never fall back to the cache.
fn ensure_manifest_at(root: &Path) -> anyhow::Result<Option<BuiltManifest>> {
    // A venv toolr didn't build contributes no plugin commands, not even to `--help`.
    let venv = resolve_venv_path(root)
        .ok()
        .and_then(|r| r.usable_venv_dir(root).map(Path::to_path_buf));
    let refreshed = refresh(root, venv.as_deref(), Persist::Write)?;
    let unreadable = refreshed.unreadable.is_some();
    if let Some(e) = refreshed.unreadable {
        eprintln!(
            "toolr: warning: ignoring unreadable {}: {e}; rebuilding",
            manifest_path(root).display()
        );
    }
    Ok(match refreshed.refresh {
        Refresh::Fresh(cached) => Some(cached.into()),
        Refresh::Built(built) => Some(built),
        // A trusted fresh clone has no manifest to fall back to. Decided from
        // the locked check, since another process may have written one
        // meanwhile; an unreadable file means a manifest existed, so it isn't one.
        Refresh::Failed {
            error,
            cached: None,
        } if !unreadable => return Err(error),
        Refresh::Failed { error, cached } => {
            warn_and_keep_cache(&error, cached.is_some());
            cached.map(Into::into)
        }
    })
}

fn warn_and_keep_cache(err: &anyhow::Error, had_cache: bool) {
    eprintln!(
        "toolr: warning: tools manifest is stale and a fresh build failed; \
         falling back to cached manifest"
    );
    // The whole chain: the outermost context alone names the file but not what is wrong with it.
    let s = format!("{err:#}");
    let first = s.lines().next().unwrap_or(&s);
    eprintln!("toolr: warning: cause: {first}");
    if !had_cache {
        eprintln!(
            "toolr: warning: no cached manifest available — `toolr <user-cmd>` \
             will likely fail until you fix the build error"
        );
    }
    eprintln!("toolr: warning: run `toolr project manifest rebuild` to see the full error");
}

#[cfg(test)]
mod tests;
