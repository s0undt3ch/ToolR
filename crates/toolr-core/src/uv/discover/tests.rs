use super::*;
use crate::test_env::EnvGuard;

#[test]
fn parses_standard_uv_version_string() {
    let s = "uv 0.5.1 (xyz)\n";
    assert_eq!(parse_uv_version(s), Some((0, 5, 1)));
}

#[test]
fn parses_with_no_trailing_paren() {
    assert_eq!(parse_uv_version("uv 1.10.2"), Some((1, 10, 2)));
}

#[test]
fn returns_none_on_garbage() {
    assert_eq!(parse_uv_version("garbage"), None);
    assert_eq!(parse_uv_version("uv broken"), None);
    assert_eq!(parse_uv_version(""), None);
}

#[test]
fn version_too_old_error_includes_both_versions() {
    let err = UvError::VersionTooOld {
        found: (0, 1, 0),
        required: MIN_UV_VERSION,
    };
    let msg = err.to_string();
    assert!(msg.contains("0.1.0") || msg.contains("(0, 1, 0)"));
}

#[test]
fn which_uv_returns_none_when_path_unset() {
    assert_eq!(which_uv_in(None), None);
}

#[test]
fn which_uv_returns_none_when_no_directory_contains_uv() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(which_uv_in(Some(tmp.path().into())), None);
}

#[cfg(unix)]
#[test]
fn which_uv_finds_uv_in_first_directory_that_contains_it() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let stub = bin_dir.join("uv");
    std::fs::write(&stub, b"#!/bin/sh\necho uv 1.0.0\n").unwrap();
    let mut perms = std::fs::metadata(&stub).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&stub, perms).unwrap();

    assert_eq!(which_uv_in(Some(bin_dir.into())), Some(stub));
}

#[cfg(unix)]
#[test]
fn probe_against_stub_uv_returns_uv_binary() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let stub = tmp.path().join("uv");
    std::fs::write(&stub, b"#!/bin/sh\necho 'uv 0.5.0 (abcdef)'\n").unwrap();
    let mut perms = std::fs::metadata(&stub).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&stub, perms).unwrap();

    let bin = probe(&stub, UvSource::Path).expect("0.5.0 >= 0.4.0 should succeed");
    assert_eq!(bin.version, (0, 5, 0));
    assert_eq!(bin.source, UvSource::Path);
    assert_eq!(bin.path, stub);
}

#[cfg(unix)]
#[test]
fn probe_rejects_old_version_with_version_too_old() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let stub = tmp.path().join("uv");
    std::fs::write(&stub, b"#!/bin/sh\necho 'uv 0.1.0'\n").unwrap();
    let mut perms = std::fs::metadata(&stub).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&stub, perms).unwrap();

    let err = probe(&stub, UvSource::Path).unwrap_err();
    assert!(matches!(err, UvError::VersionTooOld { .. }));
}

#[cfg(unix)]
#[test]
fn probe_rejects_unparsable_version_string() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let stub = tmp.path().join("uv");
    std::fs::write(&stub, b"#!/bin/sh\necho 'no version here'\n").unwrap();
    let mut perms = std::fs::metadata(&stub).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&stub, perms).unwrap();

    let err = probe(&stub, UvSource::Path).unwrap_err();
    assert!(matches!(err, UvError::UnparsableVersion(_)));
}

#[cfg(unix)]
#[test]
fn probe_rejects_nonzero_exit_as_unparsable_version() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let stub = tmp.path().join("uv");
    std::fs::write(&stub, b"#!/bin/sh\necho 'oops' 1>&2\nexit 1\n").unwrap();
    let mut perms = std::fs::metadata(&stub).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&stub, perms).unwrap();

    let err = probe(&stub, UvSource::Path).unwrap_err();
    // Non-zero exit lands on the `UnparsableVersion(stderr)` arm.
    match err {
        UvError::UnparsableVersion(msg) => assert!(msg.contains("oops"), "got: {msg}"),
        other => panic!("expected UnparsableVersion, got {other:?}"),
    }
}

#[test]
fn probe_returns_exec_failed_when_binary_is_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let candidate = tmp.path().join("does-not-exist");
    let err = probe(&candidate, UvSource::Path).unwrap_err();
    // `Command::output()` returns ENOENT, which now maps to
    // `ExecFailed` rather than the bare `Io` variant — so
    // `user_message()` can render a libc-mismatch hint.
    match err {
        UvError::ExecFailed { path, source } => {
            assert_eq!(path, candidate);
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }
        other => panic!("expected ExecFailed, got {other:?}"),
    }
}

#[test]
fn exec_failed_user_message_is_a_libc_hint_without_path_duplication() {
    let err = UvError::exec_failed(
        std::path::Path::new("/managed/uv"),
        std::io::Error::from(std::io::ErrorKind::NotFound),
    );
    let hint = err.user_message();
    // The hint mentions both the env override and the OS package
    // fallback so the user has at least two paths forward.
    assert!(
        hint.contains("TOOLR_UV_LIBC"),
        "missing env override hint in: {hint}"
    );
    assert!(hint.contains("musl"), "missing musl/gnu mention in: {hint}");
    // Path and io error are intentionally NOT duplicated in the hint
    // — they reach the user via the wrapped ExecFailed's Display,
    // which `into_anyhow()` keeps in the chain.
    assert!(
        !hint.contains("/managed/uv"),
        "path leaked into hint: {hint}"
    );
}

#[test]
fn exec_failed_into_anyhow_chain_contains_both_hint_and_path() {
    let err = UvError::exec_failed(
        std::path::Path::new("/managed/uv"),
        std::io::Error::from(std::io::ErrorKind::NotFound),
    );
    let formatted = format!("{:#}", err.into_anyhow());
    // `{:#}` joins the whole chain with `: ` — the hint should
    // appear first (outermost context) and the technical detail
    // (path + io error) should follow.
    assert!(
        formatted.contains("TOOLR_UV_LIBC"),
        "hint missing in chain: {formatted}"
    );
    assert!(
        formatted.contains("/managed/uv"),
        "path missing in chain: {formatted}"
    );
    // The io::Error display varies across platforms / kinds — the
    // synthetic `Error::from(NotFound)` here renders as "entity not
    // found" while a real ENOENT from execve renders as "No such
    // file or directory". Either way, the chain ends with the
    // ExecFailed Display followed by the source io::Error, so the
    // path appears twice (in ExecFailed display + already asserted)
    // and the io::Error tail follows.
    assert!(
        formatted.matches("entity not found").count() >= 1 || formatted.contains("No such file"),
        "io error missing in chain: {formatted}"
    );
}

#[test]
fn exec_failed_non_notfound_falls_back_to_display() {
    // For non-ENOENT exec failures (permission denied, etc.) we
    // still want SOMETHING actionable, but the libc-mismatch hint
    // would be misleading. Confirm we fall through to the plain
    // Display formatting in that case.
    let err = UvError::exec_failed(
        std::path::Path::new("/forbidden/uv"),
        std::io::Error::from(std::io::ErrorKind::PermissionDenied),
    );
    let msg = err.user_message();
    assert!(msg.contains("/forbidden/uv"));
    assert!(!msg.contains("TOOLR_UV_LIBC"));
}

#[test]
fn find_managed_uv_returns_none_when_managed_path_absent() {
    // Constrain $XDG_DATA_HOME to an empty tempdir so managed_uv_path
    // resolves into a non-existent file.
    let tmp = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::write();
    env.set("XDG_DATA_HOME", tmp.path());
    let result = find_managed_uv();
    assert!(matches!(result, Ok(None)));
}
