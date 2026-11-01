//! `PATH` lookups that skip empty and relative entries: those resolve against
//! the cwd, usually the repo toolr serves, so a file in it could run instead.

use std::ffi::OsString;
use std::path::PathBuf;

/// `path` with every empty and relative entry removed, or `None` when no
/// absolute entry is left. Never `Some("")`: an empty `PATH` reads as the cwd.
pub(crate) fn absolute_search_path(path: Option<OsString>) -> Option<OsString> {
    let kept: Vec<PathBuf> = std::env::split_paths(&path?)
        .filter(|entry| entry.is_absolute())
        .collect();
    if kept.is_empty() {
        return None;
    }
    std::env::join_paths(kept).ok()
}

/// The first executable `name` (plus `.exe` on Windows) in an absolute `$PATH`
/// directory. An unset `PATH` finds nothing: libc's default search path isn't
/// emulated. On Windows only `.exe` is tried; `PATHEXT` isn't read.
/// For uv use `uv::find_uv_on_path`, which also checks the version.
pub fn find_on_path(name: &str) -> Option<PathBuf> {
    find_on_path_in(name, std::env::var_os("PATH"))
}

pub(crate) fn find_on_path_in(name: &str, path: Option<OsString>) -> Option<PathBuf> {
    let basename = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(&absolute_search_path(path)?)
        .map(|entry| entry.join(&basename))
        .find(|candidate| is_executable_file(candidate))
}

/// A regular file this process may execute, judged by `access(X_OK)` like
/// `execvp`, not by mode bits a different owner would need.
#[cfg(unix)]
fn is_executable_file(path: &std::path::Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    path.is_file()
        && std::ffi::CString::new(path.as_os_str().as_bytes()).is_ok_and(|c_path| {
            // SAFETY: `c_path` is a valid NUL-terminated string for the call.
            unsafe { libc::access(c_path.as_ptr(), libc::X_OK) == 0 }
        })
}

#[cfg(not(unix))]
fn is_executable_file(path: &std::path::Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::test_fs::write_executable;

    fn abs(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    fn joined(entries: &[&Path]) -> OsString {
        std::env::join_paths(entries).unwrap()
    }

    fn split(path: Option<OsString>) -> Vec<PathBuf> {
        std::env::split_paths(&path.expect("an absolute entry survives")).collect()
    }

    #[test]
    fn absolute_search_path_drops_empty_and_relative_entries() {
        let (a, b) = (abs("a"), abs("b"));
        let path = joined(&[
            &a,
            Path::new(""),
            Path::new("."),
            Path::new("bin"),
            Path::new("~/bin"),
            &b,
        ]);
        assert_eq!(split(absolute_search_path(Some(path))), vec![a, b]);
    }

    #[test]
    fn absolute_search_path_keeps_absolute_entries_in_order() {
        let (a, b) = (abs("a"), abs("b"));
        let path = joined(&[&b, &a]);
        assert_eq!(split(absolute_search_path(Some(path))), vec![b, a]);
    }

    #[test]
    fn absolute_search_path_returns_none_for_an_empty_path() {
        assert_eq!(absolute_search_path(Some("".into())), None);
    }

    #[test]
    fn absolute_search_path_returns_none_for_a_dot_path() {
        assert_eq!(absolute_search_path(Some(".".into())), None);
    }

    #[test]
    #[cfg(unix)]
    fn absolute_search_path_returns_none_for_a_lone_separator() {
        assert_eq!(absolute_search_path(Some(":".into())), None);
    }

    #[test]
    fn absolute_search_path_returns_none_for_unset_path() {
        assert_eq!(absolute_search_path(None), None);
    }

    #[test]
    #[cfg(unix)]
    fn find_on_path_in_ignores_a_uv_reached_through_a_relative_entry() {
        let dir = tempfile::tempdir().unwrap();
        write_executable(dir.path(), "uv", "", 0o755);
        let rel = crate::test_fs::relative_to_cwd(dir.path());
        assert!(rel.is_relative() && rel.join("uv").is_file(), "{rel:?}");
        let path = joined(&[Path::new(""), &rel]);
        assert_eq!(find_on_path_in("uv", Some(path)), None);
    }

    /// Finds `python3` on a `PATH` whose first copy has `mode` and whose second
    /// is 0o755. Returns (first, second, found).
    #[cfg(unix)]
    fn first_has_mode(mode: u32) -> (PathBuf, PathBuf, Option<PathBuf>) {
        let (one, two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let first = write_executable(one.path(), "python3", "", mode);
        let second = write_executable(two.path(), "python3", "", 0o755);
        let found = find_on_path_in("python3", Some(joined(&[one.path(), two.path()])));
        (first, second, found)
    }

    #[test]
    #[cfg(unix)]
    fn find_on_path_in_skips_a_file_that_is_not_executable() {
        let (_, second, found) = first_has_mode(0o644);
        assert_eq!(found, Some(second));
    }

    #[test]
    #[cfg(unix)]
    fn find_on_path_in_skips_a_file_its_owner_cannot_run() {
        let (first, second, found) = first_has_mode(0o655);
        // root may run anything with an exec bit set, as execve allows.
        let is_root = unsafe { libc::geteuid() } == 0;
        assert_eq!(found, Some(if is_root { first } else { second }));
    }

    #[test]
    fn find_on_path_in_skips_directories_without_the_file() {
        let empty = tempfile::tempdir().unwrap();
        let full = tempfile::tempdir().unwrap();
        let name = format!("tool{}", std::env::consts::EXE_SUFFIX);
        let exe = write_executable(full.path(), &name, "", 0o755);
        let path = joined(&[empty.path(), full.path()]);
        assert_eq!(find_on_path_in("tool", Some(path)), Some(exe));
    }
}
