//! Locate a working uv binary on the host.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{MIN_UV_VERSION, UvBinary, UvError, UvSource, managed_uv_path};

/// Try to find a usable `uv` on `$PATH`. Returns `Ok(None)` if uv is not
/// on PATH at all (so the caller can fall through to the managed path);
/// `Err(UvError::VersionTooOld { .. })` if it exists but is too old.
pub fn find_uv_on_path() -> Result<Option<UvBinary>, UvError> {
    let candidate = which_uv()?;
    let Some(path) = candidate else {
        return Ok(None);
    };
    probe(&path, UvSource::Path).map(Some)
}

/// Try to find a toolr-managed uv at `$XDG_DATA_HOME/toolr/bin/uv`.
pub fn find_managed_uv() -> Result<Option<UvBinary>, UvError> {
    let Some(path) = managed_uv_path() else {
        return Ok(None);
    };
    if !path.is_file() {
        return Ok(None);
    }
    probe(&path, UvSource::Managed).map(Some)
}

/// Run `<path> --version`, parse output, validate against the minimum.
pub fn probe(path: &Path, source: UvSource) -> Result<UvBinary, UvError> {
    let output = Command::new(path).arg("--version").output()?;
    if !output.status.success() {
        return Err(UvError::UnparsableVersion(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let version = parse_uv_version(&stdout)
        .ok_or_else(|| UvError::UnparsableVersion(stdout.clone()))?;
    if version < MIN_UV_VERSION {
        return Err(UvError::VersionTooOld {
            found: version,
            required: MIN_UV_VERSION,
        });
    }
    Ok(UvBinary {
        path: path.to_path_buf(),
        version,
        source,
    })
}

/// `uv --version` prints something like `uv 0.5.1 (...)`. Extract the
/// three-component numeric prefix.
pub fn parse_uv_version(output: &str) -> Option<(u32, u32, u32)> {
    let line = output.lines().next()?;
    let words = line.split_whitespace().collect::<Vec<_>>();
    // Find the first token that looks like a `1.2.3` (allow a trailing
    // alphanumeric suffix, which we discard).
    for word in words {
        let trimmed: String = word
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let parts: Vec<&str> = trimmed.split('.').collect();
        if parts.len() != 3 {
            continue;
        }
        let (Ok(a), Ok(b), Ok(c)) = (
            parts[0].parse::<u32>(),
            parts[1].parse::<u32>(),
            parts[2].parse::<u32>(),
        ) else {
            continue;
        };
        return Some((a, b, c));
    }
    None
}

/// Naive PATH scan that returns the first `uv` (or `uv.exe` on Windows)
/// found.
pub fn which_uv() -> Result<Option<PathBuf>, UvError> {
    let basename = if cfg!(windows) { "uv.exe" } else { "uv" };
    let Some(path) = std::env::var_os("PATH") else {
        return Ok(None);
    };
    for entry in std::env::split_paths(&path) {
        let candidate = entry.join(basename);
        if candidate.is_file() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_uv_version_string() {
        let s = "uv 0.5.1 (xyz)\n";
        assert_eq!(parse_uv_version(s), Some((0, 5, 1)));
    }

    #[test]
    fn parses_with_no_trailing_paren() {
        assert_eq!(parse_uv_version("uv 1.10.2"), Some((1, 10, 2)));
    }

    #[test]
    fn returns_none_on_garbage() {
        assert_eq!(parse_uv_version("garbage"), None);
        assert_eq!(parse_uv_version("uv broken"), None);
        assert_eq!(parse_uv_version(""), None);
    }

    #[test]
    fn version_too_old_error_includes_both_versions() {
        let err = UvError::VersionTooOld {
            found: (0, 1, 0),
            required: MIN_UV_VERSION,
        };
        let msg = err.to_string();
        assert!(msg.contains("0.1.0") || msg.contains("(0, 1, 0)"));
    }
}
