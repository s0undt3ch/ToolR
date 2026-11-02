//! Filesystem helpers for unit tests.

use std::path::{Path, PathBuf};

/// Writes `dir/name` with `body` and, on unix, permissions `mode`.
pub(crate) fn write_executable(dir: &Path, name: &str, body: &str, mode: u32) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    #[cfg(not(unix))]
    let _ = mode;
    path
}

/// `dir` relative to the process cwd: `..` up to `/`, then down. Both sides are
/// canonicalised (macOS `/var` is a symlink). Assumes no test changes the cwd.
#[cfg(unix)]
pub(crate) fn relative_to_cwd(dir: &Path) -> PathBuf {
    use std::path::Component;
    let normal = |p: PathBuf| -> Vec<_> {
        p.components()
            .filter(|c| matches!(c, Component::Normal(_)))
            .map(|c| c.as_os_str().to_owned())
            .collect()
    };
    let cwd = normal(std::env::current_dir().unwrap().canonicalize().unwrap());
    let mut rel: PathBuf = std::iter::repeat_n("..", cwd.len()).collect();
    rel.extend(normal(dir.canonicalize().unwrap()));
    rel
}

/// Run `f` with `dir` at permissions `mode`, then put back `0o755`. `None` as root, where
/// `chmod 000` stops nothing; tests assert with `is_none_or` so they hold there too.
#[cfg(unix)]
pub(crate) fn with_mode<T>(dir: &Path, mode: u32, f: impl FnOnce() -> T) -> Option<T> {
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: geteuid has no preconditions.
    let is_root = unsafe { libc::geteuid() == 0 };
    (!is_root).then(|| {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).unwrap();
        let out = f();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        out
    })
}

/// Whether the file system holding `dir` ignores case, probed by creating a file in it.
pub(crate) fn ignores_case(dir: &Path) -> bool {
    std::fs::write(dir.join("case-probe"), "").unwrap();
    let ignores = dir.join("CASE-PROBE").exists();
    std::fs::remove_file(dir.join("case-probe")).unwrap();
    ignores
}
