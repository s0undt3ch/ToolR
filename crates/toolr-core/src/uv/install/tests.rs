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
fn auto_sync_env_forces_install_even_without_tty() {
    let consent = ConsentMode {
        auto_sync_env: true,
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
    assert!(!ConsentMode::from_env(false).auto_sync_env);

    env.set("TOOLR_AUTO_SYNC", "1");
    let on = ConsentMode::from_env(false);
    assert!(on.auto_sync_env);
    assert!(!on.yes_flag);
    env.remove("TOOLR_AUTO_SYNC");

    env.set("TOOLR_AUTO_INSTALL_UV", "yes");
    assert!(ConsentMode::from_env(false).auto_sync_env);

    // A false spelling is off, and so is anything that isn't a spelling at all.
    env.set("TOOLR_AUTO_INSTALL_UV", "0");
    env.set("TOOLR_AUTO_SYNC", "maybe");
    assert!(!ConsentMode::from_env(false).auto_sync_env);

    // Once set, the new name decides alone, even empty or unparsable.
    env.set("TOOLR_AUTO_INSTALL_UV", "1");
    for value in ["0", "", "maybe"] {
        env.set("TOOLR_AUTO_SYNC", value);
        assert!(!ConsentMode::from_env(false).auto_sync_env, "{value:?}");
    }
    env.remove("TOOLR_AUTO_SYNC");
    assert!(ConsentMode::from_env(false).auto_sync_env);
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
