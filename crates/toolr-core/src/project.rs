//! High-level orchestration: find repo, ensure uv, sync venv, validate.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cache::meta::Meta;
use crate::discovery::discover_project_root;
use crate::manifest_build::rebuild_manifest_full;
use crate::trust::{TrustMode, require_trust};
use crate::uv::{UvBinary, UvError, ensure_uv, install::ConsentMode, toolr_cache_dir};
use crate::venv::{
    ResolvedVenv, UpgradeMode, compute_repo_key, resolve_venv_path, sync::sync_if_needed,
    validate::validate_venv,
};

/// Options for [`ensure_venv_ready`]. Constructed via `Default::default()`
/// plus the builder setters; new fields can be added without breaking
/// callers that took an `EnsureOpts::default()`.
#[derive(Debug, Clone, Default)]
pub struct EnsureOpts {
    /// Run `uv sync` even when the freshness stamp says the venv is fresh.
    pub force_sync: bool,
    /// Forward `--quiet` to the uv subprocess. Has no effect when the
    /// stamp short-circuits sync (no uv invocation happens).
    pub quiet: bool,
    /// Whether to pass `-U` / `-P` flags through to uv.
    pub upgrade: UpgradeMode,
    /// How to ask for repository trust when the repo isn't trusted yet.
    pub trust: TrustMode,
}

impl EnsureOpts {
    pub fn with_force_sync(mut self, v: bool) -> Self {
        self.force_sync = v;
        self
    }
    pub fn with_quiet(mut self, v: bool) -> Self {
        self.quiet = v;
        self
    }
    pub fn with_upgrade(mut self, mode: UpgradeMode) -> Self {
        self.upgrade = mode;
        self
    }
    pub fn with_trust(mut self, mode: TrustMode) -> Self {
        self.trust = mode;
        self
    }
}

/// One-stop "make the venv ready" entrypoint. Returns the resolved venv
/// + the chosen uv binary on success.
pub fn ensure_venv_ready(
    cwd: &Path,
    consent: ConsentMode,
    opts: EnsureOpts,
) -> Result<(ResolvedVenv, UvBinary)> {
    let repo_root =
        discover_project_root(cwd).context("locating project root for the tools venv")?;
    let resolved = resolve_venv_path(&repo_root).context("resolving the tools venv path")?;
    // After the static pyproject read, so a broken one stays a benign
    // `--quiet` exit; before anything repo-controlled runs.
    require_trust(&repo_root, opts.trust)?;
    let uv = ensure_uv(consent).map_err(UvError::into_anyhow)?;
    let tools = repo_root.join("tools");
    sync_if_needed(
        &uv,
        &tools,
        &resolved,
        opts.force_sync,
        opts.quiet,
        &opts.upgrade,
    )
    .with_context(|| format!("uv sync against {}", tools.display()))?;
    validate_venv(&resolved.venv_dir, &resolved.python).context("validating the synced venv")?;
    if let Err(e) = finalize_sync(&repo_root, &resolved) {
        eprintln!("toolr: warning: failed to finalize sync (meta/manifest): {e}");
    }
    Ok((resolved, uv))
}

/// Resolve the out-of-repo cache directory that holds this repo's
/// `meta.json` sidecar.
///
/// For cache-located venvs the sidecar lives next to the venv
/// (`<cache>/<key>/meta.json`, the venv being `<cache>/<key>/venv`). For
/// in-tree venvs (`tools/.venv`) the venv stays in the repo but the
/// sidecar must live somewhere the repo cannot write, so it goes to
/// `$XDG_CACHE_HOME/toolr/<repo-key>/` keyed by a hash of the canonical
/// repo path.
pub fn meta_cache_dir(repo_root: &Path, resolved: &ResolvedVenv) -> Result<PathBuf> {
    if !resolved.repo_key.is_empty() {
        // Cache-located venv: sidecar sits beside the venv.
        return resolved
            .venv_dir
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow::anyhow!("cache venv dir has no parent"));
    }
    // In-tree venv: compute an out-of-repo cache slot.
    let key = compute_repo_key(repo_root, &resolved.python_version)?;
    let base = toolr_cache_dir()
        .ok_or_else(|| anyhow::anyhow!("could not resolve toolr cache directory"))?;
    Ok(base.join(key))
}

