//! Repository trust: toolr refuses to run a repository's code until the user
//! has trusted its canonical root once.

use std::fs;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::cache::enumerate_caches;
use crate::uv::{toolr_cache_dir, toolr_state_dir};

const RECORD_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustRecord {
    pub schema_version: u32,
    pub repo_path: PathBuf,
    pub trusted_at: DateTime<Utc>,
    pub toolr_version: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TrustMode {
    /// Never prompt, even on a terminal (`--quiet`).
    pub never_prompt: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum TrustError {
    #[error(
        "refusing to run code from an untrusted repository: {}\n\
         Run `toolr project trust` there once to allow it, or set TOOLR_TRUST_ALL=1 for automation.",
        .0.display()
    )]
    Untrusted(PathBuf),
    #[error(
        "refusing to use a trust store inside the repository being checked: {}",
        .0.display()
    )]
    StoreInsideRepo(PathBuf),
}

/// Canonical form, or the path unchanged when it can't be resolved (a deleted
/// repository still has a record to look up or remove).
fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[derive(Debug, Clone)]
pub struct TrustStore {
    dir: PathBuf,
}

impl TrustStore {
    pub fn open_default() -> Result<Self> {
        let base =
            toolr_state_dir().context("cannot determine a state directory for the trust store")?;
        Ok(Self::at(base.join("trust")))
    }

    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn record_path(&self, canonical_root: &Path) -> PathBuf {
        let digest = blake3::hash(canonical_root.as_os_str().as_encoded_bytes());
        self.dir.join(format!("{}.json", digest.to_hex()))
    }

    /// Every store operation passes through here: the store must not resolve
    /// inside the repository whose trust it holds.
    fn guard(&self, canonical_root: &Path) -> Result<(), TrustError> {
        if self.is_inside(canonical_root) {
            return Err(TrustError::StoreInsideRepo(self.dir.clone()));
        }
        Ok(())
    }

    pub fn is_trusted(&self, root: &Path) -> Result<bool> {
        let root = canonical(root);
        self.guard(&root)?;
        let bytes = match fs::read(self.record_path(&root)) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e).context("reading trust record"),
        };
        Ok(serde_json::from_slice::<TrustRecord>(&bytes)
            .map(|r| r.repo_path == root)
            .unwrap_or(false))
    }

    pub fn trust(&self, root: &Path) -> Result<TrustRecord> {
        let root = canonical(root);
        self.guard(&root)?;
        let record = TrustRecord {
            schema_version: RECORD_SCHEMA_VERSION,
            repo_path: root.clone(),
            trusted_at: Utc::now(),
            toolr_version: env!("CARGO_PKG_VERSION").to_string(),
        };
        self.ensure_dir()?;
        let mut tmp = tempfile::Builder::new()
            .prefix(".trust.")
            .suffix(".tmp")
            .tempfile_in(&self.dir)
            .context("creating trust record")?;
        tmp.write_all(&serde_json::to_vec_pretty(&record)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tmp.as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        let tmp = tmp.into_temp_path();
        fs::rename(&tmp, self.record_path(&root)).context("writing trust record")?;
        Ok(record)
    }

    /// Returns whether a record existed.
    pub fn untrust(&self, root: &Path) -> Result<bool> {
        let root = canonical(root);
        self.guard(&root)?;
        match fs::remove_file(self.record_path(&root)) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e).context("removing trust record"),
        }
    }

    /// Unreadable or corrupt record files are skipped.
    pub fn list(&self) -> Result<Vec<TrustRecord>> {
        let read = match fs::read_dir(&self.dir) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).context("reading trust store"),
        };
        let mut out: Vec<TrustRecord> = read
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                if path.extension()? != "json" {
                    return None;
                }
                serde_json::from_slice(&fs::read(path).ok()?).ok()
            })
            .collect();
        out.sort_by(|a, b| a.repo_path.cmp(&b.repo_path));
        Ok(out)
    }

    fn ensure_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.dir).context("creating trust store")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))
                .context("securing trust store")?;
        }
        Ok(())
    }

    /// True when the store directory resolves under `canonical_root`. The
    /// directory may not exist yet, so the nearest existing ancestor stands in.
    fn is_inside(&self, canonical_root: &Path) -> bool {
        match std::env::current_dir() {
            Ok(cwd) => self.is_inside_from(canonical_root, &cwd),
            Err(_) => self.is_inside_from(canonical_root, Path::new("/")),
        }
    }

    /// A relative store dir (say, from a relative `XDG_STATE_HOME`) is
    /// resolved against `cwd` before probing.
    fn is_inside_from(&self, canonical_root: &Path, cwd: &Path) -> bool {
        let absolute = cwd.join(&self.dir);
        let mut probe = absolute.as_path();
        loop {
            if let Ok(resolved) = fs::canonicalize(probe) {
                return resolved.starts_with(canonical_root);
            }
            match probe.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => probe = parent,
                _ => return false,
            }
        }
    }
}

