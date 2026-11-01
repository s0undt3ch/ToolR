//! `PATH` lookups that skip empty and relative entries: those resolve against
//! the cwd, usually the repo toolr serves, so a file in it could run instead.

use std::ffi::OsString;
use std::path::PathBuf;

/// `path` with every empty and relative entry removed, or `None` when no
/// absolute entry is left. Never `Some("")`: an empty `PATH` reads as the cwd.
pub fn absolute_search_path(path: Option<OsString>) -> Option<OsString> {
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
/// emulated.
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

/// `dir` as a path relative to the process cwd, so a test can point a
/// relative `PATH` entry at it without changing the cwd.
#[cfg(all(test, unix))]
pub(crate) fn relative_to_cwd(dir: &std::path::Path) -> PathBuf {
    use std::path::Component;
    let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
    let dir = dir.canonicalize().unwrap();
    let ups = cwd
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .count();
    let mut rel: PathBuf = std::iter::repeat_n("..", ups).collect();
    rel.extend(
        dir.components()
            .filter(|c| matches!(c, Component::Normal(_))),
    );
    rel
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[cfg(unix)]
    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

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
    fn absolute_search_path_returns_none_when_nothing_is_absolute() {
        assert_eq!(absolute_search_path(Some("".into())), None);
        assert_eq!(absolute_search_path(Some(".".into())), None);
        #[cfg(unix)]
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
        let uv = dir.path().join("uv");
        std::fs::write(&uv, b"").unwrap();
        set_mode(&uv, 0o755);
        let rel = relative_to_cwd(dir.path());
        assert!(rel.is_relative() && rel.join("uv").is_file(), "{rel:?}");
        let path = joined(&[Path::new(""), &rel]);
        assert_eq!(find_on_path_in("uv", Some(path)), None);
    }

    #[test]
    #[cfg(unix)]
    fn find_on_path_in_skips_a_file_its_owner_cannot_run() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let owner_cannot_run = first.path().join("python3");
        std::fs::write(&owner_cannot_run, b"").unwrap();
        set_mode(&owner_cannot_run, 0o655);
        let exe = second.path().join("python3");
        std::fs::write(&exe, b"").unwrap();
        set_mode(&exe, 0o755);
        let path = joined(&[first.path(), second.path()]);
        // root may run anything with an exec bit set, as execve allows.
        let expected = if unsafe { libc::geteuid() } == 0 {
            owner_cannot_run
        } else {
            exe
        };
        assert_eq!(find_on_path_in("python3", Some(path)), Some(expected));
    }

    #[test]
    #[cfg(unix)]
    fn find_on_path_in_skips_a_file_that_is_not_executable() {
        use std::os::unix::fs::PermissionsExt;
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let plain = first.path().join("python3");
        std::fs::write(&plain, b"").unwrap();
        std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o644)).unwrap();
        let exe = second.path().join("python3");
        std::fs::write(&exe, b"").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = joined(&[first.path(), second.path()]);
        assert_eq!(find_on_path_in("python3", Some(path)), Some(exe));
    }

    #[test]
    fn find_on_path_in_skips_directories_without_the_file() {
        let empty = tempfile::tempdir().unwrap();
        let full = tempfile::tempdir().unwrap();
        let exe = full
            .path()
            .join(format!("tool{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&exe, b"").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = joined(&[empty.path(), full.path()]);
        assert_eq!(find_on_path_in("tool", Some(path)), Some(exe));
    }
}
