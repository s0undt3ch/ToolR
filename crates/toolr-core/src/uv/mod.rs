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
    /// toolr won't use or remove an in-tree venv it didn't build: the message says why and
    /// how to approve replacing it. Complete on its own, so callers add no context to it.
    #[error("{0}")]
    UntrustedVenv(String),
    /// `Command::output()` failed even though the candidate binary was
    /// on disk and looked executable. Almost always a libc / dynamic-
    /// loader mismatch (a glibc-linked uv on a musl host, or vice
    /// versa). Distinct from `Io` so `user_message` can render an
    /// actionable hint instead of the bare `os error 2`.
    #[error("failed to execute uv at {path}: {source}")]
    ExecFailed {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("HTTP error during uv install: {0}")]
    Http(String),
    /// The install failed for a reason other than the network: no uv for this
    /// host, nowhere to put it, or a download that unpacks but lacks the binary.
    #[error("couldn't install uv: {0}")]
    Install(String),
    /// The archive couldn't be decoded or unpacked.
    #[error("couldn't extract the downloaded uv archive {archive}")]
    Extract {
        archive: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// The download couldn't be checked: the release tag didn't resolve, its
    /// checksum file was missing or malformed, or a file exceeded its size cap.
    #[error("couldn't verify the uv download {asset}: {reason}")]
    Unverified { asset: String, reason: String },
    #[error(
        "checksum mismatch for uv {asset} from release {tag}: expected SHA-256 {expected}, got {actual}"
    )]
    ChecksumMismatch {
        asset: String,
        tag: String,
        expected: String,
        actual: String,
    },
    #[error("uv sync failed with exit code {0:?}")]
    SyncFailed(Option<i32>),
}

impl UvError {
    /// Construct an [`UvError::ExecFailed`] carrying the path that
    /// failed to exec. Tiny helper so call sites don't repeat the
    /// `path.to_path_buf()` boilerplate.
    pub(crate) fn exec_failed(path: &Path, source: std::io::Error) -> Self {
        Self::ExecFailed {
            path: path.to_path_buf(),
            source,
        }
    }

    pub(crate) fn extract(
        archive: &Path,
        source: impl Into<Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        Self::Extract {
            archive: archive.to_path_buf(),
            source: source.into(),
        }
    }
}

impl UvError {
    /// Render this error as a user-facing message with recovery hints.
    ///
    /// `Display` gives a technical one-liner suitable for log lines and
    /// anyhow chains; `user_message` is the version meant for stderr at
    /// the CLI surface, with concrete next steps the user can act on.
    /// Callers should prefer this when they know they're producing the
    /// final user-visible error (typically wrapped in `anyhow::anyhow!`
    /// so `main`'s `"toolr: {e:#}"` prefix lands once and only once).
    pub fn user_message(&self) -> String {
        match self {
            Self::UserRefusedInstall => "uv is required for this command. Install from \
                 https://docs.astral.sh/uv/getting-started/installation/ \
                 and rerun, or set TOOLR_AUTO_SYNC=1."
                .into(),
            Self::VersionTooOld { found, required } => format!(
                "uv on PATH is {}.{}.{}, but toolr requires \
                 >= {}.{}.{}. Upgrade uv and try again.",
                found.0, found.1, found.2, required.0, required.1, required.2,
            ),
            // ENOENT from `execve` on a binary that demonstrably exists
            // on disk almost always means the dynamic loader the binary
            // needs is missing — typically a glibc-linked uv on a musl
            // host (or vice versa). Spell that out instead of leaving
            // the user with a bare "No such file or directory". The
            // path + io::Error live in the wrapped `ExecFailed` Display
            // and reach the user via the anyhow chain (see
            // [`Self::into_anyhow`]); the hint here is purely the
            // recovery advice.
            Self::ExecFailed { source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
                "uv exists on disk but couldn't be executed. This \
                 usually means the binary's dynamic loader is missing \
                 — for example a glibc-linked uv on a musl host, or \
                 vice versa. Set TOOLR_UV_LIBC=musl (or =gnu) to \
                 override toolr's libc detection, or install uv via \
                 your OS package manager."
                    .into()
            }
            Self::Extract { .. } => "toolr couldn't unpack the uv it downloaded. Install uv \
                 yourself from https://docs.astral.sh/uv/getting-started/installation/ \
                 and rerun."
                .into(),
            Self::Http(_) => "toolr couldn't download uv. Install uv yourself from \
                 https://docs.astral.sh/uv/getting-started/installation/ and rerun."
                .into(),
            Self::Install(_) => "toolr couldn't install the uv it downloaded. Install uv \
                 yourself from https://docs.astral.sh/uv/getting-started/installation/ \
                 and rerun."
                .into(),
            Self::Unverified { .. } | Self::ChecksumMismatch { .. } => {
                "toolr didn't install uv because it couldn't verify the download. \
                 Install uv yourself from \
                 https://docs.astral.sh/uv/getting-started/installation/ and rerun."
                    .into()
            }
            other => other.to_string(),
        }
    }

