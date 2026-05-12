//! Decision logic + executor for installing a toolr-managed uv.

use std::fs;
use std::io::{IsTerminal, Write};
use std::path::Path;

use super::{UvBinary, UvError, UvSource, managed_uv_path, probe};

/// What the discovery + consent flow tells the caller to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallDecision {
    /// uv was found and is usable as-is. No install required.
    AlreadyAvailable,
    /// uv is not available and the caller wants us to install it.
    Install,
    /// uv is not available and the user declined (or stdin is not a TTY
    /// and no auto-yes was set). Caller should surface a friendly error.
    Refuse,
}

/// How the toolr binary was invoked, for non-interactive decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConsentMode {
    /// `--yes` was passed.
    pub yes_flag: bool,
    /// `TOOLR_AUTO_INSTALL_UV=1` is set in the environment.
    pub auto_install_env: bool,
}

impl ConsentMode {
    pub fn from_env() -> Self {
        Self {
            yes_flag: false,
            auto_install_env: std::env::var_os("TOOLR_AUTO_INSTALL_UV")
                .is_some_and(|v| v == "1"),
        }
    }
}

/// Pure decision logic. `path_found` is whether `find_uv_on_path` returned
/// `Some`. `managed_found` is whether `find_managed_uv` returned `Some`.
/// `stdin_tty` controls whether to prompt; in tests / pipes we never
/// prompt and rely on `consent` instead.
pub fn decide_install(
    path_found: bool,
    managed_found: bool,
    consent: ConsentMode,
    stdin_tty: bool,
) -> InstallDecision {
    if path_found || managed_found {
        return InstallDecision::AlreadyAvailable;
    }
    if consent.yes_flag || consent.auto_install_env {
        return InstallDecision::Install;
    }
    if !stdin_tty {
        return InstallDecision::Refuse;
    }
    match prompt_for_consent() {
        Ok(true) => InstallDecision::Install,
        _ => InstallDecision::Refuse,
    }
}

/// Print the prompt and return `true` for "install".
fn prompt_for_consent() -> Result<bool, UvError> {
    let mut stderr = std::io::stderr();
    writeln!(
        stderr,
        "toolr needs uv (https://docs.astral.sh/uv/) and didn't find it on PATH."
    )?;
    let managed = super::managed_uv_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "$XDG_DATA_HOME/toolr/bin/uv".to_string());
    writeln!(stderr, "  [I] Install it for me at {managed}")?;
    writeln!(
        stderr,
        "  [M] I'll install it manually (see https://docs.astral.sh/uv/getting-started/installation/)"
    )?;
    write!(stderr, "Choice [I/M] ")?;
    stderr.flush()?;
    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf)?;
    Ok(matches!(buf.trim(), "I" | "i" | ""))
}

/// Convenience that combines a TTY check + decision.
pub fn decide_install_auto(
    path_found: bool,
    managed_found: bool,
    consent: ConsentMode,
) -> InstallDecision {
    decide_install(
        path_found,
        managed_found,
        consent,
        std::io::stdin().is_terminal(),
    )
}

/// Host triple selection. Returns an Astral release asset name without the
/// extension, plus the extension itself.
///
/// Reference: <https://github.com/astral-sh/uv/releases>
pub fn host_asset() -> Option<(&'static str, &'static str)> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some(("uv-x86_64-unknown-linux-gnu", "tar.gz")),
        ("linux", "aarch64") => Some(("uv-aarch64-unknown-linux-gnu", "tar.gz")),
        ("macos", "x86_64") => Some(("uv-x86_64-apple-darwin", "tar.gz")),
        ("macos", "aarch64") => Some(("uv-aarch64-apple-darwin", "tar.gz")),
        ("windows", "x86_64") => Some(("uv-x86_64-pc-windows-msvc", "zip")),
        _ => None,
    }
}

/// Where to download from. Parametrised so tests can override.
pub fn asset_url(asset: &str, ext: &str) -> String {
    format!("https://github.com/astral-sh/uv/releases/latest/download/{asset}.{ext}")
}

