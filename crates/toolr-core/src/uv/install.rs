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
