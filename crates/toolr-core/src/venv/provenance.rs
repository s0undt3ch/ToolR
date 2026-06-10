//! Verify a resolved interpreter is one toolr is allowed to execute.
//!
//! Manifest building never spawns Python, so the only place repository
//! code can run is explicit command dispatch. This module gates that
//! step: an interpreter living inside the repository tree is executed
//! only when toolr itself provisioned it (recorded in the out-of-repo
//! `meta.json` sidecar). A committed fake `tools/.venv` therefore has no
//! provenance record and is always refused.

use std::path::{Path, PathBuf};

use crate::cache::meta::Meta;
use crate::hash::hash_file;
use crate::uv::toolr_cache_dir;

/// Why an interpreter was rejected.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProvenanceError {
    #[error(
        "refusing to run {0}: it lives inside the repository and was not provisioned by toolr — run `toolr project venv sync`"
    )]
    UntrustedInRepo(PathBuf),
}

/// Returns `Ok(())` if `interpreter` is safe to execute for `repo_root`.
///
/// Trusted when EITHER:
/// - the canonical interpreter path is under toolr's cache dir
///   (`$XDG_CACHE_HOME/toolr/`), which a repo cannot write to, OR
/// - it is inside the repo tree but matches the provenance record in the
///   out-of-repo `meta.json` for this repo (path + content hash).
///
/// An interpreter outside both the repo tree and the cache dir (e.g. a
/// system Python for a project that never opted into the venv layer) is
/// allowed — the repo could not have planted it.
pub fn verify_interpreter(
    interpreter: &Path,
    repo_root: &Path,
    cache_dir_for_repo: Option<&Path>,
) -> Result<(), ProvenanceError> {
    let canon = interpreter
        .canonicalize()
        .unwrap_or_else(|_| interpreter.to_path_buf());

    // 1. Under toolr's cache dir → trusted (repo can't write there).
    if let Some(cache_root) = toolr_cache_dir() {
        let cache_root = cache_root.canonicalize().unwrap_or(cache_root);
        if canon.starts_with(&cache_root) {
            return Ok(());
        }
    }

    // 2. Inside the repo tree → require a matching provenance record.
    let repo_canon = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    if canon.starts_with(&repo_canon) {
        if let Some(cache_dir) = cache_dir_for_repo {
            if let Ok(meta) = Meta::load(cache_dir) {
                if meta.interpreter_path.as_deref() == Some(canon.as_path())
                    && meta.interpreter_hash.as_deref() == hash_file(&canon).ok().as_deref()
                {
                    return Ok(());
                }
            }
        }
        return Err(ProvenanceError::UntrustedInRepo(canon));
    }

    // 3. Outside repo, outside cache (e.g. system python for non-venv repos) → allowed.
    Ok(())
}

// All tests here build executable shell-script interpreters (Unix-only:
// `from_mode` / `#!/bin/sh`). Gate the whole module so Windows doesn't see
// the Unix-only imports/helpers (avoids E0433/E0599 and unused-import noise).
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn write_exec(p: &Path, body: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn rejects_unrecorded_in_repo_interpreter() {
        let repo = TempDir::new().unwrap();
        let py = repo.path().join("tools/.venv/bin/python");
        write_exec(&py, "#!/bin/sh\n");
        let cache = TempDir::new().unwrap(); // empty: no meta.json
        let err = verify_interpreter(&py, repo.path(), Some(cache.path())).unwrap_err();
        assert!(matches!(err, ProvenanceError::UntrustedInRepo(_)));
    }

    #[test]
    #[cfg(unix)]
    fn accepts_recorded_in_repo_interpreter() {
        let repo = TempDir::new().unwrap();
        let py = repo.path().join("tools/.venv/bin/python");
        write_exec(&py, "#!/bin/sh\necho hi\n");
        let canon = py.canonicalize().unwrap();
        let cache = TempDir::new().unwrap();
        Meta::new(repo.path(), "0", "3.13")
            .with_interpreter(canon.clone(), hash_file(&canon).unwrap())
            .write(cache.path())
            .unwrap();
        assert!(verify_interpreter(&py, repo.path(), Some(cache.path())).is_ok());
    }

    #[test]
    #[cfg(unix)]
    fn rejects_in_repo_interpreter_when_hash_does_not_match() {
        let repo = TempDir::new().unwrap();
        let py = repo.path().join("tools/.venv/bin/python");
        write_exec(&py, "#!/bin/sh\necho hi\n");
        let canon = py.canonicalize().unwrap();
        let cache = TempDir::new().unwrap();
        // Record the correct path but a stale/wrong hash.
        Meta::new(repo.path(), "0", "3.13")
            .with_interpreter(canon.clone(), "deadbeef".into())
            .write(cache.path())
            .unwrap();
        let err = verify_interpreter(&py, repo.path(), Some(cache.path())).unwrap_err();
        assert!(matches!(err, ProvenanceError::UntrustedInRepo(_)));
    }

    #[test]
    #[cfg(unix)]
    fn allows_interpreter_outside_repo_and_cache() {
        let repo = TempDir::new().unwrap();
        let elsewhere = TempDir::new().unwrap();
        let py = elsewhere.path().join("bin/python");
        write_exec(&py, "#!/bin/sh\n");
        assert!(verify_interpreter(&py, repo.path(), None).is_ok());
    }
}