    /// Convert into an `anyhow::Error` while preserving the underlying
    /// chain. When [`Self::user_message`] adds recovery advice on top
    /// of the technical Display string, attach it as a context layer
    /// so it surfaces first; otherwise just wrap the error so callers
    /// don't double-print the same line.
    ///
    /// Use this at the toolr-binary boundary in place of
    /// `anyhow::anyhow!(e.user_message())` — that pattern collapses
    /// the entire chain and obscures which io::Error / which path was
    /// the actual cause.
    pub fn into_anyhow(self) -> anyhow::Error {
        let hint = self.user_message();
        let display = self.to_string();
        let err = anyhow::Error::new(self);
        if hint == display {
            err
        } else {
            err.context(hint)
        }
    }
}

/// Where toolr keeps its private state (binaries, etc).
/// Defaults to `$XDG_DATA_HOME/toolr`, falling back to
/// `~/.local/share/toolr` if `XDG_DATA_HOME` is unset, empty or relative.
pub(crate) fn toolr_data_dir() -> Option<PathBuf> {
    crate::env::xdg_dir_env("XDG_DATA_HOME")
        .map(|v| v.join("toolr"))
        .or_else(|| dirs::data_dir().map(|d| d.join("toolr")))
}

/// Where toolr keeps state that must survive a cache wipe, such as repository
/// trust. Defaults to `$XDG_STATE_HOME/toolr` unless `XDG_STATE_HOME` is unset,
/// empty or relative, then the platform state directory, then `toolr_data_dir`
/// (`dirs::state_dir()` is `None` on macOS and Windows).
pub fn toolr_state_dir() -> Option<PathBuf> {
    crate::env::xdg_dir_env("XDG_STATE_HOME")
        .map(|v| v.join("toolr"))
        .or_else(|| dirs::state_dir().map(|d| d.join("toolr")))
        .or_else(toolr_data_dir)
}

/// Where toolr keeps cached venvs and other transient files.
/// Defaults to `$XDG_CACHE_HOME/toolr`, falling back to
/// `~/.cache/toolr` if `XDG_CACHE_HOME` is unset, empty or relative.
pub fn toolr_cache_dir() -> Option<PathBuf> {
    crate::env::xdg_dir_env("XDG_CACHE_HOME")
        .map(|v| v.join("toolr"))
        .or_else(|| dirs::cache_dir().map(|d| d.join("toolr")))
}

/// The path where toolr installs a managed uv if the user consents.
pub fn managed_uv_path() -> Option<PathBuf> {
    toolr_data_dir().map(|d| d.join("bin").join(uv_basename()))
}

fn uv_basename() -> &'static str {
    if cfg!(windows) { "uv.exe" } else { "uv" }
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
mod tests;