fn confirm(input: &mut dyn BufRead, out: &mut dyn Write, root: &Path) -> bool {
    let _ = write!(
        out,
        "toolr hasn't run code from this repository before:\n  {}\n\
         Running its commands executes tools/*.py and syncs tools/pyproject.toml with uv.\n\
         Trust it? [y/N] ",
        root.display()
    );
    let _ = out.flush();
    let mut answer = String::new();
    match input.read_line(&mut answer) {
        Ok(n) if n > 0 => matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"),
        _ => false,
    }
}

/// The decision flow: env override, existing record, migration from a prior
/// cache entry, prompt, refuse.
pub fn decide(
    root: &Path,
    store: &TrustStore,
    cache_root: Option<&Path>,
    all_env: bool,
    mode: TrustMode,
    prompt_io: Option<(&mut dyn BufRead, &mut dyn Write)>,
) -> Result<(), TrustError> {
    if all_env {
        return Ok(());
    }
    let root = canonical(root);
    store.guard(&root)?;
    if store.is_trusted(&root).unwrap_or(false) {
        return Ok(());
    }

    let migrated = cache_root
        .and_then(|c| enumerate_caches(c).ok())
        .is_some_and(|caches| caches.iter().any(|c| c.meta.repo_path == root));
    if migrated {
        if let Err(e) = store.trust(&root) {
            log::warn!("could not record trust for {}: {e:#}", root.display());
        }
        return Ok(());
    }

    if !mode.never_prompt {
        if let Some((input, out)) = prompt_io {
            if confirm(input, out, &root) {
                if let Err(e) = store.trust(&root) {
                    let _ = writeln!(out, "warning: could not record trust: {e:#}");
                }
                return Ok(());
            }
        }
    }
    Err(TrustError::Untrusted(root))
}

