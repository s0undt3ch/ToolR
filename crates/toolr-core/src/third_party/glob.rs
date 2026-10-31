//! Glob `<tools-venv>/{lib/python*,Lib}/site-packages/*/toolr-manifest.json`.
//!
//! uv venvs use different layouts per platform:
//!
//! - Unix: `<venv>/lib/python3.x/site-packages/<pkg>/toolr-manifest.json`
//! - Windows: `<venv>/Lib/site-packages/<pkg>/toolr-manifest.json`
//!   (no `python*` subdir; case-sensitive `Lib` is canonical even on
//!   NTFS).
//!
//! We try both patterns and concatenate the results so the same code
//! path works on every supported platform.
//!
//! The venv path is escaped before the wildcard tail is appended, so a
//! `[`, `]`, `*` or `?` in it matches literally.

use std::path::{Path, PathBuf};

use glob::{MatchOptions, glob_with};

use super::parse::ThirdPartyError;
use crate::glob_path::escape_path;

/// Glob all third-party manifest fragments under `tools_venv`.
///
/// Returns paths in deterministic (sorted) order for reproducibility.
/// Errors only on filesystem-level glob failures; individual path
/// parsing happens later in `parse_fragment`.
pub fn glob_manifests(tools_venv: &Path) -> Result<Vec<PathBuf>, ThirdPartyError> {
    let opts = MatchOptions {
        // Escaped venv components are matched, not joined, so a case-insensitive
        // match would pull in a sibling venv like `[WIP]` next to `[wip]`.
        // Case-insensitive filesystems still resolve the literal `Lib` either way.
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: false,
    };

    let prefix = escape_path(tools_venv)
        .ok_or_else(|| ThirdPartyError::NonUtf8Path(tools_venv.to_path_buf()))?;

    let mut out = Vec::new();
    for layout_segments in candidate_layouts() {
        let pattern = format!(
            "{prefix}/{}/site-packages/*/toolr-manifest.json",
            layout_segments.join("/")
        );
        for entry in glob_with(&pattern, opts).map_err(ThirdPartyError::Pattern)? {
            match entry {
                Ok(path) => out.push(path),
                Err(e) => return Err(ThirdPartyError::Glob(e)),
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// Per-platform candidate venv subdirectory layouts. We try both forms
/// on every platform because cross-platform venvs (e.g., a venv created
/// on one OS, inspected on another via a mounted filesystem in CI) do
/// occur and the cost of a second glob is trivial.
fn candidate_layouts() -> &'static [&'static [&'static str]] {
    // Each inner slice is the path between `<venv>` and `site-packages`.
    &[&["lib", "python*"], &["Lib"]]
}
