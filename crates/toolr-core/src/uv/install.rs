//! Decision logic + executor for installing a toolr-managed uv.

use std::fs;
use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;

use super::{UvBinary, UvError, UvSource, managed_uv_path, probe};
use crate::env::{env_truthy, parse_bool};
use crate::path_search::find_on_path_in;

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
    /// `TOOLR_AUTO_SYNC` (or its older alias `TOOLR_AUTO_INSTALL_UV`) is true in the environment.
    pub auto_install_env: bool,
    /// Caller must never prompt (`--quiet`, scripts, shell-enter hooks). If uv
    /// isn't available and `yes_flag`/`auto_install_env` haven't pre-authorised
    /// an install, return `Refuse`; the caller decides what to print.
    pub never_prompt: bool,
}

impl ConsentMode {
    /// Consent from the environment, plus the caller's `never_prompt` (set by
    /// `--quiet`: a scripted run must not stop to ask).
    pub fn from_env(never_prompt: bool) -> Self {
        Self {
            yes_flag: false,
            auto_install_env: auto_sync_from_env(),
            never_prompt,
        }
    }
}

/// `TOOLR_AUTO_SYNC` when it holds a boolean, so `TOOLR_AUTO_SYNC=0` beats a leftover
/// `TOOLR_AUTO_INSTALL_UV=1`; otherwise the older `TOOLR_AUTO_INSTALL_UV`.
fn auto_sync_from_env() -> bool {
    std::env::var("TOOLR_AUTO_SYNC")
        .ok()
        .and_then(|v| parse_bool(&v))
        .unwrap_or_else(|| env_truthy("TOOLR_AUTO_INSTALL_UV"))
}

/// Pure decision logic. `path_found` is whether `find_uv_on_path` returned
/// `Some`. `managed_found` is whether `find_managed_uv` returned `Some`.
/// `prompt_io` is the (input, output) pair to prompt on, or `None` when
/// stdin is not a TTY; then we never prompt and rely on `consent` instead.
pub fn decide_install(
    path_found: bool,
    managed_found: bool,
    consent: ConsentMode,
    prompt_io: Option<(&mut dyn BufRead, &mut dyn Write)>,
) -> InstallDecision {
    if path_found || managed_found {
        return InstallDecision::AlreadyAvailable;
    }
    if consent.yes_flag || consent.auto_install_env {
        return InstallDecision::Install;
    }
    if consent.never_prompt {
        return InstallDecision::Refuse;
    }
    let Some((input, output)) = prompt_io else {
        return InstallDecision::Refuse;
    };
    match prompt_for_consent(input, output) {
        Ok(true) => InstallDecision::Install,
        _ => InstallDecision::Refuse,
    }
}

/// Print the prompt to `stderr`, read the answer from `input`, and return
/// `true` for "install".
fn prompt_for_consent(input: &mut dyn BufRead, stderr: &mut dyn Write) -> Result<bool, UvError> {
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
    match read_consent(input)? {
        Some(consent) => Ok(consent),
        None => {
            // EOF echoes no newline, so the error would land on the prompt line.
            writeln!(stderr)?;
            Ok(false)
        }
    }
}

/// Read one answer line. Bare Enter means "install". Returns `None` on EOF
/// (Ctrl-D, closed stdin): no answer, which is not consent.
fn read_consent(input: &mut dyn BufRead) -> std::io::Result<Option<bool>> {
    let mut buf = String::new();
    if input.read_line(&mut buf)? == 0 {
        return Ok(None);
    }
    Ok(Some(matches!(buf.trim(), "I" | "i" | "")))
}

/// Convenience that combines a TTY check + decision.
pub fn decide_install_auto(
    path_found: bool,
    managed_found: bool,
    consent: ConsentMode,
) -> InstallDecision {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut stderr = std::io::stderr();
    let prompt_io = stdin.is_terminal().then_some((
        &mut input as &mut dyn BufRead,
        &mut stderr as &mut dyn Write,
    ));
    decide_install(path_found, managed_found, consent, prompt_io)
}

/// Linux libc variant. Picked at runtime via `detect_linux_libc` so
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
pub(crate) fn host_asset() -> Option<(&'static str, &'static str)> {
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
pub(crate) fn detect_linux_libc() -> LinuxLibc {
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
    ldd_reports_musl_in(std::env::var_os("PATH"))
}

/// Only an `ldd` in an absolute `PATH` directory runs; none means not musl.
fn ldd_reports_musl_in(path: Option<std::ffi::OsString>) -> bool {
    let Some(ldd) = find_on_path_in("ldd", path) else {
        return false;
    };
    let Ok(output) = std::process::Command::new(ldd).arg("--version").output() else {
        return false;
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    stdout.to_lowercase().contains("musl") || stderr.to_lowercase().contains("musl")
}

/// Where uv's GitHub releases live.
const UV_RELEASES: &str = "https://github.com/astral-sh/uv";

/// A `sha256sum` line is under 200 bytes; anything far bigger isn't one.
const MAX_SIDECAR_BYTES: u64 = 4 * 1024;

/// uv's largest archive is about 19 MB; ten times that is still no uv.
const MAX_ARCHIVE_BYTES: u64 = 200 * 1024 * 1024;

/// Read at most `limit` bytes of `body`; `None` when it holds more. Counts
/// the bytes read, so a lying `Content-Length` can't get past it.
fn read_capped(body: impl std::io::Read, limit: u64) -> std::io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut body.take(limit + 1), &mut bytes)?;
    Ok((bytes.len() as u64 <= limit).then_some(bytes))
}

