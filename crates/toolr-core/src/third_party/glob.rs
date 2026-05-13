//! Glob `<tools-venv>/lib/python*/site-packages/*/toolr-manifest.json`.

use std::path::{Path, PathBuf};

use glob::{MatchOptions, glob_with};

use super::parse::ThirdPartyError;

/// Glob all third-party manifest fragments under `tools_venv`.
///
/// Returns paths in deterministic (sorted) order for reproducibility.
/// Errors only on filesystem-level glob failures; individual path
/// parsing happens later in `parse_fragment`.
pub fn glob_manifests(tools_venv: &Path) -> Result<Vec<PathBuf>, ThirdPartyError> {
    let pattern = tools_venv
        .join("lib")
        .join("python*")
        .join("site-packages")
        .join("*")
        .join("toolr-manifest.json");
    let pattern_str = pattern
        .to_str()
        .ok_or_else(|| ThirdPartyError::NonUtf8Path(pattern.clone()))?;

    let opts = MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: false,
    };

    let mut out = Vec::new();
    for entry in glob_with(pattern_str, opts).map_err(ThirdPartyError::Pattern)? {
        match entry {
            Ok(path) => out.push(path),
            Err(e) => return Err(ThirdPartyError::Glob(e)),
        }
    }
    out.sort();
    Ok(out)
}