/// Final step of a successful sync: write the out-of-repo `meta.json`
/// sidecar (repo path, versions, timestamps) and rebuild the manifest so
/// third-party plugin commands appear immediately (design §2 and §3).
pub fn finalize_sync(repo_root: &Path, resolved: &ResolvedVenv) -> Result<()> {
    let cache_dir = meta_cache_dir(repo_root, resolved)?;

    // Preserve created_at when a sidecar already exists; otherwise mint a
    // fresh one.
    let meta = match Meta::load(&cache_dir) {
        Ok(existing) => existing,
        Err(_) => Meta::new(
            repo_root.to_path_buf(),
            env!("CARGO_PKG_VERSION"),
            &resolved.python_version,
        ),
    };
    meta.write(&cache_dir)
        .with_context(|| format!("writing meta.json to {}", cache_dir.display()))?;

    rebuild_manifest_full(repo_root, &resolved.venv_dir)
        .with_context(|| "rebuilding manifest after sync")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_env::EnvGuard;
    use crate::venv::config::ToolrConfig;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn make_resolved_cache(venv_dir: PathBuf, repo_key: &str) -> ResolvedVenv {
        ResolvedVenv {
            python: venv_dir.join("bin").join("python"),
            venv_dir,
            repo_key: repo_key.to_string(),
            python_version: "3.13.1".to_string(),
            config: ToolrConfig::default(),
        }
    }

    #[test]
    fn ensure_opts_default_means_no_force_no_quiet() {
        let opts = EnsureOpts::default();
        assert!(!opts.force_sync);
        assert!(!opts.quiet);
    }

    #[test]
    fn ensure_opts_builder_setters_work() {
        let opts = EnsureOpts::default().with_force_sync(true).with_quiet(true);
        assert!(opts.force_sync);
        assert!(opts.quiet);
    }

    #[test]
    fn ensure_venv_ready_reports_missing_project_root() {
        let _env = EnvGuard::read();
        // No `tools/pyproject.toml` - `ensure_venv_ready` must fail at
        // discovery, before any uv interaction. A bare host `tools/` (the
        // Windows runners' `C:\tools\`) fails there too.
        let tmp = tempfile::tempdir().unwrap();
        let err = ensure_venv_ready(tmp.path(), ConsentMode::default(), EnsureOpts::default())
            .expect_err("expected ensure_venv_ready to fail without a project");
        let chain: Vec<String> = err.chain().map(|e| e.to_string()).collect();
        assert!(
            chain.iter().any(|m| m.contains("locating project root")),
            "expected discovery to fail, got: {chain:?}"
        );
    }

    #[test]
    fn meta_cache_dir_for_cache_venv_is_venv_parent() {
        // Cache-located venv (`repo_key` non-empty): the sidecar lives
        // next to the venv, never derived from the cache dir.
        let resolved = make_resolved_cache(PathBuf::from("/cache/toolr/abc/venv"), "abc");
        let dir = meta_cache_dir(&PathBuf::from("/repo"), &resolved).unwrap();
        assert_eq!(dir, PathBuf::from("/cache/toolr/abc"));
    }

    #[test]
    fn meta_cache_dir_for_in_tree_venv_uses_an_out_of_repo_cache_slot() {
        // In-tree venv (`repo_key` empty): the sidecar must live outside the
        // repo, under `$XDG_CACHE_HOME/toolr/<repo-key>` — exercises the
        // `compute_repo_key` + cache-dir branch.
        let (_env, xdg) = EnvGuard::temp_cache_home();

        let repo = TempDir::new().unwrap();
        let resolved = ResolvedVenv {
            python: repo.path().join("tools/.venv/bin/python"),
            venv_dir: repo.path().join("tools/.venv"),
            repo_key: String::new(), // in-tree
            python_version: "3.13".to_string(),
            config: ToolrConfig::default(),
        };
        let dir = meta_cache_dir(repo.path(), &resolved);
        let dir = dir.unwrap();
        assert!(
            dir.starts_with(xdg.path().join("toolr")),
            "in-tree meta must live under the toolr cache dir, got {dir:?}"
        );
    }

    #[test]
    fn ensure_opts_with_upgrade_sets_mode() {
        use crate::venv::UpgradeMode;
        let opts = EnsureOpts::default().with_upgrade(UpgradeMode::Packages(vec!["foo".into()]));
        match opts.upgrade {
            UpgradeMode::Packages(ref p) => assert_eq!(p, &vec!["foo".to_string()]),
            other => panic!("expected Packages, got {other:?}"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn finalize_sync_writes_meta_and_rebuilds_manifest() {
        let (_env, _cache_home) = EnvGuard::temp_cache_home();
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        // A cache-located venv keyed by "abc" so the sidecar lands next to it.
        let cache_entry = tmp.path().join("cache-entry");
        let venv_dir = cache_entry.join("venv");
        let bin = venv_dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let py = bin.join("python");
        std::fs::write(&py, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&py, std::fs::Permissions::from_mode(0o755)).unwrap();

        // A minimal repo with a statically-parseable tools/ dir.
        let repo_root = tmp.path().join("my-project");
        let tools = repo_root.join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::write(
            tools.join("ci.py"),
            "\"\"\"CI.\"\"\"\ngroup = command_group(\"ci\", \"CI\")\n@group.command\ndef hello(ctx):\n    \"\"\"Hi.\"\"\"\n",
        )
        .unwrap();

        let resolved = make_resolved_cache(venv_dir.clone(), "abc");
        finalize_sync(&repo_root, &resolved).unwrap();

        // Sidecar written next to the venv; no interpreter fields recorded.
        let cache_dir = venv_dir.parent().unwrap();
        let meta = Meta::load(cache_dir).unwrap();
        assert_eq!(meta.repo_path, repo_root);
        assert!(
            meta.interpreter_path.is_none(),
            "interpreter_path not recorded"
        );
        assert!(
            meta.interpreter_hash.is_none(),
            "interpreter_hash not recorded"
        );
        // Manifest rebuilt as the final step.
        assert!(tools.join(".toolr-manifest.json").is_file());
    }

    #[test]
    #[cfg(unix)]
    fn finalize_sync_preserves_an_existing_sidecar() {
        let (_env, _cache_home) = EnvGuard::temp_cache_home();
        // When a sidecar already exists, finalize_sync takes the
        // `Meta::load -> Ok(existing)` branch and preserves `created_at`
        // (rather than minting a fresh one).
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let venv_dir = tmp.path().join("cache-entry").join("venv");
        let bin = venv_dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let py = bin.join("python");
        std::fs::write(&py, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&py, std::fs::Permissions::from_mode(0o755)).unwrap();

        let repo_root = tmp.path().join("proj");
        let tools = repo_root.join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::write(
            tools.join("ci.py"),
            "\"\"\"CI.\"\"\"\ngroup = command_group(\"ci\", \"CI\")\n",
        )
        .unwrap();

        // Pre-seed a sidecar so the `Ok(existing)` branch runs.
        let cache_dir = venv_dir.parent().unwrap();
        Meta::new(&repo_root, "old-version", "3.13")
            .write(cache_dir)
            .unwrap();
        let created = Meta::load(cache_dir).unwrap().created_at;

        let resolved = make_resolved_cache(venv_dir.clone(), "abc");
        finalize_sync(&repo_root, &resolved).unwrap();

        let meta = Meta::load(cache_dir).unwrap();
        assert_eq!(
            meta.created_at, created,
            "created_at preserved from the existing sidecar"
        );
        assert!(meta.interpreter_path.is_none());
    }
}