pub fn require_trust(root: &Path, mode: TrustMode) -> Result<()> {
    let root = fs::canonicalize(root)
        .with_context(|| format!("resolving repository root {}", root.display()))?;
    let store = TrustStore::open_default()?;
    let all_env = std::env::var_os("TOOLR_TRUST_ALL").is_some_and(|v| v == "1");
    let cache_root = toolr_cache_dir();
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut stderr = std::io::stderr();
    let prompt_io = stdin.is_terminal().then_some((
        &mut input as &mut dyn BufRead,
        &mut stderr as &mut dyn Write,
    ));
    decide(
        &root,
        &store,
        cache_root.as_deref(),
        all_env,
        mode,
        prompt_io,
    )
    .map_err(|e| anyhow!(e))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use tempfile::TempDir;

    use super::*;
    use crate::cache::Meta;

    struct Fixture {
        repo: TempDir,
        store_dir: TempDir,
        cache: TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                repo: TempDir::new().unwrap(),
                store_dir: TempDir::new().unwrap(),
                cache: TempDir::new().unwrap(),
            }
        }
        fn root(&self) -> PathBuf {
            fs::canonicalize(self.repo.path()).unwrap()
        }
        fn store(&self) -> TrustStore {
            TrustStore::at(self.store_dir.path().join("trust"))
        }
        fn decide(&self, input: Option<&str>, mode: TrustMode) -> (Result<(), TrustError>, String) {
            let store = self.store();
            let mut err = Vec::new();
            let result = match input {
                Some(text) => {
                    let mut cursor = Cursor::new(text.as_bytes().to_vec());
                    decide(
                        &self.root(),
                        &store,
                        Some(self.cache.path()),
                        false,
                        mode,
                        Some((&mut cursor, &mut err)),
                    )
                }
                None => decide(
                    &self.root(),
                    &store,
                    Some(self.cache.path()),
                    false,
                    mode,
                    None,
                ),
            };
            (result, String::from_utf8(err).unwrap())
        }
    }

    const PROMPT: TrustMode = TrustMode {
        never_prompt: false,
    };

    #[test]
    fn untrusted_without_tty_is_refused() {
        let f = Fixture::new();
        let (r, _) = f.decide(None, PROMPT);
        assert!(matches!(r, Err(TrustError::Untrusted(p)) if p == f.root()));
    }

    #[test]
    fn untrusted_display_is_the_refusal_text() {
        let msg = TrustError::Untrusted(PathBuf::from("/r")).to_string();
        assert_eq!(
            msg,
            "refusing to run code from an untrusted repository: /r\n\
             Run `toolr project trust` there once to allow it, or set TOOLR_TRUST_ALL=1 for automation."
        );
    }

    #[test]
    fn trust_all_env_allows() {
        let f = Fixture::new();
        let store = f.store();
        decide(&f.root(), &store, None, true, PROMPT, None).unwrap();
        assert!(!store.is_trusted(&f.root()).unwrap());
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn recorded_root_is_allowed() {
        let f = Fixture::new();
        f.store().trust(&f.root()).unwrap();
        let (r, _) = f.decide(None, PROMPT);
        assert!(r.is_ok());
    }

    #[test]
    fn untrust_removes_record() {
        let f = Fixture::new();
        let store = f.store();
        store.trust(&f.root()).unwrap();
        assert!(store.untrust(&f.root()).unwrap());
        assert!(!store.is_trusted(&f.root()).unwrap());
        assert!(!store.untrust(&f.root()).unwrap());
    }

    #[test]
    fn prompt_yes_records_trust() {
        let f = Fixture::new();
        let (r, err) = f.decide(Some("y\n"), PROMPT);
        assert!(r.is_ok());
        assert!(f.store().is_trusted(&f.root()).unwrap());
        assert!(err.contains("Trust it? [y/N]"));
        assert!(err.contains(&f.root().display().to_string()));
    }

    #[test]
    fn prompt_default_and_eof_refuse() {
        for input in ["\n", ""] {
            let f = Fixture::new();
            let (r, _) = f.decide(Some(input), PROMPT);
            assert!(
                matches!(r, Err(TrustError::Untrusted(_))),
                "input {input:?}"
            );
            assert!(!f.store().is_trusted(&f.root()).unwrap());
        }
    }

    #[test]
    fn never_prompt_refuses_even_on_tty() {
        let f = Fixture::new();
        let (r, err) = f.decide(Some("y\n"), TrustMode { never_prompt: true });
        assert!(matches!(r, Err(TrustError::Untrusted(_))));
        assert!(err.is_empty());
    }

    #[test]
    fn cache_meta_migrates() {
        let f = Fixture::new();
        Meta::new(f.root(), "0.0.0", "3.13.1")
            .write(&f.cache.path().join("some-key"))
            .unwrap();
        let (r, _) = f.decide(None, PROMPT);
        assert!(r.is_ok());
        assert!(f.store().is_trusted(&f.root()).unwrap());
    }

    #[test]
    fn meta_inside_repo_is_ignored() {
        let f = Fixture::new();
        Meta::new(f.root(), "0.0.0", "3.13.1")
            .write(&f.root().join("tools"))
            .unwrap();
        let (r, _) = f.decide(None, PROMPT);
        assert!(matches!(r, Err(TrustError::Untrusted(_))));
    }

    #[cfg(unix)]
    #[test]
    fn trust_key_is_canonical_through_symlink() {
        let f = Fixture::new();
        let link_parent = TempDir::new().unwrap();
        let link = link_parent.path().join("link");
        std::os::unix::fs::symlink(f.root(), &link).unwrap();
        let store = f.store();
        store.trust(&link).unwrap();
        assert!(store.is_trusted(&f.root()).unwrap());
        assert_eq!(store.list().unwrap()[0].repo_path, f.root());
    }

    #[test]
    fn store_inside_repo_is_rejected() {
        let f = Fixture::new();
        let store = TrustStore::at(f.root().join("state"));
        let r = decide(&f.root(), &store, None, false, PROMPT, None);
        assert!(matches!(r, Err(TrustError::StoreInsideRepo(_))));
        assert!(!f.root().join("state").exists());
    }

    #[test]
    fn direct_operations_reject_store_inside_repo() {
        let f = Fixture::new();
        let store = TrustStore::at(f.root().join("state"));
        for err in [
            store.trust(&f.root()).unwrap_err(),
            store.untrust(&f.root()).unwrap_err(),
            store.is_trusted(&f.root()).unwrap_err(),
        ] {
            assert!(matches!(
                err.downcast_ref::<TrustError>(),
                Some(TrustError::StoreInsideRepo(_))
            ));
        }
        assert!(!f.root().join("state").exists());
    }

    #[test]
    fn relative_store_inside_repo_is_rejected() {
        let f = Fixture::new();
        let store = TrustStore::at(PathBuf::from("state/trust"));
        assert!(store.is_inside_from(&f.root(), &f.root()));
        assert!(!store.is_inside_from(&f.root(), f.store_dir.path()));
    }

    #[cfg(unix)]
    #[test]
    fn store_reaching_repo_through_symlink_is_rejected() {
        let f = Fixture::new();
        let link_parent = TempDir::new().unwrap();
        let link = link_parent.path().join("link");
        std::os::unix::fs::symlink(f.root(), &link).unwrap();
        let store = TrustStore::at(link.join("state"));
        let r = decide(&f.root(), &store, None, false, PROMPT, None);
        assert!(matches!(r, Err(TrustError::StoreInsideRepo(_))));
        assert!(!f.root().join("state").exists());
    }

    #[test]
    fn list_skips_corrupt_records() {
        let f = Fixture::new();
        let store = f.store();
        store.trust(&f.root()).unwrap();
        fs::write(store.dir().join("junk.json"), b"{not json").unwrap();
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn record_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new();
        let store = f.store();
        store.trust(&f.root()).unwrap();
        let dir_mode = fs::metadata(store.dir()).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
        let file = fs::read_dir(store.dir())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let file_mode = fs::metadata(file).unwrap().permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600);
    }
}
