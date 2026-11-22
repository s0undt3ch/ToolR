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
mod tests;
