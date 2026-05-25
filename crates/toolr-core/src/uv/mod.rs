//! `uv` integration: discovery, consent-based install, and sync invocation.

use std::path::PathBuf;

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
            Self::UserRefusedInstall => {
                "uv is required for this command. Install from \
                 https://docs.astral.sh/uv/getting-started/installation/ \
                 and rerun, or set TOOLR_AUTO_INSTALL_UV=1."
                    .into()
            }
            Self::VersionTooOld { found, required } => format!(
                "uv on PATH is {}.{}.{}, but toolr requires \
                 >= {}.{}.{}. Upgrade uv and try again.",
                found.0, found.1, found.2, required.0, required.1, required.2,
            ),
            other => other.to_string(),
        }
    }
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

    /// Mutating XDG_DATA_HOME / XDG_CACHE_HOME and reading them back in
    /// the same test is racy if siblings touch the same vars. Take a
    /// per-module lock around every env-touching region.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_env_var<R>(key: &str, value: Option<&str>, f: impl FnOnce() -> R) -> R {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var_os(key);
        // SAFETY: serialised by ENV_LOCK; no other thread in this crate
        // mutates these XDG vars concurrently with this helper.
        unsafe {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
        let r = f();
        unsafe {
            match prev {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
        r
    }

    #[test]
    fn toolr_data_dir_uses_xdg_data_home_when_set() {
        let result = with_env_var("XDG_DATA_HOME", Some("/tmp/test-xdg-data"), toolr_data_dir);
        assert_eq!(result, Some(PathBuf::from("/tmp/test-xdg-data/toolr")));
    }

    #[test]
    fn toolr_cache_dir_uses_xdg_cache_home_when_set() {
        let result =
            with_env_var("XDG_CACHE_HOME", Some("/tmp/test-xdg-cache"), toolr_cache_dir);
        assert_eq!(result, Some(PathBuf::from("/tmp/test-xdg-cache/toolr")));
    }

    #[test]
    fn managed_uv_path_lives_under_data_dir_bin() {
        let result = with_env_var(
            "XDG_DATA_HOME",
            Some("/tmp/test-xdg-data"),
            managed_uv_path,
        );
        // `uv_basename()` is platform-specific; just assert the directory
        // structure so this still passes on Windows runners.
        let p = result.expect("XDG_DATA_HOME set → Some");
        assert!(p.starts_with("/tmp/test-xdg-data/toolr/bin"));
        let basename = p.file_name().unwrap().to_string_lossy().into_owned();
        assert!(basename == "uv" || basename == "uv.exe");
    }

    #[test]
    fn uv_error_display_strings_remain_descriptive() {
        // We don't want a future refactor to swap these for "error" or
        // an empty message — keep the user-facing text descriptive.
        assert!(UvError::NotAvailable.to_string().contains("uv"));
        assert!(
            UvError::VersionTooOld {
                found: (0, 1, 0),
                required: (0, 4, 0),
            }
            .to_string()
            .contains("toolr requires")
        );
        assert!(
            UvError::UnparsableVersion("garbage".into())
                .to_string()
                .contains("garbage")
        );
        assert!(
            UvError::UserRefusedInstall
                .to_string()
                .contains("declined")
        );
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "nope");
        assert!(UvError::Io(io).to_string().contains("nope"));
        assert!(
            UvError::Http("bad host".into())
                .to_string()
                .contains("bad host")
        );
        assert!(
            UvError::SyncFailed(Some(2))
                .to_string()
                .contains("uv sync failed")
        );
        assert!(UvError::SyncFailed(None).to_string().contains("uv sync failed"));
    }

    #[test]
    fn uv_error_from_io_error_conversion_works() {
        // The `#[from]` derive on the Io arm is exercised every time the
        // download/extract pipeline propagates an io::Error via `?`.
        // Belt-and-braces the conversion here so a future refactor that
        // breaks it gets caught by a unit test, not by a CI run.
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let uv: UvError = io.into();
        assert!(matches!(uv, UvError::Io(_)));
    }

    #[test]
    fn user_message_for_refused_install_names_the_install_url_and_env_var() {
        let s = UvError::UserRefusedInstall.user_message();
        assert!(s.contains("uv is required"));
        assert!(s.contains("https://docs.astral.sh/uv/"));
        assert!(s.contains("TOOLR_AUTO_INSTALL_UV"));
        // Anti-regression: must not double-prefix `toolr: ` — main.rs
        // adds that once at the CLI surface.
        assert!(!s.starts_with("toolr:"), "actual: {s}");
    }

    #[test]
    fn user_message_for_version_too_old_names_both_versions() {
        let s = UvError::VersionTooOld {
            found: (0, 1, 2),
            required: (3, 4, 5),
        }
        .user_message();
        assert!(s.contains("0.1.2"), "actual: {s}");
        assert!(s.contains("3.4.5"), "actual: {s}");
        assert!(s.contains("Upgrade uv"), "actual: {s}");
        assert!(!s.starts_with("toolr:"), "actual: {s}");
    }

    #[test]
    fn user_message_for_other_variants_falls_through_to_display() {
        // Variants without bespoke recovery hints fall back to the
        // technical `Display` string. Still no `toolr:` prefix.
        let s = UvError::NotAvailable.user_message();
        assert_eq!(s, UvError::NotAvailable.to_string());
        assert!(!s.starts_with("toolr:"), "actual: {s}");
    }
}
