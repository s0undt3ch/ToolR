use super::*;
use crate::test_env::EnvGuard;

#[test]
fn min_uv_version_is_a_real_tuple() {
    let (maj, _, _) = MIN_UV_VERSION;
    assert!(maj < 100, "min uv major version should be plausible");
}

#[test]
fn data_dir_resolves_or_returns_none_on_exotic_envs() {
    let _env = EnvGuard::read();
    // We don't assert a specific path: this just exercises the call.
    let _ = toolr_data_dir();
    let _ = toolr_cache_dir();
    let _ = managed_uv_path();
}

#[test]
fn toolr_data_dir_uses_xdg_data_home_when_set() {
    let mut env = EnvGuard::write();
    // `temp_dir()` is absolute on every platform; a literal `/tmp/…` has no drive on Windows.
    let base = std::env::temp_dir().join("test-xdg-data");
    env.set("XDG_DATA_HOME", &base);
    let result = toolr_data_dir();
    assert_eq!(result, Some(base.join("toolr")));
}

#[test]
fn toolr_data_dir_treats_empty_xdg_data_home_as_unset() {
    let mut env = EnvGuard::write();
    env.set("XDG_DATA_HOME", "");
    let result = toolr_data_dir();
    assert_eq!(result, dirs::data_dir().map(|d| d.join("toolr")));
    assert!(result.expect("platform data dir").is_absolute());
}

#[test]
fn toolr_state_dir_treats_empty_xdg_state_home_as_unset() {
    let mut env = EnvGuard::write();
    env.remove("XDG_STATE_HOME");
    let expected = toolr_state_dir();
    env.set("XDG_STATE_HOME", "");
    assert_eq!(toolr_state_dir(), expected);
    assert!(expected.expect("platform state dir").is_absolute());
}

#[test]
fn toolr_cache_dir_treats_empty_xdg_cache_home_as_unset() {
    let mut env = EnvGuard::write();
    env.set("XDG_CACHE_HOME", "");
    let result = toolr_cache_dir();
    assert_eq!(result, dirs::cache_dir().map(|d| d.join("toolr")));
    assert!(result.expect("platform cache dir").is_absolute());
}

#[test]
fn managed_uv_path_ignores_empty_xdg_data_home() {
    let mut env = EnvGuard::write();
    env.set("XDG_DATA_HOME", "");
    let p = managed_uv_path().expect("platform data dir");
    assert!(p.is_absolute());
    assert!(p.ends_with("toolr/bin/uv") || p.ends_with("toolr/bin/uv.exe"));
}

#[test]
fn relative_xdg_dirs_are_ignored() {
    let mut env = EnvGuard::write();
    env.remove("XDG_DATA_HOME");
    env.remove("XDG_STATE_HOME");
    env.remove("XDG_CACHE_HOME");
    let (data, state, cache) = (toolr_data_dir(), toolr_state_dir(), toolr_cache_dir());
    env.set("XDG_DATA_HOME", "rel/data");
    env.set("XDG_STATE_HOME", "rel/state");
    env.set("XDG_CACHE_HOME", "rel/cache");
    assert_eq!(toolr_data_dir(), data);
    assert_eq!(toolr_state_dir(), state);
    assert_eq!(toolr_cache_dir(), cache);
}

#[test]
fn toolr_cache_dir_uses_xdg_cache_home_when_set() {
    let mut env = EnvGuard::write();
    let base = std::env::temp_dir().join("test-xdg-cache");
    env.set("XDG_CACHE_HOME", &base);
    let result = toolr_cache_dir();
    assert_eq!(result, Some(base.join("toolr")));
}

#[test]
fn managed_uv_path_lives_under_data_dir_bin() {
    let mut env = EnvGuard::write();
    let base = std::env::temp_dir().join("test-xdg-data");
    env.set("XDG_DATA_HOME", &base);
    let result = managed_uv_path();
    // `uv_basename()` is platform-specific; just assert the directory
    // structure so this still passes on Windows runners.
    let p = result.expect("XDG_DATA_HOME set → Some");
    assert!(p.starts_with(base.join("toolr").join("bin")));
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
    assert!(UvError::UserRefusedInstall.to_string().contains("declined"));
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
    assert!(
        UvError::SyncFailed(None)
            .to_string()
            .contains("uv sync failed")
    );
}

#[test]
fn extract_error_names_the_archive_and_points_at_manual_install() {
    let err = UvError::extract(Path::new("uv.zip"), "bad header");
    assert!(err.to_string().contains("uv.zip"));
    assert!(
        err.user_message()
            .contains("https://docs.astral.sh/uv/getting-started/installation/")
    );
    let chain = format!("{:#}", err.into_anyhow());
    assert!(chain.contains("bad header"), "got: {chain}");
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
    assert!(s.contains("TOOLR_AUTO_SYNC"));
    // Anti-regression: must not double-prefix `toolr: ` — main.rs
    // adds that once at the CLI surface.
    assert!(!s.starts_with("toolr:"), "actual: {s}");
}

#[test]
fn user_message_for_http_points_at_a_manual_install() {
    let s = UvError::Http("unreachable".into()).user_message();
    assert!(s.contains("couldn't download uv"), "actual: {s}");
    assert!(s.contains("https://docs.astral.sh/uv/"), "actual: {s}");
}

#[test]
fn install_error_names_the_cause_and_points_at_manual_install() {
    let err = UvError::Install("no uv binary".into());
    assert!(err.to_string().contains("no uv binary"), "{err}");
    let s = err.user_message();
    assert!(
        s.contains("couldn't install the uv it downloaded"),
        "actual: {s}"
    );
    assert!(s.contains("https://docs.astral.sh/uv/"), "actual: {s}");
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

#[test]
fn verification_errors_name_the_details_and_point_at_manual_install() {
    let unverified = UvError::Unverified {
        asset: "uv-a.tar.gz".into(),
        reason: "no checksum".into(),
    };
    let mismatch = UvError::ChecksumMismatch {
        asset: "uv-a.tar.gz".into(),
        tag: "9.9.9".into(),
        expected: "aa".into(),
        actual: "bb".into(),
    };
    assert!(unverified.to_string().contains("uv-a.tar.gz"));
    assert!(unverified.to_string().contains("no checksum"));
    for needle in ["uv-a.tar.gz", "9.9.9", "aa", "bb"] {
        assert!(mismatch.to_string().contains(needle), "{mismatch}");
    }
    for err in [unverified, mismatch] {
        let s = err.user_message();
        assert!(
            s.contains("https://docs.astral.sh/uv/getting-started/installation/"),
            "actual: {s}"
        );
        assert!(!s.starts_with("toolr:"), "actual: {s}");
    }
}
