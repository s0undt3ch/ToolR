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
mod tests {
    use super::{ensure_manifest_at, manifest_path};
    use toolr_core::freshness::{FreshnessVerdict, compare};
    use toolr_core::manifest::{Manifest, load_manifest};
    use toolr_core::manifest_build::{compute_third_party_hash, empty_third_party_hash};

    fn tools_repo() -> tempfile::TempDir {
        let tmp = tempfile::TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::write(tools.join("pyproject.toml"), "[project]\nname = \"t\"\n").unwrap();
        std::fs::write(tools.join("demo.py"), "\"\"\"Demo.\"\"\"\n").unwrap();
        tmp
    }

    static CACHE_HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    const GUARDED_ENV: [&str; 3] = ["XDG_CACHE_HOME", "XDG_STATE_HOME", "TOOLR_TRUST_ALL"];

    /// Restores [`GUARDED_ENV`] to what it was at capture, on drop. Only while
    /// `CACHE_HOME_LOCK` is held.
    struct EnvRestore {
        prev: Vec<Option<std::ffi::OsString>>,
    }

    impl EnvRestore {
        fn capture() -> Self {
            Self {
                prev: GUARDED_ENV.iter().map(std::env::var_os).collect(),
            }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            // SAFETY: CACHE_HOME_LOCK serialises every test in this module
            // that touches env vars, and its holder outlives this value.
            unsafe {
                for (name, prev) in GUARDED_ENV.iter().zip(&self.prev) {
                    match prev {
                        Some(v) => std::env::set_var(name, v),
                        None => std::env::remove_var(name),
                    }
                }
            }
        }
    }

    /// Restores [`GUARDED_ENV`] on drop, then releases the module lock: fields drop in order.
    struct CacheHomeGuard {
        _restore: EnvRestore,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl CacheHomeGuard {
        fn trust_all(&self) {
            // SAFETY: as in `EnvRestore::drop`; the lock is held while `self` lives.
            unsafe { std::env::set_var("TOOLR_TRUST_ALL", "1") };
        }
    }

    /// Points `XDG_CACHE_HOME` and `XDG_STATE_HOME` at a fresh temp dir, so a
    /// persisting rebuild writes its lock there instead of the real cache and
    /// no trust store of the user's is read, and unsets `TOOLR_TRUST_ALL`.
    /// Bind as `(home, _guard)` so the env is restored before the dir is deleted.
    fn temp_cache_home() -> (tempfile::TempDir, CacheHomeGuard) {
        let lock = CACHE_HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let restore = EnvRestore::capture();
        let home = tempfile::TempDir::new().unwrap();
        // SAFETY: as in `EnvRestore::drop`.
        unsafe {
            std::env::set_var("XDG_CACHE_HOME", home.path());
            std::env::set_var("XDG_STATE_HOME", home.path().join("state"));
            std::env::remove_var("TOOLR_TRUST_ALL");
        }
        (
            home,
            CacheHomeGuard {
                _restore: restore,
                _lock: lock,
            },
        )
    }

    #[test]
    fn the_env_guard_puts_back_a_value_that_was_set() {
        let (home, _guard) = temp_cache_home();
        let restore = EnvRestore::capture();
        // SAFETY: `_guard` holds CACHE_HOME_LOCK.
        unsafe { std::env::remove_var("XDG_CACHE_HOME") };
        drop(restore);
        assert_eq!(
            std::env::var_os("XDG_CACHE_HOME").as_deref(),
            Some(home.path().as_os_str())
        );
    }

    /// The rebuild lock landed in the temp cache home, proving the redirect, under
    /// the name `self cache prune` keeps for this repo.
    fn assert_one_lock_in(home: &std::path::Path, repo: &std::path::Path) {
        let locks = std::fs::read_dir(home.join("toolr/locks")).unwrap();
        let names: Vec<_> = locks.map(|e| e.unwrap().path()).collect();
        let expected = toolr_core::manifest::lock_path_for_repo(&home.join("toolr"), repo);
        assert_eq!(names, [expected]);
    }

