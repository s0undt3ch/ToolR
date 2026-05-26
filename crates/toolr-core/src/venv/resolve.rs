//! Resolve the absolute path where the tools venv should live.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result};

use super::config::{
    ToolrConfig, VENV_LOCATION_ENV, VenvLocation, load_toolr_config, read_requires_python,
};
use super::repo_key::compute_repo_key;
use crate::uv::toolr_cache_dir;

/// Output of venv resolution.
#[derive(Debug, Clone)]
pub struct ResolvedVenv {
    /// Where the venv directory lives.
    pub venv_dir: PathBuf,
    /// `<venv>/bin/python` (or `Scripts\\python.exe` on Windows).
    pub python: PathBuf,
    /// The repo-key used in the cache layout (empty when in-tree).
    pub repo_key: String,
    /// Python version string used as a hash input (best-effort).
    pub python_version: String,
    /// Source `tools/pyproject.toml` config.
    pub config: ToolrConfig,
}

/// Resolve the tools venv path for the given repo root.
pub fn resolve_venv_path(repo_root: &Path) -> Result<ResolvedVenv> {
    let tools = repo_root.join("tools");
    let config = load_toolr_config(&tools)
        .with_context(|| format!("loading tools/pyproject.toml under {}", repo_root.display()))?;
    let python_version = config
        .python_version
        .clone()
        .or(read_requires_python(&tools).ok().flatten())
        .unwrap_or_default();

    // Env-var override of the `[tool.toolr] venv-location` setting.
    // Primary use case: CI workflows that want to opt every repo into
    // in-tree venvs without rewriting each one's `tools/pyproject.toml`.
    // An invalid spelling is a hard error so a typo doesn't silently
    // fall back to the file-configured value.
    let venv_location = match std::env::var(VENV_LOCATION_ENV) {
        Ok(raw) if !raw.is_empty() => VenvLocation::from_str(&raw).map_err(|e| {
            anyhow::anyhow!("{VENV_LOCATION_ENV}={raw:?} is invalid: {e}")
        })?,
        _ => config.venv_location,
    };

    let (venv_dir, repo_key) = match venv_location {
        VenvLocation::InTree => (tools.join(".venv"), String::new()),
        VenvLocation::Cache => {
            let key = compute_repo_key(repo_root, &python_version)?;
            let base = toolr_cache_dir()
                .ok_or_else(|| anyhow::anyhow!("could not resolve toolr cache directory"))?;
            (base.join(&key).join("venv"), key)
        }
    };

    let python = if cfg!(windows) {
        venv_dir.join("Scripts").join("python.exe")
    } else {
        venv_dir.join("bin").join("python")
    };

    Ok(ResolvedVenv {
        venv_dir,
        python,
        repo_key,
        python_version,
        config,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use tempfile::TempDir;

    /// Serialise every test in this module: they all share the
    /// process-wide environment (via `std::env::var`), and the
    /// `TOOLR_VENV_LOCATION` override is read on every invocation
    /// of `resolve_venv_path`. Running tests in parallel would let
    /// one test's `set_var` bleed into another's expectations.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn setup_repo(body: &str) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        std::fs::create_dir(&tools).unwrap();
        std::fs::write(tools.join("pyproject.toml"), body).unwrap();
        tmp
    }

    #[test]
    fn cache_default_uses_repo_key_subdir() {
        let _env = ENV_LOCK.lock().unwrap();
        let _guard = EnvVarGuard::new(VENV_LOCATION_ENV);
        unsafe { std::env::remove_var(VENV_LOCATION_ENV) };
        let tmp = setup_repo("[project]\nname=\"x\"\nversion=\"0\"\n");
        let resolved = resolve_venv_path(tmp.path()).unwrap();
        assert!(resolved.venv_dir.ends_with("venv"));
        assert!(!resolved.repo_key.is_empty());
        assert!(
            resolved
                .venv_dir
                .to_string_lossy()
                .contains(&resolved.repo_key)
        );
    }

    #[test]
    fn in_tree_lands_inside_tools_dot_venv() {
        let _env = ENV_LOCK.lock().unwrap();
        let _guard = EnvVarGuard::new(VENV_LOCATION_ENV);
        unsafe { std::env::remove_var(VENV_LOCATION_ENV) };
        let tmp = setup_repo(
            "[project]\nname=\"x\"\nversion=\"0\"\n\n[tool.toolr]\nvenv-location = \"in-tree\"\n",
        );
        let resolved = resolve_venv_path(tmp.path()).unwrap();
        assert_eq!(resolved.venv_dir, tmp.path().join("tools").join(".venv"));
        assert!(resolved.repo_key.is_empty());
    }

    /// Setting `TOOLR_VENV_LOCATION=in-tree` flips the resolution to
    /// `tools/.venv` even when `pyproject.toml` requests `cache`. The
    /// inverse direction (env says `cache`, file says `in-tree`) is
    /// also covered to prove the override is bidirectional rather
    /// than a one-way "force in-tree" flag.
    #[test]
    fn env_var_overrides_pyproject_venv_location() {
        let _env = ENV_LOCK.lock().unwrap();
        let _guard = EnvVarGuard::new(VENV_LOCATION_ENV);

        let tmp = setup_repo("[project]\nname=\"x\"\nversion=\"0\"\n");
        // SAFETY: the EnvVarGuard above ensures we restore the
        // previous value on drop.
        unsafe { std::env::set_var(VENV_LOCATION_ENV, "in-tree") };
        let resolved = resolve_venv_path(tmp.path()).unwrap();
        assert_eq!(resolved.venv_dir, tmp.path().join("tools").join(".venv"));

        let tmp_in = setup_repo(
            "[project]\nname=\"x\"\nversion=\"0\"\n\n[tool.toolr]\nvenv-location = \"in-tree\"\n",
        );
        unsafe { std::env::set_var(VENV_LOCATION_ENV, "cache") };
        let resolved_cache = resolve_venv_path(tmp_in.path()).unwrap();
        // Cache mode lands under the toolr cache dir, not in-tree.
        assert!(!resolved_cache.venv_dir.starts_with(tmp_in.path()));
        assert!(!resolved_cache.repo_key.is_empty());
    }

    #[test]
    fn env_var_invalid_value_is_an_error() {
        let _env = ENV_LOCK.lock().unwrap();
        let _guard = EnvVarGuard::new(VENV_LOCATION_ENV);
        let tmp = setup_repo("[project]\nname=\"x\"\nversion=\"0\"\n");
        unsafe { std::env::set_var(VENV_LOCATION_ENV, "bogus") };
        let err = resolve_venv_path(tmp.path()).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("TOOLR_VENV_LOCATION"), "got: {msg}");
        assert!(msg.contains("bogus"), "got: {msg}");
    }

    /// RAII helper that restores (or removes) the original env var
    /// value on drop. Necessary because Rust tests run with a shared
    /// process env and setting `TOOLR_VENV_LOCATION` from one test
    /// would otherwise bleed into the next.
    struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn new(key: &'static str) -> Self {
            Self {
                key,
                previous: std::env::var(key).ok(),
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => unsafe { std::env::set_var(self.key, v) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }
}
