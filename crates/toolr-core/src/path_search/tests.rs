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