    /// A missing manifest is rebuilt and written, not reported as unreadable.
    #[test]
    fn ensure_manifest_rebuilds_a_missing_manifest() {
        let (home, _guard) = temp_cache_home();
        let tmp = tools_repo();

        let fresh = ensure_manifest_at(tmp.path()).unwrap();

        assert!(fresh.is_some());
        assert!(manifest_path(tmp.path()).is_file());
        assert_one_lock_in(home.path(), tmp.path());
    }

    /// A failed build with a manifest to fall back to warns and serves it,
    /// only a trusted fresh clone exits.
    #[test]
    fn ensure_manifest_serves_the_cache_when_a_persisting_build_fails() {
        let (_home, _guard) = temp_cache_home();
        let tmp = tools_repo();
        let path = manifest_path(tmp.path());
        std::fs::write(tmp.path().join("tools/broken.py"), "def not closed(\n").unwrap();
        let mut stale = Manifest::empty();
        stale.toolr_version = "0.0.0-stale".into();
        toolr_core::manifest::write_manifest(&path, &stale).unwrap();

        let served = ensure_manifest_at(tmp.path()).unwrap();

        assert_eq!(served.map(|b| b.manifest), Some(stale));
    }

    /// An unreadable manifest means one existed, so a trusted run whose
    /// build then fails warns instead of exiting like a fresh clone.
    #[test]
    fn ensure_manifest_warns_when_an_unreadable_manifest_fails_to_rebuild() {
        let (_home, _guard) = temp_cache_home();
        let tmp = tools_repo();
        std::fs::write(manifest_path(tmp.path()), "not json").unwrap();
        std::fs::write(tmp.path().join("tools/broken.py"), "def not closed(\n").unwrap();

        let served = ensure_manifest_at(tmp.path()).unwrap();

        assert!(served.is_none());
    }

    /// A trusted fresh clone with a venv: the build that serves the run is
    /// the one written, stamped with the venv's plugin hash, so the file is
    /// already fresh. A write that skips the hash (the old bootstrap step)
    /// leaves it stale and forces a second build.
    /// A fresh clone with an in-tree venv carrying one plugin fragment.
    fn in_tree_plugin_repo() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tools_repo();
        let tools = tmp.path().join("tools");
        std::fs::write(
            tools.join("pyproject.toml"),
            "[project]\nname = \"t\"\n\n[tool.toolr]\nvenv-location = \"in-tree\"\n",
        )
        .unwrap();
        let plugin = tools.join(".venv/lib/python3.13/site-packages/demo");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(tools.join(".venv/pyvenv.cfg"), "home = /usr\n").unwrap();
        std::fs::write(
            plugin.join("toolr-manifest.json"),
            r#"{"toolr_schema_version": 2, "package": "demo", "groups": [], "commands": []}"#,
        )
        .unwrap();
        let root = toolr_core::discovery::discover_project_root(tmp.path()).unwrap();
        (tmp, root)
    }

    #[test]
    fn ensure_manifest_writes_a_fresh_clone_once_with_the_plugin_hash() {
        let (_home, guard) = temp_cache_home();
        guard.trust_all();
        let (_tmp, root) = in_tree_plugin_repo();
        let served = ensure_manifest_at(&root).unwrap().unwrap();

        let venv = toolr_core::venv::resolve_venv_path(&root).unwrap().venv_dir;
        let written = load_manifest(&manifest_path(&root)).unwrap();
        assert_eq!(served.manifest, written);
        let plugin_hash = compute_third_party_hash(&venv).unwrap();
        assert_ne!(plugin_hash, empty_third_party_hash());
        assert_eq!(written.third_party_hash, plugin_hash);
        let verdict = compare(Some(&written), &root.join("tools"), Some(&venv)).unwrap();
        assert_eq!(verdict, FreshnessVerdict::Fresh);
    }

    /// Without a token in the trust store the venv is foreign, and its
    /// fragments are never globbed.
    #[test]
    fn ensure_manifest_skips_a_foreign_venvs_fragments() {
        let (_home, _guard) = temp_cache_home();
        let (_tmp, root) = in_tree_plugin_repo();
        let served = ensure_manifest_at(&root).unwrap().unwrap();
        assert_eq!(served.manifest.third_party_hash, empty_third_party_hash());
    }
}
