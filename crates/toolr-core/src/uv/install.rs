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

/// Linux libc variant. Picked at runtime via [`detect_linux_libc`] so
/// the auto-installer fetches a uv binary whose dynamic loader exists
/// on the running host — a glibc binary on a musl box `execve`s as
/// `ENOENT` (missing `/lib64/ld-linux-x86-64.so.2`), surfacing as the
/// notoriously unhelpful "I/O error: No such file or directory".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxLibc {
    Gnu,
    Musl,
}

/// Host triple selection. Returns an Astral release asset name without the
/// extension, plus the extension itself.
///
/// Reference: <https://github.com/astral-sh/uv/releases>
pub fn host_asset() -> Option<(&'static str, &'static str)> {
    host_asset_for(
        std::env::consts::OS,
        std::env::consts::ARCH,
        detect_linux_libc(),
    )
}

/// Inner table used by [`host_asset`]. Split out so tests can pin
/// `(os, arch, libc)` and exercise every row deterministically without
/// touching env vars, `/etc`, or running `ldd`.
pub(crate) fn host_asset_for(
    os: &str,
    arch: &str,
    linux_libc: LinuxLibc,
) -> Option<(&'static str, &'static str)> {
    match (os, arch) {
        ("linux", "x86_64") => Some((
            match linux_libc {
                LinuxLibc::Gnu => "uv-x86_64-unknown-linux-gnu",
                LinuxLibc::Musl => "uv-x86_64-unknown-linux-musl",
            },
            "tar.gz",
        )),
        ("linux", "aarch64") => Some((
            match linux_libc {
                LinuxLibc::Gnu => "uv-aarch64-unknown-linux-gnu",
                LinuxLibc::Musl => "uv-aarch64-unknown-linux-musl",
            },
            "tar.gz",
        )),
        ("macos", "x86_64") => Some(("uv-x86_64-apple-darwin", "tar.gz")),
        ("macos", "aarch64") => Some(("uv-aarch64-apple-darwin", "tar.gz")),
        ("windows", "x86_64") => Some(("uv-x86_64-pc-windows-msvc", "zip")),
        _ => None,
    }
}

/// Detect the Linux libc of the running host.
///
/// Order of precedence:
/// 1. `TOOLR_UV_LIBC=musl|gnu` env override — escape hatch for hosts
///    where automatic detection guesses wrong (custom distros, chroots,
///    forced cross-libc setups).
/// 2. `/etc/alpine-release` exists → musl. Cheap and deterministic;
///    matches the `setup-toolr` GitHub Action's own detection.
/// 3. `ldd --version` mentions `musl` → musl. Catches non-Alpine musl
///    distros (Void Linux musl edition, Chimera, distroless-musl).
/// 4. Otherwise → gnu (the historical default).
///
/// Non-Linux targets always report [`LinuxLibc::Gnu`]; the value is
/// unused there because [`host_asset_for`]'s `"linux"` arms never fire
/// on non-Linux builds.
pub fn detect_linux_libc() -> LinuxLibc {
    detect_linux_libc_from(
        std::env::var_os("TOOLR_UV_LIBC")
            .and_then(|v| v.into_string().ok())
            .as_deref(),
        || Path::new("/etc/alpine-release").exists(),
        ldd_reports_musl,
    )
}

/// Pure decision logic for [`detect_linux_libc`]. Each side-effect is
/// injected as a closure so tests can run the table without touching
/// env vars, the filesystem, or spawning `ldd`.
fn detect_linux_libc_from(
    env_override: Option<&str>,
    alpine_release_exists: impl FnOnce() -> bool,
    ldd_reports_musl: impl FnOnce() -> bool,
) -> LinuxLibc {
    if let Some(value) = env_override {
        match value.trim() {
            "musl" => return LinuxLibc::Musl,
            "gnu" => return LinuxLibc::Gnu,
            // Anything else (empty, typo, unknown) falls through to
            // auto-detection rather than getting silently coerced.
            _ => {}
        }
    }
    if alpine_release_exists() {
        return LinuxLibc::Musl;
    }
    if ldd_reports_musl() {
        return LinuxLibc::Musl;
    }
    LinuxLibc::Gnu
}

