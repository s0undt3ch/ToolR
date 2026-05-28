//! Locate a working uv binary on the host.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{MIN_UV_VERSION, UvBinary, UvError, UvSource, managed_uv_path};

/// Try to find a usable `uv` on `$PATH`. Returns `Ok(None)` if uv is not
/// on PATH at all (so the caller can fall through to the managed path);
/// `Err(UvError::VersionTooOld { .. })` if it exists but is too old.
pub fn find_uv_on_path() -> Result<Option<UvBinary>, UvError> {
    let candidate = which_uv()?;
    let Some(path) = candidate else {
        return Ok(None);
    };
    probe(&path, UvSource::Path).map(Some)
}

/// Try to find a toolr-managed uv at `$XDG_DATA_HOME/toolr/bin/uv`.
pub fn find_managed_uv() -> Result<Option<UvBinary>, UvError> {
    let Some(path) = managed_uv_path() else {
        return Ok(None);
    };
    if !path.is_file() {
        return Ok(None);
    }
    probe(&path, UvSource::Managed).map(Some)
}

/// Run `<path> --version`, parse output, validate against the minimum.
pub fn probe(path: &Path, source: UvSource) -> Result<UvBinary, UvError> {
    // `Command::output()` returns `ErrorKind::NotFound` when the binary
    // exists on disk but `execve` reports ENOENT — almost always a libc
    // mismatch (missing dynamic loader). Tag that distinctly so the
    // user-facing error spells out the recovery path instead of
    // bottoming out as "I/O error: No such file or directory".
    let output = Command::new(path)
        .arg("--version")
        .output()
        .map_err(|e| UvError::exec_failed(path, e))?;
    if !output.status.success() {
        return Err(UvError::UnparsableVersion(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let version = parse_uv_version(&stdout)
        .ok_or_else(|| UvError::UnparsableVersion(stdout.clone()))?;
    if version < MIN_UV_VERSION {
        return Err(UvError::VersionTooOld {
            found: version,
            required: MIN_UV_VERSION,
        });
    }
    Ok(UvBinary {
        path: path.to_path_buf(),
        version,
        source,
    })
}

/// `uv --version` prints something like `uv 0.5.1 (...)`. Extract the
/// three-component numeric prefix.
pub fn parse_uv_version(output: &str) -> Option<(u32, u32, u32)> {
    let line = output.lines().next()?;
    let words = line.split_whitespace().collect::<Vec<_>>();
    // Find the first token that looks like a `1.2.3` (allow a trailing
    // alphanumeric suffix, which we discard).
    for word in words {
        let trimmed: String = word
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let parts: Vec<&str> = trimmed.split('.').collect();
        if parts.len() != 3 {
            continue;
        }
        let (Ok(a), Ok(b), Ok(c)) = (
            parts[0].parse::<u32>(),
            parts[1].parse::<u32>(),
            parts[2].parse::<u32>(),
        ) else {
            continue;
        };
        return Some((a, b, c));
    }
    None
}

/// Naive PATH scan that returns the first `uv` (or `uv.exe` on Windows)
/// found.
pub fn which_uv() -> Result<Option<PathBuf>, UvError> {
    let basename = if cfg!(windows) { "uv.exe" } else { "uv" };
    let Some(path) = std::env::var_os("PATH") else {
        return Ok(None);
    };
    for entry in std::env::split_paths(&path) {
        let candidate = entry.join(basename);
        if candidate.is_file() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Mutating `PATH` for the duration of a test is unavoidable for
    /// `which_uv` / `find_uv_on_path` coverage. Serialise so cargo's
    /// parallel runner doesn't race.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_path<R>(value: Option<&str>, f: impl FnOnce() -> R) -> R {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var_os("PATH");
        // SAFETY: ENV_LOCK serialises the only tests in this module
        // that touch PATH.
        unsafe {
            match value {
                Some(v) => std::env::set_var("PATH", v),
                None => std::env::remove_var("PATH"),
            }
        }
        let r = f();
        unsafe {
            match prev {
                Some(v) => std::env::set_var("PATH", v),
                None => std::env::remove_var("PATH"),
            }
        }
        r
    }

    #[test]
    fn which_uv_returns_none_when_path_unset() {
        let result = with_path(None, which_uv);
        assert!(matches!(result, Ok(None)));
    }

    #[test]
    fn which_uv_returns_none_when_no_directory_contains_uv() {
        let tmp = tempfile::tempdir().unwrap();
        let path_value = tmp.path().to_string_lossy().into_owned();
        let result = with_path(Some(&path_value), which_uv);
        assert!(matches!(result, Ok(None)));
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

        let path_value = bin_dir.to_string_lossy().into_owned();
        let found = with_path(Some(&path_value), which_uv).unwrap();
        assert_eq!(found, Some(stub));
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
        assert!(hint.contains("TOOLR_UV_LIBC"), "missing env override hint in: {hint}");
        assert!(hint.contains("musl"), "missing musl/gnu mention in: {hint}");
        // Path and io error are intentionally NOT duplicated in the hint
        // — they reach the user via the wrapped ExecFailed's Display,
        // which `into_anyhow()` keeps in the chain.
        assert!(!hint.contains("/managed/uv"), "path leaked into hint: {hint}");
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
        assert!(formatted.contains("TOOLR_UV_LIBC"), "hint missing in chain: {formatted}");
        assert!(formatted.contains("/managed/uv"), "path missing in chain: {formatted}");
        // The io::Error display varies across platforms / kinds — the
        // synthetic `Error::from(NotFound)` here renders as "entity not
        // found" while a real ENOENT from execve renders as "No such
        // file or directory". Either way, the chain ends with the
        // ExecFailed Display followed by the source io::Error, so the
        // path appears twice (in ExecFailed display + already asserted)
        // and the io::Error tail follows.
        assert!(
            formatted.matches("entity not found").count() >= 1
                || formatted.contains("No such file"),
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
        // resolves into a non-existent file. Use ENV_LOCK to avoid
        // colliding with sibling tests that also touch env vars.
        let tmp = tempfile::tempdir().unwrap();
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var_os("XDG_DATA_HOME");
        // SAFETY: serialised by the module-level ENV_LOCK.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path());
        }
        let result = find_managed_uv();
        unsafe {
            match prev {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
        }
        assert!(matches!(result, Ok(None)));
    }
}
