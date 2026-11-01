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