fn latest_asset_url(base: &str, file: &str) -> String {
    format!("{base}/releases/latest/download/{file}")
}

fn tagged_asset_url(base: &str, tag: &str, file: &str) -> String {
    format!("{base}/releases/download/{tag}/{file}")
}

fn unverified(file: &str, reason: impl Into<String>) -> UvError {
    UvError::Unverified {
        asset: file.to_string(),
        reason: reason.into(),
    }
}

/// Read the release tag from the `/latest` redirect without following it, so
/// the checksum and the archive both come from that one release.
fn resolve_tag(base: &str, file: &str) -> Result<String, UvError> {
    let url = latest_asset_url(base, file);
    // `Client::builder()` keeps reqwest's HTTP(S)_PROXY handling.
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| UvError::Http(e.to_string()))?;
    // A 4xx/5xx is a failed request, like an unreachable host, not a bad redirect.
    let response = client.get(&url).send().and_then(|r| r.error_for_status());
    let response = response.map_err(|e| UvError::Http(e.to_string()))?;
    if !response.status().is_redirection() {
        let status = response.status();
        return Err(unverified(
            file,
            format!("{url} answered {status}, not a redirect"),
        ));
    }
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| unverified(file, format!("{url} redirected without a Location")))?;
    let target = response
        .url()
        .join(location)
        .map_err(|e| unverified(file, format!("bad redirect {location:?}: {e}")))?;
    tag_from_path(target.path(), file)
        .map(str::to_string)
        .ok_or_else(|| unverified(file, format!("unexpected redirect to {target}")))
}

fn tag_from_path<'a>(path: &'a str, file: &str) -> Option<&'a str> {
    let (rest, tag) = path
        .strip_suffix(file)?
        .strip_suffix('/')?
        .rsplit_once('/')?;
    let valid = !tag.is_empty()
        && tag != "."
        && tag != ".."
        && tag
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c));
    (valid && rest.ends_with("/releases/download")).then_some(tag)
}

/// Parse a `sha256sum`-format sidecar for `file` into its lowercase digest.
fn parse_sidecar(text: &str, file: &str) -> Result<String, UvError> {
    let mut tokens = text.split_whitespace();
    let digest = tokens
        .next()
        .filter(|d| d.len() == 64 && d.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| unverified(file, "the checksum file holds no SHA-256 digest"))?;
    match (tokens.next(), tokens.next()) {
        (None, _) => {}
        (Some(name), None) if name.strip_prefix('*').unwrap_or(name) == file => {}
        _ => return Err(unverified(file, "the checksum file names another file")),
    }
    Ok(digest.to_ascii_lowercase())
}

/// Download + extract uv into the managed location.
pub fn perform_install() -> Result<UvBinary, UvError> {
    let dest = managed_uv_path()
        .ok_or_else(|| UvError::Install("could not resolve $XDG_DATA_HOME/toolr/bin".into()))?;
    perform_install_from(UV_RELEASES, &dest, MAX_ARCHIVE_BYTES)
}

/// Install the host's uv from the latest release under `base`, after checking
/// it against the `.sha256` file published in that same release.
pub(crate) fn perform_install_from(
    base: &str,
    dest: &Path,
    max_archive_bytes: u64,
) -> Result<UvBinary, UvError> {
    let (asset, ext) = host_asset().ok_or_else(unsupported_host)?;
    let file = format!("{asset}.{ext}");
    let tag = resolve_tag(base, &file)?;
    let sidecar_url = tagged_asset_url(base, &tag, &format!("{file}.sha256"));
    // Without its checksum the archive can't be trusted, so a failed fetch is unverified.
    let sidecar = fetch_capped(&sidecar_url, MAX_SIDECAR_BYTES, &file, "checksum file").map_err(
        |e| match e {
            UvError::Http(e) => unverified(&file, format!("no checksum file: {e}")),
            other => other,
        },
    )?;
    let expected = parse_sidecar(&String::from_utf8_lossy(&sidecar), &file)?;
    let url = tagged_asset_url(base, &tag, &file);
    let bytes = fetch_capped(&url, max_archive_bytes, &file, "archive")?;

    // Hash the exact bytes we unpack, before anything reaches the disk.
    let actual = sha256_hex(&bytes);
    if actual != expected {
        return Err(UvError::ChecksumMismatch {
            asset: file,
            tag,
            expected,
            actual,
        });
    }
    install_archive(&bytes, asset, ext, dest)?;
    probe(dest, UvSource::FreshlyInstalled)
}

fn unsupported_host() -> UvError {
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    UvError::Install(format!("no prebuilt uv asset for {os}-{arch}"))
}