/// Download + extract uv into the managed location.
pub fn perform_install() -> Result<UvBinary, UvError> {
    let dest = managed_uv_path().ok_or_else(|| {
        UvError::Http("could not resolve $XDG_DATA_HOME/toolr/bin".into())
    })?;
    let (asset, ext) = host_asset().ok_or_else(|| {
        UvError::Http(format!(
            "no prebuilt uv asset for {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
    })?;
    let url = asset_url(asset, ext);
    download_and_extract(&url, asset, ext, &dest)?;
    probe(&dest, UvSource::FreshlyInstalled)
}

fn download_and_extract(
    url: &str,
    asset: &str,
    ext: &str,
    dest: &Path,
) -> Result<(), UvError> {
    let parent = dest.parent().ok_or_else(|| {
        UvError::Http("managed uv path has no parent directory".into())
    })?;
    fs::create_dir_all(parent)?;

    let response = reqwest::blocking::get(url)
        .map_err(|e| UvError::Http(e.to_string()))?
        .error_for_status()
        .map_err(|e| UvError::Http(e.to_string()))?;
    let bytes = response
        .bytes()
        .map_err(|e| UvError::Http(e.to_string()))?;

    let archive_name = format!("{asset}.{ext}");
    let tmp = tempfile::tempdir()?;
    let archive_path = tmp.path().join(&archive_name);
    fs::write(&archive_path, &bytes)?;

    match ext {
        "tar.gz" => extract_tar_gz(&archive_path, tmp.path())?,
        "zip" => extract_zip(&archive_path, tmp.path())?,
        other => {
            return Err(UvError::Http(format!("unknown archive extension: {other}")));
        }
    }

    // The archive contains a single `uv` binary at the top of a
    // directory matching the asset name.
    let binary_name = if cfg!(windows) { "uv.exe" } else { "uv" };
    let candidates = [
        tmp.path().join(asset).join(binary_name),
        tmp.path().join(binary_name),
    ];
    let src = candidates.iter().find(|p| p.is_file()).ok_or_else(|| {
        UvError::Http(format!(
            "extracted archive did not contain a {binary_name} binary"
        ))
    })?;

    fs::copy(src, dest)?;
    set_executable(dest)?;
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), UvError> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), UvError> {
    Ok(())
}

fn extract_tar_gz(archive: &Path, into: &Path) -> Result<(), UvError> {
    let status = std::process::Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .current_dir(into)
        .status()?;
    if !status.success() {
        return Err(UvError::Http(format!("tar exited with status {status:?}")));
    }
    Ok(())
}

fn extract_zip(archive: &Path, into: &Path) -> Result<(), UvError> {
    let status = std::process::Command::new("unzip")
        .arg("-q")
        .arg(archive)
        .arg("-d")
        .arg(into)
        .status()?;
    if !status.success() {
        return Err(UvError::Http(format!(
            "unzip exited with status {status:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn already_available_when_path_present() {
        assert_eq!(
            decide_install(true, false, ConsentMode::default(), false),
            InstallDecision::AlreadyAvailable
        );
    }

    #[test]
    fn already_available_when_managed_present() {
        assert_eq!(
            decide_install(false, true, ConsentMode::default(), false),
            InstallDecision::AlreadyAvailable
        );
    }

    #[test]
    fn yes_flag_forces_install_even_without_tty() {
        let consent = ConsentMode { yes_flag: true, ..Default::default() };
        assert_eq!(
            decide_install(false, false, consent, false),
            InstallDecision::Install
        );
    }

    #[test]
    fn auto_install_env_forces_install_even_without_tty() {
        let consent = ConsentMode { auto_install_env: true, ..Default::default() };
        assert_eq!(
            decide_install(false, false, consent, false),
            InstallDecision::Install
        );
    }

    #[test]
    fn refuses_non_interactive_without_consent() {
        assert_eq!(
            decide_install(false, false, ConsentMode::default(), false),
            InstallDecision::Refuse
        );
    }
}
