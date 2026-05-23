//! Per-cache-entry `meta.json` sidecar.
//!
//! Layout (alongside the venv that `toolr_core::venv` manages):
//!
//! ```text
//! $XDG_CACHE_HOME/toolr/<repo-key>/
//!     venv/         (managed by `toolr_core::venv`)
//!     meta.json     (this module)
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current sidecar schema version. Bump on breaking format changes;
/// `Meta::load` rejects newer versions and silently upgrades older ones
/// in-process if migrations are added.
pub const SCHEMA_VERSION: u32 = 1;

/// Filename used for the sidecar inside the per-repo cache directory.
pub const FILE_NAME: &str = "meta.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    /// Schema version. Defaults to 1 for files written by older toolr.
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// Absolute, symlink-resolved repo path that owns this venv.
    pub repo_path: PathBuf,
    /// Toolr binary version that created this entry.
    pub toolr_version: String,
    /// Concrete Python version used (e.g. "3.13.1").
    pub python_version: String,
    /// When this cache entry was first materialised.
    pub created_at: DateTime<Utc>,
    /// Updated on every toolr invocation against this cache entry.
    pub last_used_at: DateTime<Utc>,
}

fn default_schema_version() -> u32 {
    1
}

#[derive(Debug, Error)]
pub enum MetaError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unknown meta schema_version {0}; this toolr supports up to {max}", max = SCHEMA_VERSION)]
    UnknownSchemaVersion(u32),
}

impl Meta {
    /// Build a fresh `Meta`. `created_at` and `last_used_at` are set to
    /// the same instant.
    pub fn new(
        repo_path: impl Into<PathBuf>,
        toolr_version: impl Into<String>,
        python_version: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            schema_version: SCHEMA_VERSION,
            repo_path: repo_path.into(),
            toolr_version: toolr_version.into(),
            python_version: python_version.into(),
            created_at: now,
            last_used_at: now,
        }
    }

    /// Path of the sidecar file inside `cache_dir`.
    pub fn path_in(cache_dir: &Path) -> PathBuf {
        cache_dir.join(FILE_NAME)
    }

    /// Load `meta.json` from `cache_dir`.
    pub fn load(cache_dir: &Path) -> Result<Self, MetaError> {
        let path = Self::path_in(cache_dir);
        let bytes = fs::read(&path)?;
        let raw: serde_json::Value = serde_json::from_slice(&bytes)?;
        let version = raw
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u32;
        if version > SCHEMA_VERSION {
            return Err(MetaError::UnknownSchemaVersion(version));
        }
        let meta: Meta = serde_json::from_value(raw)?;
        Ok(meta)
    }

    /// Atomically write `meta.json` into `cache_dir`. The directory is
    /// created if missing.
    pub fn write(&self, cache_dir: &Path) -> Result<(), MetaError> {
        fs::create_dir_all(cache_dir)?;
        let final_path = Self::path_in(cache_dir);
        let tmp_path = cache_dir.join(".meta.json.tmp");
        let bytes = serde_json::to_vec_pretty(self)?;
        fs::write(&tmp_path, bytes)?;
        fs::rename(&tmp_path, &final_path)?;
        Ok(())
    }
}