/// GET `url` and return its body, refusing one over `limit` bytes as an
/// unverified `file`; `what` names the body in that refusal.
fn fetch_capped(url: &str, limit: u64, file: &str, what: &str) -> Result<Vec<u8>, UvError> {
    let response = reqwest::blocking::get(url).and_then(|r| r.error_for_status());
    let response = response.map_err(|e| UvError::Http(e.to_string()))?;
    let body = read_capped(response, limit).map_err(|e| UvError::Http(e.to_string()))?;
    body.ok_or_else(|| unverified(file, format!("the {what} exceeds {limit} bytes")))
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Unpack the verified `asset` archive and move its uv binary to `dest`.
fn install_archive(bytes: &[u8], asset: &str, ext: &str, dest: &Path) -> Result<(), UvError> {
    let parent = dest
        .parent()
        .ok_or_else(|| UvError::Install("managed uv path has no parent directory".into()))?;
    fs::create_dir_all(parent)?;
    let tmp = tempfile::tempdir()?;
    let archive_path = tmp.path().join(format!("{asset}.{ext}"));
    fs::write(&archive_path, bytes)?;

    match ext {
        "tar.gz" => extract_tar_gz(&archive_path, tmp.path())?,
        "zip" => extract_zip(&archive_path, tmp.path())?,
        other => {
            return Err(UvError::Install(format!(
                "unknown archive extension: {other}"
            )));
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
        UvError::Install(format!(
            "extracted archive did not contain a {binary_name} binary"
        ))
    })?;

    install_staged(src, parent, dest)
}

/// Stage beside `dest` and rename over it, so a failed write never leaves a
/// truncated uv; the staged file is removed on drop if we bail out.
fn install_staged(src: &Path, parent: &Path, dest: &Path) -> Result<(), UvError> {
    let mut staged = tempfile::Builder::new()
        .prefix(".uv-")
        .tempfile_in(parent)?;
    std::io::copy(&mut fs::File::open(src)?, staged.as_file_mut())?;
    set_executable(staged.path())?;
    // Name the path: on Windows a running uv.exe (a parallel install won) refuses the rename.
    staged.persist(dest).map_err(|e| {
        let msg = format!(
            "couldn't move uv into place at {}: {}",
            dest.display(),
            e.error
        );
        UvError::Io(std::io::Error::new(e.error.kind(), msg))
    })?;
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

// In-process, so the managed install needs no `tar` or `unzip` on the host
// (stock Windows has no `unzip`). Neither writes outside `into`: tar skips an
// escaping entry, zip rejects the whole archive.
fn extract_tar_gz(archive: &Path, into: &Path) -> Result<(), UvError> {
    let file = fs::File::open(archive).map_err(|e| UvError::extract(archive, e))?;
    tar::Archive::new(flate2::read::GzDecoder::new(file))
        .unpack(into)
        .map_err(|e| UvError::extract(archive, e))
}

fn extract_zip(archive: &Path, into: &Path) -> Result<(), UvError> {
    let file = fs::File::open(archive).map_err(|e| UvError::extract(archive, e))?;
    zip::ZipArchive::new(file)
        .and_then(|mut zip| zip.extract(into))
        .map_err(|e| UvError::extract(archive, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_env::EnvGuard;
    use std::collections::HashMap;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    #[test]
    fn install_staged_names_the_destination_when_the_rename_fails() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("uv-src");
        fs::write(&src, b"uv").unwrap();
        // A non-empty directory can't be renamed over on any platform.
        let dest = dir.path().join("uv");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("keep"), b"").unwrap();

        let err = install_staged(&src, dir.path(), &dest).unwrap_err();

        assert!(
            err.to_string().contains(&dest.display().to_string()),
            "{err}"
        );
        assert!(dest.join("keep").is_file());
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(".uv-"))
            .collect();
        assert!(leftovers.is_empty(), "staged file left behind");
    }

    #[test]
    fn read_consent_eof_is_no_answer() {
        assert_eq!(read_consent(&mut std::io::Cursor::new("")).unwrap(), None);
    }

    #[test]
    fn prompts_on_a_tty_and_follows_the_answer() {
        let _env = EnvGuard::read();
        for (answer, expected) in [
            ("\n", InstallDecision::Install),
            ("M\n", InstallDecision::Refuse),
            ("", InstallDecision::Refuse),
        ] {
            let mut out = Vec::new();
            assert_eq!(
                decide_install(
                    false,
                    false,
                    ConsentMode::default(),
                    Some((&mut std::io::Cursor::new(answer), &mut out))
                ),
                expected,
                "{answer:?}"
            );
            assert!(String::from_utf8(out).unwrap().contains("Choice [I/M]"));
        }
    }

    #[test]
    fn prompt_for_consent_refuses_on_eof_and_ends_the_line() {
        let _env = EnvGuard::read();
        let mut out = Vec::new();
        assert!(!prompt_for_consent(&mut std::io::Cursor::new(""), &mut out).unwrap());
        assert!(String::from_utf8(out).unwrap().ends_with("Choice [I/M] \n"));
    }

    #[test]
    fn prompt_for_consent_relies_on_the_echoed_newline_for_answers() {
        let _env = EnvGuard::read();
        for (answer, expected) in [("\n", true), ("M\n", false)] {
            let mut out = Vec::new();
            assert_eq!(
                prompt_for_consent(&mut std::io::Cursor::new(answer), &mut out).unwrap(),
                expected,
                "{answer:?}"
            );
            assert!(String::from_utf8(out).unwrap().ends_with("Choice [I/M] "));
        }
    }

    #[test]
    fn read_consent_accepts_enter_and_i() {
        for answer in ["\n", "\r\n", "i\n", "I\n", "  i  \n", "I"] {
            assert_eq!(
                read_consent(&mut std::io::Cursor::new(answer)).unwrap(),
                Some(true),
                "{answer:?}"
            );
        }
    }

    #[test]
    fn read_consent_refuses_anything_else() {
        for answer in ["M\n", "m\n", "yes\n", "n\n"] {
            assert_eq!(
                read_consent(&mut std::io::Cursor::new(answer)).unwrap(),
                Some(false),
                "{answer:?}"
            );
        }
    }

    #[test]
    fn already_available_when_path_present() {
        assert_eq!(
            decide_install(true, false, ConsentMode::default(), None),
            InstallDecision::AlreadyAvailable
        );
    }

    #[test]
    fn already_available_when_managed_present() {
        assert_eq!(
            decide_install(false, true, ConsentMode::default(), None),
            InstallDecision::AlreadyAvailable
        );
    }

    #[test]
    fn yes_flag_forces_install_even_without_tty() {
        let consent = ConsentMode {
            yes_flag: true,
            ..Default::default()
        };
        assert_eq!(
            decide_install(false, false, consent, None),
            InstallDecision::Install
        );
    }

    #[test]
    fn auto_install_env_forces_install_even_without_tty() {
        let consent = ConsentMode {
            auto_install_env: true,
            ..Default::default()
        };
        assert_eq!(
            decide_install(false, false, consent, None),
            InstallDecision::Install
        );
    }

    #[test]
    fn refuses_non_interactive_without_consent() {
        assert_eq!(
            decide_install(false, false, ConsentMode::default(), None),
            InstallDecision::Refuse
        );
    }

    #[test]
    fn never_prompt_returns_refuse_without_consent_or_tty() {
        let consent = ConsentMode {
            never_prompt: true,
            ..Default::default()
        };
        // No path/managed uv, stdin is a TTY answering Enter → without
        // never_prompt we'd prompt and install. With it we must Refuse.
        assert_eq!(
            decide_install(
                false,
                false,
                consent,
                Some((&mut std::io::Cursor::new("\n"), &mut Vec::new()))
            ),
            InstallDecision::Refuse,
        );
    }

    #[test]
    fn never_prompt_does_not_override_already_available() {
        let consent = ConsentMode {
            never_prompt: true,
            ..Default::default()
        };
        // uv is on PATH: never_prompt must NOT pretend it's missing.
        assert_eq!(
            decide_install(true, false, consent, None),
            InstallDecision::AlreadyAvailable,
        );
    }

    #[test]
    fn never_prompt_does_not_override_explicit_consent() {
        let consent = ConsentMode {
            never_prompt: true,
            yes_flag: true,
            ..Default::default()
        };
        // Caller explicitly asked for unattended install via --yes;
        // never_prompt must not contradict that. Explicit consent wins.
        assert_eq!(
            decide_install(false, false, consent, None),
            InstallDecision::Install,
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
    fn consent_mode_from_env_reads_toolr_auto_sync_and_its_alias() {
        let mut env = EnvGuard::write();
        env.remove("TOOLR_AUTO_SYNC");
        env.remove("TOOLR_AUTO_INSTALL_UV");
        assert!(!ConsentMode::from_env(false).auto_install_env);

        env.set("TOOLR_AUTO_SYNC", "1");
        let on = ConsentMode::from_env(false);
        assert!(on.auto_install_env);
        assert!(!on.yes_flag);
        env.remove("TOOLR_AUTO_SYNC");

        env.set("TOOLR_AUTO_INSTALL_UV", "yes");
        assert!(ConsentMode::from_env(false).auto_install_env);

        // A false spelling is off, and so is anything that isn't a spelling at all.
        env.set("TOOLR_AUTO_INSTALL_UV", "0");
        env.set("TOOLR_AUTO_SYNC", "maybe");
        assert!(!ConsentMode::from_env(false).auto_install_env);

        // The new name wins when it holds a boolean; an unparsable one defers to the alias.
        env.set("TOOLR_AUTO_SYNC", "0");
        env.set("TOOLR_AUTO_INSTALL_UV", "1");
        assert!(!ConsentMode::from_env(false).auto_install_env);
        env.set("TOOLR_AUTO_SYNC", "maybe");
        assert!(ConsentMode::from_env(false).auto_install_env);
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

    #[cfg(unix)]
    fn stub_ldd(dir: &Path, stdout: &str, stderr: &str) {
        let body = format!("#!/bin/sh\necho '{stdout}'\necho '{stderr}' >&2\n");
        crate::test_fs::write_executable(dir, "ldd", &body, 0o755);
    }

    #[cfg(unix)]
    #[test]
    fn ldd_reports_musl_reads_both_streams() {
        let musl = tempfile::tempdir().unwrap();
        stub_ldd(musl.path(), "", "musl libc (x86_64)");
        assert!(ldd_reports_musl_in(Some(musl.path().into())));
        let gnu = tempfile::tempdir().unwrap();
        stub_ldd(gnu.path(), "ldd (GNU libc) 2.39", "");
        assert!(!ldd_reports_musl_in(Some(gnu.path().into())));
    }

    #[cfg(unix)]
    #[test]
    fn ldd_reports_musl_ignores_an_ldd_reached_through_a_relative_entry() {
        let dir = tempfile::tempdir().unwrap();
        stub_ldd(dir.path(), "", "musl libc (x86_64)");
        let rel = crate::test_fs::relative_to_cwd(dir.path());
        assert!(rel.is_relative() && rel.join("ldd").is_file(), "{rel:?}");
        assert!(!ldd_reports_musl_in(Some(rel.into())));
    }

    #[test]
    fn ldd_reports_musl_is_false_with_path_unset() {
        assert!(!ldd_reports_musl_in(None));
    }

    const HEX: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const FILE: &str = "uv-x86_64-apple-darwin.tar.gz";

    #[test]
    fn url_builders_point_at_latest_and_tagged_downloads() {
        assert_eq!(
            latest_asset_url(UV_RELEASES, FILE),
            "https://github.com/astral-sh/uv/releases/latest/download/uv-x86_64-apple-darwin.tar.gz"
        );
        assert_eq!(
            tagged_asset_url(UV_RELEASES, "0.13.7", FILE),
            "https://github.com/astral-sh/uv/releases/download/0.13.7/uv-x86_64-apple-darwin.tar.gz"
        );
    }

    #[test]
    fn parse_sidecar_accepts_sha256sum_shapes() {
        for text in [
            format!("{HEX}  {FILE}\n"),
            format!("{HEX} *{FILE}"),
            format!("{HEX}\n"),
            format!("{}  {FILE}\n", HEX.to_uppercase()),
        ] {
            assert_eq!(parse_sidecar(&text, FILE).unwrap(), HEX, "{text:?}");
        }
    }

    #[test]
    fn parse_sidecar_rejects_malformed_checksums() {
        for text in [
            String::new(),
            format!("{}  {FILE}", &HEX[1..]),
            format!("{}g  {FILE}", &HEX[1..]),
            format!("{HEX}  uv-other.tar.gz"),
            format!("{HEX}  {FILE} extra"),
        ] {
            let err = parse_sidecar(&text, FILE).unwrap_err();
            assert!(
                matches!(&err, UvError::Unverified { asset, .. } if asset == FILE),
                "{text:?}: {err:?}"
            );
        }
    }

    fn redirect(location: Option<&str>) -> Route {
        Route {
            status: 302,
            headers: location
                .map(|l| vec![("Location".to_string(), l.to_string())])
                .unwrap_or_default(),
            body: Vec::new(),
        }
    }

    fn latest_path(file: &str) -> String {
        format!("/releases/latest/download/{file}")
    }

    fn tagged_path(tag: &str, file: &str) -> String {
        format!("/releases/download/{tag}/{file}")
    }

    #[test]
    fn resolve_tag_reads_the_tag_from_the_latest_redirect() {
        let server = serve(HashMap::from([(
            latest_path(FILE),
            redirect(Some(&tagged_path("v9.9.9", FILE))),
        )]));
        let base = format!("http://{}", server.addr);
        assert_eq!(resolve_tag(&base, FILE).unwrap(), "v9.9.9");
        // The redirect itself is never followed.
        assert_eq!(*server.hits.lock().unwrap(), [latest_path(FILE)]);
    }

    #[test]
    fn resolve_tag_accepts_an_absolute_location() {
        let location = format!("{UV_RELEASES}/releases/download/0.13.7/{FILE}");
        let server = serve(HashMap::from([(
            latest_path(FILE),
            redirect(Some(&location)),
        )]));
        let base = format!("http://{}", server.addr);
        assert_eq!(resolve_tag(&base, FILE).unwrap(), "0.13.7");
    }

    #[test]
    fn resolve_tag_rejects_anything_but_a_tagged_redirect() {
        let routes = [
            Route::ok(b"archive".to_vec()),
            redirect(None),
            redirect(Some("/releases/download/v1/uv-other.tar.gz")),
            redirect(Some(&format!("/elsewhere/v1/{FILE}"))),
            redirect(Some(&format!("/releases/download/{FILE}"))),
            redirect(Some(&format!("/releases/download//{FILE}"))),
            redirect(Some(&format!("/releases/download/../{FILE}"))),
            redirect(Some(&format!("/releases/download/./{FILE}"))),
            redirect(Some(&format!("/releases/download/a%2Fb/{FILE}"))),
            redirect(Some(&format!("/releases/download/a b/{FILE}"))),
            redirect(Some("http://[::1")),
        ];
        for route in routes {
            let status = route.status;
            let headers = route.headers.clone();
            let server = serve(HashMap::from([(latest_path(FILE), route)]));
            let base = format!("http://{}", server.addr);
            let err = resolve_tag(&base, FILE).unwrap_err();
            assert!(
                matches!(&err, UvError::Unverified { asset, .. } if asset == FILE),
                "{status} {headers:?}: {err:?}"
            );
        }
    }

    #[test]
    fn resolve_tag_reports_a_failed_request_as_http() {
        let server = serve(HashMap::new());
        let err = resolve_tag(&format!("http://{}", server.addr), FILE).unwrap_err();
        assert!(matches!(err, UvError::Http(_)), "got {err:?}");
        let err = resolve_tag("http://this-host-cannot-exist.invalid", FILE).unwrap_err();
        assert!(matches!(err, UvError::Http(_)), "got {err:?}");
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

    fn write_tar_gz(path: &Path, entries: &[(&str, &[u8])]) {
        let gz = flate2::write::GzEncoder::new(
            fs::File::create(path).unwrap(),
            flate2::Compression::default(),
        );
        let mut builder = tar::Builder::new(gz);
        for (name, data) in entries {
            let mut header = tar::Header::new_gnu();
            // Written raw: `set_path` refuses `..`, which the traversal test needs.
            header.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name.as_bytes());
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append(&header, *data).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap();
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn extract_tar_gz_unpacks_archive_into_dest() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("bundle.tar.gz");
        write_tar_gz(&archive, &[("uv-x86_64-apple-darwin/uv", b"binary")]);
        let out = tmp.path().join("out");
        fs::create_dir_all(&out).unwrap();
        extract_tar_gz(&archive, &out).unwrap();
        assert_eq!(
            fs::read(out.join("uv-x86_64-apple-darwin/uv")).unwrap(),
            b"binary"
        );
    }

    #[test]
    fn extract_tar_gz_never_writes_outside_dest() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("evil.tar.gz");
        write_tar_gz(&archive, &[("../escaped", b"x"), ("ok", b"y")]);
        let out = tmp.path().join("out");
        fs::create_dir_all(&out).unwrap();
        // tar skips the escaping entry and unpacks the rest.
        extract_tar_gz(&archive, &out).unwrap();
        assert!(!tmp.path().join("escaped").exists());
        assert_eq!(fs::read(out.join("ok")).unwrap(), b"y");
    }

    #[test]
    fn extract_tar_gz_reports_the_archive_it_could_not_read() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope.tar.gz");
        let err = extract_tar_gz(&missing, tmp.path()).unwrap_err();
        assert!(
            matches!(&err, UvError::Extract { archive, .. } if *archive == missing),
            "got {err:?}"
        );
    }

    #[test]
    fn extract_tar_gz_keeps_the_cause_of_a_truncated_download() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("cut.tar.gz");
        // Incompressible, so cutting the archive in half lands mid-stream.
        let payload: Vec<u8> = (0..200_000u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
            .collect();
        write_tar_gz(&archive, &[("uv-x86_64-apple-darwin/uv", &payload)]);
        let bytes = fs::read(&archive).unwrap();
        fs::write(&archive, &bytes[..bytes.len() / 2]).unwrap();
        let out = tmp.path().join("out");
        fs::create_dir_all(&out).unwrap();
        let err = extract_tar_gz(&archive, &out).unwrap_err();
        // Extract -> "failed to unpack" -> the real cause.
        let err = anyhow::Error::new(err);
        assert!(err.chain().count() >= 3, "cause missing from: {err:#}");
    }

    #[test]
    fn extract_zip_unpacks_archive_into_dest() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("bundle.zip");
        write_zip(&archive, &[("uv.exe", b"binary")]);
        let out = tmp.path().join("out");
        fs::create_dir_all(&out).unwrap();
        extract_zip(&archive, &out).unwrap();
        assert_eq!(fs::read(out.join("uv.exe")).unwrap(), b"binary");
    }

    #[test]
    fn extract_zip_never_writes_outside_dest() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("evil.zip");
        write_zip(&archive, &[("../escaped", b"x"), ("ok", b"y")]);
        let out = tmp.path().join("out");
        fs::create_dir_all(&out).unwrap();
        // zip rejects the whole archive.
        let err = extract_zip(&archive, &out).unwrap_err();
        assert!(matches!(err, UvError::Extract { .. }), "got {err:?}");
        assert!(!tmp.path().join("escaped").exists());
    }

    #[test]
    fn extract_zip_reports_a_corrupt_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("corrupt.zip");
        fs::write(&archive, b"not a zip").unwrap();
        let err = extract_zip(&archive, tmp.path()).unwrap_err();
        assert!(
            matches!(&err, UvError::Extract { archive: got, .. } if *got == archive),
            "got {err:?}"
        );
    }

    #[test]
    fn perform_install_surfaces_http_failure_for_unreachable_host() {
        // `.invalid` never resolves (RFC 2606).
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("bin").join("uv");
        let base = "http://this-host-cannot-exist.invalid";
        let err = perform_install_from(base, &dest, MAX_ARCHIVE_BYTES).unwrap_err();
        assert!(matches!(err, UvError::Http(_)), "got {err:?}");
        assert!(!dest.exists());
    }

    #[test]
    fn sha256_hex_matches_the_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    const TAG: &str = "v9.9.9";

    fn host_file() -> (&'static str, &'static str, String) {
        let (asset, ext) = host_asset().expect("tests run on a host uv publishes for");
        (asset, ext, format!("{asset}.{ext}"))
    }

    /// A fake uv release: `/latest` redirects to `TAG`, which holds the
    /// archive and, when given, its checksum file.
    fn fake_release(file: &str, archive: Vec<u8>, sidecar: Option<String>) -> TestServer {
        let tagged = tagged_path(TAG, file);
        let mut routes = HashMap::from([
            (latest_path(file), redirect(Some(&tagged))),
            (tagged.clone(), Route::ok(archive)),
        ]);
        if let Some(sidecar) = sidecar {
            routes.insert(format!("{tagged}.sha256"), Route::ok(sidecar));
        }
        serve(routes)
    }

    /// The host's archive holding a `uv` that reports `uv 9.9.9`.
    fn host_archive(dir: &Path) -> Vec<u8> {
        host_archive_holding(dir, super::super::uv_basename())
    }

    /// The host's archive with that script stored as `binary` in the asset directory.
    fn host_archive_holding(dir: &Path, binary: &str) -> Vec<u8> {
        let (asset, ext, file) = host_file();
        let script: &[u8] = b"#!/bin/sh\necho 'uv 9.9.9'\n";
        let archive = dir.join(&file);
        let entry = format!("{asset}/{binary}");
        match ext {
            "zip" => write_zip(&archive, &[(&entry, script)]),
            _ => write_tar_gz(&archive, &[(&entry, script)]),
        }
        fs::read(&archive).unwrap()
    }

    /// Serve `archive` as a release with a matching checksum file.
    fn verified_release(archive: Vec<u8>) -> TestServer {
        let (_, _, file) = host_file();
        let sidecar = format!("{}  {file}\n", sha256_hex(&archive));
        fake_release(&file, archive, Some(sidecar))
    }

    #[cfg(unix)]
    #[test]
    fn perform_install_from_installs_a_verified_release() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let (_, _, file) = host_file();
        let server = verified_release(host_archive(tmp.path()));
        let dest = tmp.path().join("bin").join("uv");

        let base = format!("http://{}", server.addr);
        let uv = perform_install_from(&base, &dest, MAX_ARCHIVE_BYTES).unwrap();

        assert_eq!(uv.version, (9, 9, 9));
        assert_eq!(uv.source, UvSource::FreshlyInstalled);
        let mode = fs::metadata(&dest).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
        let tagged = tagged_path(TAG, &file);
        assert_eq!(
            *server.hits.lock().unwrap(),
            [latest_path(&file), format!("{tagged}.sha256"), tagged]
        );
    }

    #[cfg(unix)]
    #[test]
    fn perform_install_from_follows_the_asset_redirects() {
        // GitHub answers asset downloads with a 302 to its object store.
        let tmp = tempfile::tempdir().unwrap();
        let (_, _, file) = host_file();
        let archive = host_archive(tmp.path());
        let sidecar = format!("{}  {file}\n", sha256_hex(&archive));
        let tagged = tagged_path(TAG, &file);
        let server = serve(HashMap::from([
            (latest_path(&file), redirect(Some(&tagged))),
            (tagged.clone(), redirect(Some("/objects/archive"))),
            (
                format!("{tagged}.sha256"),
                redirect(Some("/objects/sidecar")),
            ),
            ("/objects/archive".to_string(), Route::ok(archive)),
            ("/objects/sidecar".to_string(), Route::ok(sidecar)),
        ]));
        let dest = tmp.path().join("bin").join("uv");

        let base = format!("http://{}", server.addr);
        let uv = perform_install_from(&base, &dest, MAX_ARCHIVE_BYTES).unwrap();

        assert_eq!(uv.version, (9, 9, 9));
        // The `/latest` redirect is read, not followed; the asset ones are followed.
        assert_eq!(
            *server.hits.lock().unwrap(),
            [
                latest_path(&file),
                format!("{tagged}.sha256"),
                "/objects/sidecar".to_string(),
                tagged,
                "/objects/archive".to_string(),
            ]
        );
    }

    #[test]
    fn perform_install_from_refuses_a_checksum_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, _, file) = host_file();
        let archive = host_archive(tmp.path());
        let actual_digest = sha256_hex(&archive);
        let server = fake_release(&file, archive, Some(format!("{HEX}  {file}\n")));
        let dest = tmp.path().join("bin").join("uv");

        let base = format!("http://{}", server.addr);
        let err = perform_install_from(&base, &dest, MAX_ARCHIVE_BYTES).unwrap_err();

        match err {
            UvError::ChecksumMismatch {
                asset,
                tag,
                expected,
                actual,
            } => {
                assert_eq!(asset, file);
                assert_eq!(tag, TAG);
                assert_eq!(expected, HEX);
                assert_eq!(actual, actual_digest);
            }
            other => panic!("expected ChecksumMismatch, got {other:?}"),
        }
        assert!(!dest.parent().unwrap().exists());
    }

    #[test]
    fn perform_install_from_refuses_a_missing_or_malformed_checksum() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, _, file) = host_file();
        for sidecar in [None, Some(format!("{HEX}  uv-other.tar.gz\n"))] {
            let server = fake_release(&file, host_archive(tmp.path()), sidecar.clone());
            let dest = tmp.path().join("bin").join("uv");

            let base = format!("http://{}", server.addr);
            let err = perform_install_from(&base, &dest, MAX_ARCHIVE_BYTES).unwrap_err();

            assert!(
                matches!(&err, UvError::Unverified { asset, .. } if *asset == file),
                "{sidecar:?}: {err:?}"
            );
            assert!(!dest.parent().unwrap().exists());
            // The archive is never fetched without a usable checksum.
            assert!(
                !server
                    .hits
                    .lock()
                    .unwrap()
                    .contains(&tagged_path(TAG, &file))
            );
        }
    }

    #[test]
    fn perform_install_from_refuses_an_oversize_checksum_file() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, _, file) = host_file();
        let archive = host_archive(tmp.path());
        // A valid line padded past the cap: only the size can reject it.
        let mut sidecar = format!("{}  {file}\n", sha256_hex(&archive));
        sidecar.push_str(&" ".repeat(MAX_SIDECAR_BYTES as usize));
        let server = fake_release(&file, archive, Some(sidecar));
        let dest = tmp.path().join("bin").join("uv");

        let base = format!("http://{}", server.addr);
        let err = perform_install_from(&base, &dest, MAX_ARCHIVE_BYTES).unwrap_err();

        assert!(
            matches!(&err, UvError::Unverified { asset, .. } if *asset == file),
            "got {err:?}"
        );
        assert!(!dest.parent().unwrap().exists());
        assert!(
            !server
                .hits
                .lock()
                .unwrap()
                .contains(&tagged_path(TAG, &file))
        );
    }

    #[test]
    fn perform_install_from_refuses_an_archive_over_the_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, _, file) = host_file();
        let archive = host_archive(tmp.path());
        let max = archive.len() as u64 - 1;
        let server = verified_release(archive);
        let dest = tmp.path().join("bin").join("uv");

        let err = perform_install_from(&format!("http://{}", server.addr), &dest, max).unwrap_err();

        assert!(
            matches!(&err, UvError::Unverified { asset, .. } if *asset == file),
            "got {err:?}"
        );
        assert!(!dest.parent().unwrap().exists());
    }

    #[test]
    fn read_capped_allows_exactly_the_limit() {
        assert_eq!(read_capped(&b"abc"[..], 3).unwrap(), Some(b"abc".to_vec()));
        assert_eq!(read_capped(&b"abcd"[..], 3).unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn perform_install_from_replaces_an_existing_uv_whole() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let server = verified_release(host_archive(tmp.path()));
        let dest = tmp.path().join("bin").join("uv");
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        fs::write(&dest, b"old uv").unwrap();
        // Read-only, so only a rename can replace it: an in-place copy fails.
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o444)).unwrap();

        let base = format!("http://{}", server.addr);
        let uv = perform_install_from(&base, &dest, MAX_ARCHIVE_BYTES).unwrap();

        assert_eq!(uv.version, (9, 9, 9));
        assert_eq!(dir_names(dest.parent().unwrap()), ["uv"]);
    }

    #[test]
    fn a_failed_install_leaves_an_existing_uv_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, _, file) = host_file();
        let dest = tmp.path().join("bin").join("uv");
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        fs::write(&dest, b"old uv").unwrap();

        let server = fake_release(
            &file,
            host_archive(tmp.path()),
            Some(format!("{HEX}  {file}\n")),
        );
        let base = format!("http://{}", server.addr);
        let err = perform_install_from(&base, &dest, MAX_ARCHIVE_BYTES).unwrap_err();
        assert!(
            matches!(err, UvError::ChecksumMismatch { .. }),
            "got {err:?}"
        );
        assert_eq!(fs::read(&dest).unwrap(), b"old uv");

        // A verified archive that lacks the binary fails after extraction.
        let server = verified_release(host_archive_holding(tmp.path(), "not-uv"));
        let base = format!("http://{}", server.addr);
        let err = perform_install_from(&base, &dest, MAX_ARCHIVE_BYTES).unwrap_err();
        assert!(
            matches!(&err, UvError::Install(msg) if msg.contains("did not contain")),
            "got {err:?}"
        );
        assert_eq!(fs::read(&dest).unwrap(), b"old uv");
        assert_eq!(dir_names(dest.parent().unwrap()), ["uv"]);
    }

    fn dir_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn install_archive_unpacks_a_zip_asset() {
        // The Windows asset shape: a zip with the binary at its root.
        let binary_name = super::super::uv_basename();
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("uv.zip");
        write_zip(&archive, &[(binary_name, b"fake-uv")]);
        let dest = tmp.path().join("dest").join(binary_name);
        let bytes = fs::read(&archive).unwrap();
        install_archive(&bytes, "uv-x86_64-pc-windows-msvc", "zip", &dest).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"fake-uv");
    }

    #[test]
    fn unsupported_host_names_the_platform() {
        let err = unsupported_host();
        let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
        assert!(
            matches!(&err, UvError::Install(msg) if msg.contains(&platform)),
            "got {err:?}"
        );
    }

    #[test]
    fn install_archive_refuses_an_unknown_extension_or_a_rootless_dest() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("bin").join("uv");
        let err = install_archive(b"x", "uv-a", "rar", &dest).unwrap_err();
        assert!(matches!(err, UvError::Install(_)), "got {err:?}");
        let err = install_archive(b"x", "uv-a", "tar.gz", Path::new("/")).unwrap_err();
        assert!(matches!(err, UvError::Install(_)), "got {err:?}");
    }

    #[derive(Clone)]
    struct Route {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    impl Route {
        fn ok(body: impl Into<Vec<u8>>) -> Self {
            Self {
                status: 200,
                headers: Vec::new(),
                body: body.into(),
            }
        }
    }

    /// HTTP/1.1 server for the install tests: answers each request by its
    /// path (404 for unknown ones) and records every path it saw.
    struct TestServer {
        addr: String,
        hits: Arc<Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            // Wake the blocking `accept` so the thread sees `stop`.
            let _ = std::net::TcpStream::connect(&self.addr);
        }
    }

    fn serve(routes: HashMap<String, Route>) -> TestServer {
        use std::io::Read;
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let hits = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (thread_hits, thread_stop) = (Arc::clone(&hits), Arc::clone(&stop));
        std::thread::spawn(move || {
            for sock in listener.incoming() {
                if thread_stop.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(mut sock) = sock else { continue };
                let mut request = Vec::new();
                let mut buf = [0u8; 4096];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => request.extend_from_slice(&buf[..n]),
                    }
                }
                let request = String::from_utf8_lossy(&request);
                let path = request.split_whitespace().nth(1).unwrap_or("").to_string();
                thread_hits.lock().unwrap().push(path.clone());
                let not_found = Route {
                    status: 404,
                    headers: Vec::new(),
                    body: b"not found".to_vec(),
                };
                let route = routes.get(&path).unwrap_or(&not_found);
                let mut head = format!(
                    "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                    route.status,
                    route.body.len()
                );
                for (name, value) in &route.headers {
                    head.push_str(&format!("{name}: {value}\r\n"));
                }
                head.push_str("\r\n");
                let _ = sock.write_all(head.as_bytes());
                let _ = sock.write_all(&route.body);
                let _ = sock.flush();
            }
        });
        TestServer { addr, hits, stop }
    }
}