/// Spawn `ldd --version` and return `true` if either stream mentions
/// `musl`. glibc's ldd writes to stdout, musl's ldd writes to stderr
/// (and exits non-zero), so we concatenate both before searching.
fn ldd_reports_musl() -> bool {
    let Ok(output) = std::process::Command::new("ldd").arg("--version").output() else {
        return false;
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    stdout.to_lowercase().contains("musl") || stderr.to_lowercase().contains("musl")
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
    use std::process::Command;

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

    #[test]
    fn decide_install_auto_smoke() {
        // Path-found short-circuits before reading `is_terminal`, so this is
        // stable regardless of how the test process's stdin is wired.
        assert_eq!(
            decide_install_auto(true, false, ConsentMode::default()),
            InstallDecision::AlreadyAvailable
        );
    }

    #[test]
    fn consent_mode_from_env_reads_toolr_auto_install_uv() {
        // Mutating process env is racy across tests in the same binary;
        // serialise inside a single test by handling both states here.
        let prev = std::env::var_os("TOOLR_AUTO_INSTALL_UV");
        // SAFETY: single-threaded test, see comment above.
        unsafe { std::env::set_var("TOOLR_AUTO_INSTALL_UV", "1") };
        let on = ConsentMode::from_env();
        assert!(on.auto_install_env);
        assert!(!on.yes_flag);

        unsafe { std::env::remove_var("TOOLR_AUTO_INSTALL_UV") };
        let off = ConsentMode::from_env();
        assert!(!off.auto_install_env);

        // Non-"1" value also counts as "not set" for our purposes.
        unsafe { std::env::set_var("TOOLR_AUTO_INSTALL_UV", "true") };
        let other = ConsentMode::from_env();
        assert!(!other.auto_install_env);

        // Restore prior value so we don't leak into sibling tests.
        match prev {
            Some(v) => unsafe { std::env::set_var("TOOLR_AUTO_INSTALL_UV", v) },
            None => unsafe { std::env::remove_var("TOOLR_AUTO_INSTALL_UV") },
        }
    }

    #[test]
    fn host_asset_returns_a_known_triple_or_none() {
        let result = host_asset();
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("linux" | "macos" | "windows", "x86_64" | "aarch64") => {
                assert!(result.is_some());
                let (asset, ext) = result.unwrap();
                assert!(asset.starts_with("uv-"));
                assert!(ext == "tar.gz" || ext == "zip");
            }
            _ => {
                // Exotic platform - host_asset returns None, that's fine.
                let _ = result;
            }
        }
    }

    #[test]
    fn host_asset_for_linux_gnu_x86_64() {
        assert_eq!(
            host_asset_for("linux", "x86_64", LinuxLibc::Gnu),
            Some(("uv-x86_64-unknown-linux-gnu", "tar.gz"))
        );
    }

    #[test]
    fn host_asset_for_linux_musl_x86_64() {
        assert_eq!(
            host_asset_for("linux", "x86_64", LinuxLibc::Musl),
            Some(("uv-x86_64-unknown-linux-musl", "tar.gz"))
        );
    }

    #[test]
    fn host_asset_for_linux_gnu_aarch64() {
        assert_eq!(
            host_asset_for("linux", "aarch64", LinuxLibc::Gnu),
            Some(("uv-aarch64-unknown-linux-gnu", "tar.gz"))
        );
    }

    #[test]
    fn host_asset_for_linux_musl_aarch64() {
        assert_eq!(
            host_asset_for("linux", "aarch64", LinuxLibc::Musl),
            Some(("uv-aarch64-unknown-linux-musl", "tar.gz"))
        );
    }

    #[test]
    fn host_asset_for_non_linux_ignores_libc() {
        // The libc value is irrelevant on non-Linux targets; passing
        // either variant must still return the platform-canonical asset.
        for libc in [LinuxLibc::Gnu, LinuxLibc::Musl] {
            assert_eq!(
                host_asset_for("macos", "aarch64", libc),
                Some(("uv-aarch64-apple-darwin", "tar.gz"))
            );
            assert_eq!(
                host_asset_for("windows", "x86_64", libc),
                Some(("uv-x86_64-pc-windows-msvc", "zip"))
            );
        }
    }

    #[test]
    fn host_asset_for_unknown_combo_returns_none() {
        assert_eq!(host_asset_for("freebsd", "x86_64", LinuxLibc::Gnu), None);
        assert_eq!(host_asset_for("linux", "riscv64", LinuxLibc::Gnu), None);
    }

    #[test]
    fn detect_linux_libc_env_override_wins() {
        // Even on a "musl" looking host, an explicit gnu override sticks.
        let libc = detect_linux_libc_from(Some("gnu"), || true, || true);
        assert_eq!(libc, LinuxLibc::Gnu);

        // And vice versa on a "gnu" looking host.
        let libc = detect_linux_libc_from(Some("musl"), || false, || false);
        assert_eq!(libc, LinuxLibc::Musl);
    }

    #[test]
    fn detect_linux_libc_env_override_trims_whitespace() {
        let libc = detect_linux_libc_from(Some("  musl\n"), || false, || false);
        assert_eq!(libc, LinuxLibc::Musl);
    }

    #[test]
    fn detect_linux_libc_unknown_env_override_falls_through() {
        // Typos / unknown values must not short-circuit the rest of the
        // detection — otherwise `TOOLR_UV_LIBC=glibc` would silently
        // force gnu on an actually-musl host.
        let libc = detect_linux_libc_from(Some("glibc"), || true, || false);
        assert_eq!(libc, LinuxLibc::Musl);
    }

    #[test]
    fn detect_linux_libc_alpine_release_means_musl() {
        let libc = detect_linux_libc_from(None, || true, || false);
        assert_eq!(libc, LinuxLibc::Musl);
    }

    #[test]
    fn detect_linux_libc_ldd_musl_means_musl() {
        let libc = detect_linux_libc_from(None, || false, || true);
        assert_eq!(libc, LinuxLibc::Musl);
    }

    #[test]
    fn detect_linux_libc_default_is_gnu() {
        let libc = detect_linux_libc_from(None, || false, || false);
        assert_eq!(libc, LinuxLibc::Gnu);
    }

    #[test]
    fn detect_linux_libc_empty_env_override_falls_through() {
        let libc = detect_linux_libc_from(Some(""), || true, || false);
        assert_eq!(libc, LinuxLibc::Musl);
    }

    #[test]
    fn asset_url_points_at_latest_download() {
        let url = asset_url("uv-x86_64-apple-darwin", "tar.gz");
        assert_eq!(
            url,
            "https://github.com/astral-sh/uv/releases/latest/download/uv-x86_64-apple-darwin.tar.gz"
        );
    }

    #[cfg(unix)]
    #[test]
    fn set_executable_marks_file_runnable() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("victim");
        fs::write(&path, b"#!/bin/sh\necho hi\n").unwrap();
        // Start with 0o600 so we can observe the bit flip.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        set_executable(&path).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
    }

    #[cfg(unix)]
    #[test]
    fn extract_tar_gz_unpacks_archive_into_dest() {
        // Skip when `tar` isn't on PATH (very rare; CI containers have it).
        if which::which("tar").is_err() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        let payload = src_dir.join("hello.txt");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(&payload, b"world").unwrap();

        let archive = tmp.path().join("bundle.tar.gz");
        let status = Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(tmp.path())
            .arg("src")
            .status()
            .unwrap();
        assert!(status.success());

        let out = tmp.path().join("out");
        fs::create_dir_all(&out).unwrap();
        extract_tar_gz(&archive, &out).unwrap();
        assert_eq!(fs::read(out.join("src").join("hello.txt")).unwrap(), b"world");
    }

    #[cfg(unix)]
    #[test]
    fn extract_tar_gz_reports_failure_for_missing_archive() {
        if which::which("tar").is_err() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope.tar.gz");
        let out = tmp.path().join("out");
        fs::create_dir_all(&out).unwrap();
        let err = extract_tar_gz(&missing, &out).unwrap_err();
        match err {
            UvError::Http(msg) => assert!(msg.contains("tar exited with status")),
            other => panic!("expected Http error, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn extract_zip_unpacks_archive_into_dest() {
        if which::which("zip").is_err() || which::which("unzip").is_err() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("hello.txt"), b"world").unwrap();
        let archive = tmp.path().join("bundle.zip");
        let status = Command::new("zip")
            .arg("-qr")
            .arg(&archive)
            .arg("src")
            .current_dir(tmp.path())
            .status()
            .unwrap();
        assert!(status.success());

        let out = tmp.path().join("out");
        fs::create_dir_all(&out).unwrap();
        extract_zip(&archive, &out).unwrap();
        assert_eq!(fs::read(out.join("src").join("hello.txt")).unwrap(), b"world");
    }

    #[cfg(unix)]
    #[test]
    fn extract_zip_reports_failure_for_missing_archive() {
        if which::which("unzip").is_err() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("out");
        fs::create_dir_all(&out).unwrap();
        let err = extract_zip(&tmp.path().join("nope.zip"), &out).unwrap_err();
        match err {
            UvError::Http(msg) => assert!(msg.contains("unzip exited with status")),
            other => panic!("expected Http error, got {other:?}"),
        }
    }

    #[test]
    fn perform_install_surfaces_http_failure_for_unreachable_host() {
        // Drive `download_and_extract` directly with a guaranteed-bad URL so
        // we exercise the reqwest error path without touching the network's
        // real DNS / firewall behaviour. Using a `.invalid` TLD per RFC 2606.
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("bin").join("uv");
        let err = download_and_extract(
            "http://this-host-cannot-exist.invalid/uv-x86_64-apple-darwin.tar.gz",
            "uv-x86_64-apple-darwin",
            "tar.gz",
            &dest,
        )
        .unwrap_err();
        match err {
            UvError::Http(_) => {}
            other => panic!("expected Http error, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn download_and_extract_against_local_http_with_missing_binary_in_archive() {
        // Stands up a one-shot HTTP/1.1 server on 127.0.0.1:<auto> that
        // serves a tar.gz lacking the `uv` binary, then asserts that
        // `download_and_extract` walks the pipeline and surfaces the
        // "extracted archive did not contain a uv binary" Http error.
        // This covers the post-fetch / post-extract branch that the
        // unreachable-host test can't reach.
        if which::which("tar").is_err() {
            return;
        }
        // Build a tar.gz with one file that isn't `uv`.
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("uv-fake-asset").join("not-uv");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, b"placeholder").unwrap();
        let archive = tmp.path().join("uv.tar.gz");
        let ok = Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(tmp.path())
            .arg("uv-fake-asset")
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let payload = std::fs::read(&archive).unwrap();

        let server = serve_once(payload);
        let dest = tmp.path().join("bin").join("uv");
        let err = download_and_extract(
            &format!("http://{}/uv.tar.gz", server.addr),
            "uv-fake-asset",
            "tar.gz",
            &dest,
        )
        .unwrap_err();
        match err {
            UvError::Http(msg) => assert!(
                msg.contains("did not contain a uv binary"),
                "got: {msg}"
            ),
            other => panic!("expected Http error, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn download_and_extract_happy_path_writes_executable_uv_binary() {
        // Serve a tar.gz containing a fake `uv` script at the asset root.
        // Verifies the full pipeline: fetch → extract → locate binary →
        // copy → set_executable. This is the single test that drives
        // download_and_extract's success arm end-to-end.
        if which::which("tar").is_err() {
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let asset_name = "uv-asset-x";
        let src_dir = tmp.path().join(asset_name);
        std::fs::create_dir_all(&src_dir).unwrap();
        let fake_uv = src_dir.join("uv");
        std::fs::write(&fake_uv, b"#!/bin/sh\necho fake-uv\n").unwrap();
        std::fs::set_permissions(&fake_uv, std::fs::Permissions::from_mode(0o644)).unwrap();
        let archive = tmp.path().join("uv.tar.gz");
        assert!(
            Command::new("tar")
                .arg("-czf")
                .arg(&archive)
                .arg("-C")
                .arg(tmp.path())
                .arg(asset_name)
                .status()
                .unwrap()
                .success()
        );
        let payload = std::fs::read(&archive).unwrap();

        let server = serve_once(payload);
        let dest = tmp.path().join("dest").join("uv");
        download_and_extract(
            &format!("http://{}/uv.tar.gz", server.addr),
            asset_name,
            "tar.gz",
            &dest,
        )
        .expect("happy path should succeed");
        assert!(dest.is_file(), "destination uv should exist");
        let mode = std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755, "set_executable should mark dest 0o755");
    }

    /// Minimal one-shot HTTP/1.1 server for the install tests. Binds an
    /// ephemeral port, replies to the first request with the supplied
    /// `payload` as `application/octet-stream`, then exits. Lives in a
    /// background thread; the JoinHandle is detached on drop.
    struct OneShotServer {
        addr: String,
        _join: std::thread::JoinHandle<()>,
    }

    fn serve_once(payload: Vec<u8>) -> OneShotServer {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let join = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            // Drain the request headers — we don't care about the body.
            let _ = sock.read(&mut buf);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            sock.write_all(header.as_bytes()).unwrap();
            sock.write_all(&payload).unwrap();
            let _ = sock.flush();
        });
        OneShotServer { addr, _join: join }
    }
}
