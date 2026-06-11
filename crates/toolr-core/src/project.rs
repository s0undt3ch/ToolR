//! High-level orchestration: find repo, ensure uv, sync venv, validate.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cache::meta::Meta;
use crate::discovery::discover_project_root;
use crate::manifest_build::rebuild_manifest_full;
use crate::hash::hash_file;
use crate::uv::{UvBinary, UvError, ensure_uv, install::ConsentMode, toolr_cache_dir};
use crate::venv::{
    ResolvedVenv, UpgradeMode, compute_repo_key, perform_editable_installs, resolve_venv_path,
    sync::sync_if_needed, validate::validate_venv, warn_failures,
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
}

/// One-stop "make the venv ready" entrypoint. Returns the resolved venv
/// + the chosen uv binary on success.
pub fn ensure_venv_ready(
    cwd: &Path,
    consent: ConsentMode,
    opts: EnsureOpts,
) -> Result<(ResolvedVenv, UvBinary)> {
    let repo_root = discover_project_root(cwd)
        .context("locating project root for the tools venv")?;
    let resolved = resolve_venv_path(&repo_root)
        .context("resolving the tools venv path")?;
    let uv = ensure_uv(consent).map_err(UvError::into_anyhow)?;
    let tools = repo_root.join("tools");
    sync_if_needed(&uv, &tools, &resolved, opts.force_sync, opts.quiet, &opts.upgrade)
        .with_context(|| format!("uv sync against {}", tools.display()))?;
    validate_venv(&resolved.venv_dir, &resolved.python)
        .context("validating the synced venv")?;
    if let Err(e) = finalize_sync(&repo_root, &resolved) {
        eprintln!("toolr: warning: failed to finalize sync (provenance/manifest): {e}");
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

/// Resolve the out-of-repo cache directory that holds this repo's
/// `meta.json` provenance sidecar.
///
/// For cache-located venvs the sidecar lives next to the venv
/// (`<cache>/<key>/meta.json`, the venv being `<cache>/<key>/venv`). For
/// in-tree venvs (`tools/.venv`) the venv stays in the repo but its
/// provenance record must live somewhere the repo cannot write, so it
/// goes to `$XDG_CACHE_HOME/toolr/<repo-key>/` keyed by a hash of the
/// canonical repo path.
pub fn provenance_cache_dir(repo_root: &Path, resolved: &ResolvedVenv) -> Result<PathBuf> {
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

/// Final step of a successful sync: record interpreter provenance in the
/// out-of-repo `meta.json` and rebuild the manifest so third-party
/// plugin commands appear immediately (design §2 and §3).
///
/// The interpreter is canonicalised and content-hashed; the resulting
/// record is what `venv::provenance::verify_interpreter` checks at
/// dispatch time before executing an in-repo interpreter.
pub fn finalize_sync(repo_root: &Path, resolved: &ResolvedVenv) -> Result<()> {
    let cache_dir = provenance_cache_dir(repo_root, resolved)?;
    let canon_py = resolved
        .python
        .canonicalize()
        .unwrap_or_else(|_| resolved.python.clone());
    let py_hash = hash_file(&canon_py)
        .with_context(|| format!("hashing interpreter {}", canon_py.display()))?;

    // Preserve created_at when a sidecar already exists; otherwise mint a
    // fresh one. Either way, stamp the interpreter provenance.
    let meta = match Meta::load(&cache_dir) {
        Ok(existing) => existing,
        Err(_) => Meta::new(
            repo_root.to_path_buf(),
            env!("CARGO_PKG_VERSION"),
            &resolved.python_version,
        ),
    }
    .with_interpreter(canon_py, py_hash);
    meta.write(&cache_dir)
        .with_context(|| format!("writing meta.json to {}", cache_dir.display()))?;

    rebuild_manifest_full(repo_root, &resolved.venv_dir)
        .with_context(|| "rebuilding manifest after sync")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::venv::config::ToolrConfig;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use tempfile::TempDir;

    /// Serialises the `XDG_CACHE_HOME`-mutating test below (process-global env).
    static ENV_LOCK: Mutex<()> = Mutex::new(());

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
        // No pyproject.toml or marker - `ensure_venv_ready` must fail and
        // surface a clear error. On most hosts we hit "locating project
        // root" via `discover_project_root` returning NotFound. On GHA
        // Windows runners `C:\tools\` ships pre-populated, so the walk
        // succeeds up to `C:\` and we instead fail at the resolve step
        // ("resolving the tools venv path" because `C:\tools\pyproject.toml`
        // is missing). Either error proves the orchestration aborts before
        // any uv interaction - which is what the test is really asserting.
        let tmp = tempfile::tempdir().unwrap();
        let err = ensure_venv_ready(tmp.path(), ConsentMode::default(), EnsureOpts::default())
            .expect_err("expected ensure_venv_ready to fail without a project");
        let chain: Vec<String> = err.chain().map(|e| e.to_string()).collect();
        let expected_one_of = ["locating project root", "resolving the tools venv path"];
        assert!(
            chain.iter().any(|m| expected_one_of.iter().any(|hint| m.contains(hint))),
            "expected one of {expected_one_of:?} in the error chain, got: {chain:?}"
        );
    }

    #[test]
    fn provenance_cache_dir_for_cache_venv_is_venv_parent() {
        // Cache-located venv (`repo_key` non-empty): the sidecar lives
        // next to the venv, never derived from the cache dir.
        let resolved = make_resolved_cache(PathBuf::from("/cache/toolr/abc/venv"), "abc");
        let dir = provenance_cache_dir(&PathBuf::from("/repo"), &resolved).unwrap();
        assert_eq!(dir, PathBuf::from("/cache/toolr/abc"));
    }

    #[test]
    fn provenance_cache_dir_for_in_tree_venv_uses_an_out_of_repo_cache_slot() {
        // In-tree venv (`repo_key` empty): the sidecar must live outside the
        // repo, under `$XDG_CACHE_HOME/toolr/<repo-key>` — exercises the
        // `compute_repo_key` + cache-dir branch.
        let _guard = ENV_LOCK.lock().unwrap();
        let xdg = TempDir::new().unwrap();
        let prev = std::env::var_os("XDG_CACHE_HOME");
        // SAFETY: serialised by ENV_LOCK; restored before returning.
        unsafe { std::env::set_var("XDG_CACHE_HOME", xdg.path()) };

        let repo = TempDir::new().unwrap();
        let resolved = ResolvedVenv {
            python: repo.path().join("tools/.venv/bin/python"),
            venv_dir: repo.path().join("tools/.venv"),
            repo_key: String::new(), // in-tree
            python_version: "3.13".to_string(),
            config: ToolrConfig::default(),
        };
        let dir = provenance_cache_dir(repo.path(), &resolved);

        match prev {
            Some(v) => unsafe { std::env::set_var("XDG_CACHE_HOME", v) },
            None => unsafe { std::env::remove_var("XDG_CACHE_HOME") },
        }
        let dir = dir.unwrap();
        assert!(
            dir.starts_with(xdg.path().join("toolr")),
            "in-tree provenance must live under the toolr cache dir, got {dir:?}"
        );
    }

    #[test]
    fn ensure_opts_with_upgrade_sets_mode() {
        use crate::venv::UpgradeMode;
        let opts = EnsureOpts::default()
            .with_upgrade(UpgradeMode::Packages(vec!["foo".into()]));
        match opts.upgrade {
            UpgradeMode::Packages(ref p) => assert_eq!(p, &vec!["foo".to_string()]),
            other => panic!("expected Packages, got {other:?}"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn finalize_sync_records_provenance_and_rebuilds_manifest() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        // A cache-located venv keyed by "abc" so provenance lands next to it.
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

        // Provenance recorded next to the venv.
        let cache_dir = venv_dir.parent().unwrap();
        let meta = Meta::load(cache_dir).unwrap();
        assert!(meta.interpreter_path.is_some(), "interpreter_path recorded");
        assert!(meta.interpreter_hash.is_some(), "interpreter_hash recorded");
        // Manifest rebuilt as the final step.
        assert!(tools.join(".toolr-manifest.json").is_file());
    }
}
