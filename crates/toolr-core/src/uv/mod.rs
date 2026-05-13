//! `uv` integration: discovery, consent-based install, and sync invocation.

use std::path::{Path, PathBuf};

use thiserror::Error;

pub mod discover;
pub use discover::{find_managed_uv, find_uv_on_path, parse_uv_version, probe, which_uv};

pub mod install;
pub use install::{ConsentMode, InstallDecision, decide_install, decide_install_auto};

/// Minimum supported uv version. Bumped when toolr starts to rely on a
/// uv feature only available in a newer release.
pub const MIN_UV_VERSION: (u32, u32, u32) = (0, 4, 0);

/// A resolved uv binary location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UvBinary {
    pub path: PathBuf,
    /// Parsed `uv --version` output, `(major, minor, patch)`.
    pub version: (u32, u32, u32),
    /// Where this binary was found.
    pub source: UvSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UvSource {
    /// Found on `$PATH`.
    Path,
    /// Found at `$XDG_DATA_HOME/toolr/bin/uv` (toolr-managed).
    Managed,
    /// Just installed by toolr this run.
    FreshlyInstalled,
}

#[derive(Debug, Error)]
pub enum UvError {
    #[error("uv is required but not available; install it from https://docs.astral.sh/uv/")]
    NotAvailable,
    #[error("uv on PATH reported version {found:?} but toolr requires >= {required:?}")]
    VersionTooOld {
        found: (u32, u32, u32),
        required: (u32, u32, u32),
    },
    #[error("`uv --version` produced unparsable output: {0:?}")]
    UnparsableVersion(String),
    #[error("user declined uv install; commands that require uv cannot run")]
    UserRefusedInstall,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP error during uv install: {0}")]
    Http(String),
    #[error("uv sync failed with exit code {0:?}")]
    SyncFailed(Option<i32>),
}

/// Where toolr keeps its private state (binaries, etc).
/// Defaults to `$XDG_DATA_HOME/toolr`, falling back to
/// `~/.local/share/toolr` if `XDG_DATA_HOME` is unset.
pub fn toolr_data_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(|v| PathBuf::from(v).join("toolr"))
        .or_else(|| dirs::data_dir().map(|d| d.join("toolr")))
}

/// Where toolr keeps cached venvs and other transient files.
/// Defaults to `$XDG_CACHE_HOME/toolr`, falling back to
/// `~/.cache/toolr`.
pub fn toolr_cache_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .map(|v| PathBuf::from(v).join("toolr"))
        .or_else(|| dirs::cache_dir().map(|d| d.join("toolr")))
}

/// The path where toolr installs a managed uv if the user consents.
pub fn managed_uv_path() -> Option<PathBuf> {
    toolr_data_dir().map(|d| d.join("bin").join(uv_basename()))
}

fn uv_basename() -> &'static str {
    if cfg!(windows) { "uv.exe" } else { "uv" }
}

/// Optional binary-resolution helper to be exposed in later tasks.
pub fn _placeholder(_path: &Path) -> Option<UvBinary> {
    None
}

use install::perform_install;

/// Find or install a working uv binary. The single entrypoint other
/// modules call when they need uv.
pub fn ensure_uv(consent: ConsentMode) -> Result<UvBinary, UvError> {
    if let Some(uv) = find_uv_on_path()? {
        return Ok(uv);
    }
    if let Some(uv) = find_managed_uv()? {
        return Ok(uv);
    }
    match decide_install_auto(false, false, consent) {
        InstallDecision::AlreadyAvailable => {
            // Shouldn't happen given the checks above, but if it does,
            // try one more time.
            find_uv_on_path()?
                .or(find_managed_uv()?)
                .ok_or(UvError::NotAvailable)
        }
        InstallDecision::Install => perform_install(),
        InstallDecision::Refuse => Err(UvError::UserRefusedInstall),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_uv_version_is_a_real_tuple() {
        let (maj, _, _) = MIN_UV_VERSION;
        assert!(maj < 100, "min uv major version should be plausible");
    }

    #[test]
    fn data_dir_resolves_or_returns_none_on_exotic_envs() {
        // We don't assert a specific path: this just exercises the call.
        let _ = toolr_data_dir();
        let _ = toolr_cache_dir();
        let _ = managed_uv_path();
    }
}
