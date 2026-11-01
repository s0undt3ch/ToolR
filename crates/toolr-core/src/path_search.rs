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

/// The first `name` (plus `.exe` on Windows) in an absolute `$PATH` directory.
pub fn find_on_path(name: &str) -> Option<PathBuf> {
    find_on_path_in(name, std::env::var_os("PATH"))
}

pub(crate) fn find_on_path_in(name: &str, path: Option<OsString>) -> Option<PathBuf> {
    let basename = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(&absolute_search_path(path)?)
        .map(|entry| entry.join(&basename))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

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
    fn find_on_path_in_returns_none_when_nothing_is_absolute() {
        assert_eq!(find_on_path_in("uv", Some(":.:bin".into())), None);
    }

    #[test]
    fn find_on_path_in_skips_directories_without_the_file() {
        let empty = tempfile::tempdir().unwrap();
        let full = tempfile::tempdir().unwrap();
        let exe = full
            .path()
            .join(format!("tool{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&exe, b"").unwrap();
        let path = joined(&[empty.path(), full.path()]);
        assert_eq!(find_on_path_in("tool", Some(path)), Some(exe));
    }
}
