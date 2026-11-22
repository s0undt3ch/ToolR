//! Decision logic + executor for installing a toolr-managed uv.

use std::fs;
use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;

use super::{UvBinary, UvError, UvSource, managed_uv_path, probe};
use crate::env::env_truthy;
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
    pub auto_sync_env: bool,
    /// Caller must never prompt (`--quiet`, scripts, shell-enter hooks). If uv
    /// isn't available and `yes_flag`/`auto_sync_env` haven't pre-authorised
    /// an install, return `Refuse`; the caller decides what to print.
    pub never_prompt: bool,
}

impl ConsentMode {
    /// Consent from the environment, plus the caller's `never_prompt` (set by
    /// `--quiet`: a scripted run must not stop to ask).
    pub fn from_env(never_prompt: bool) -> Self {
        Self {
            yes_flag: false,
            auto_sync_env: auto_sync_from_env(),
            never_prompt,
        }
    }
}

/// `TOOLR_AUTO_SYNC` decides whenever it is set (an unparsable or empty value is off), so
/// `TOOLR_AUTO_SYNC=0` beats a leftover `TOOLR_AUTO_INSTALL_UV=1`. The older
/// `TOOLR_AUTO_INSTALL_UV` counts only while `TOOLR_AUTO_SYNC` is unset.
fn auto_sync_from_env() -> bool {
    if std::env::var_os("TOOLR_AUTO_SYNC").is_some() {
        env_truthy("TOOLR_AUTO_SYNC")
    } else {
        env_truthy("TOOLR_AUTO_INSTALL_UV")
    }
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
    if consent.yes_flag || consent.auto_sync_env {
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
mod tests;
