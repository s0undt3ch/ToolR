//! Find `<site-packages>/*/toolr-manifest.json` in the tools venv.
//!
//! The site-packages directories come from `venv::site_packages_dirs`,
//! which checks both venv layouts on every platform:
//!
//! - Unix: `<venv>/lib/python3.x/site-packages/<pkg>/toolr-manifest.json`
//! - Windows: `<venv>/Lib/site-packages/<pkg>/toolr-manifest.json`
//!
//! Both are listed directory by directory, with no glob pattern, so a
//! `[`, `]`, `*` or `?` in the venv path needs no escaping.

use std::fs;
use std::path::{Path, PathBuf};

use super::parse::ThirdPartyError;
use crate::venv::site_packages_dirs;

/// Find all third-party manifest fragments under `tools_venv`.
///
/// Returns paths in deterministic (sorted) order for reproducibility.
/// Errors only when an existing site-packages directory can't be listed;
/// individual path parsing happens later in `parse_fragment`.
pub fn glob_manifests(tools_venv: &Path) -> Result<Vec<PathBuf>, ThirdPartyError> {
    let mut out = Vec::new();
    for site_packages in site_packages_dirs(tools_venv) {
        let entries = fs::read_dir(&site_packages).map_err(|source| ThirdPartyError::Io {
            path: site_packages.clone(),
            source,
        })?;
        out.extend(
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path().join("toolr-manifest.json"))
                .filter(|path| path.is_file()),
        );
    }
    out.sort();
    out.dedup();
    Ok(out)
}
