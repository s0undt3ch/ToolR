//! Repository trust: toolr refuses to run a repository's code until the user
//! has trusted its canonical root once.

use std::fs;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::cache::Meta;
use crate::uv::{toolr_cache_dir, toolr_state_dir};

const RECORD_SCHEMA_VERSION: u32 = 1;
/// Present once the one-time import of pre-trust cache entries has run.
const MIGRATION_MARKER: &str = ".migrated-v1";

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
        "refusing to use a trust store inside the repository being checked: {}\n\
         Set XDG_STATE_HOME to a directory outside the repository.",
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
        self.guard_with_home(
            canonical_root,
            dirs::home_dir().map(|h| canonical(&h)).as_deref(),
        )
    }

    /// A `~/tools` makes the home directory the root, and the default store
    /// always lives under it. Nobody clones a hostile repository as `$HOME`.
    fn guard_with_home(
        &self,
        canonical_root: &Path,
        home: Option<&Path>,
    ) -> Result<(), TrustError> {
        if home == Some(canonical_root) {
            return Ok(());
        }
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

    /// True when the store directory resolves under `canonical_root`.
    fn is_inside(&self, canonical_root: &Path) -> bool {
        match std::env::current_dir() {
            Ok(cwd) => self.is_inside_from(canonical_root, &cwd),
            Err(_) => self.is_inside_from(canonical_root, Path::new("/")),
        }
    }

    fn is_inside_from(&self, canonical_root: &Path, cwd: &Path) -> bool {
        resolves_inside(&self.dir, canonical_root, cwd)
    }

    /// One-time import of trust from cache entries written before trust
    /// existed: a prior sync was prior consent. It runs at most once per
    /// store, so a later `untrust` (or a cache entry written under
    /// `TOOLR_TRUST_ALL=1`) is never turned back into trust. `checking_root`
    /// is the canonical root of the repository being acted on; nothing is
    /// read from inside it.
    pub fn migrate_once(&self, cache_root: Option<&Path>, checking_root: &Path) {
        let Some(cache_root) = cache_root else {
            return;
        };
        let checking_root = &canonical(checking_root);
        if self.guard(checking_root).is_err() || self.dir.join(MIGRATION_MARKER).exists() {
            return;
        }
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        if resolves_inside(cache_root, checking_root, &cwd) {
            return;
        }
        let entries = match fs::read_dir(cache_root) {
            Ok(entries) => Some(entries),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                log::warn!(
                    "could not read {} for trust migration: {e}",
                    cache_root.display()
                );
                return;
            }
        };
        for entry in entries.into_iter().flatten().flatten() {
            let cache_dir = entry.path();
            let Ok(meta) = Meta::load(&cache_dir) else {
                continue;
            };
            let Ok(repo) = fs::canonicalize(&meta.repo_path) else {
                continue;
            };
            if !repo.is_dir() || canonical(&Meta::path_in(&cache_dir)).starts_with(&repo) {
                continue;
            }
            if let Err(e) = self.trust(&repo) {
                log::warn!("could not migrate trust for {}: {e:#}", repo.display());
            }
        }
        if let Err(e) = self
            .ensure_dir()
            .and_then(|()| fs::write(self.dir.join(MIGRATION_MARKER), b"").map_err(Into::into))
        {
            log::warn!("could not record trust migration: {e:#}");
        }
    }
}

/// True when `path` resolves under `canonical_root`. `path` may not exist
/// yet, so its nearest existing ancestor stands in; a relative `path` (say,
/// from a relative `XDG_STATE_HOME`) is resolved against `cwd` first.
fn resolves_inside(path: &Path, canonical_root: &Path, cwd: &Path) -> bool {
    let absolute = cwd.join(path);
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

/// The decision flow: one-time migration from pre-trust cache entries, env
/// override, existing record, prompt, refuse. Migration runs even under the
/// env override so the cache entry that run writes is never imported later.
pub fn decide(
    root: &Path,
    store: &TrustStore,
    cache_root: Option<&Path>,
    all_env: bool,
    mode: TrustMode,
    prompt_io: Option<(&mut dyn BufRead, &mut dyn Write)>,
) -> Result<(), TrustError> {
    let root = canonical(root);
    store.migrate_once(cache_root, &root);
    if all_env {
        return Ok(());
    }
    store.guard(&root)?;
    if store.is_trusted(&root).unwrap_or(false) {
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
    fn migration_runs_once() {
        let f = Fixture::new();
        Meta::new(f.root(), "0.0.0", "3.13.1")
            .write(&f.cache.path().join("first"))
            .unwrap();
        assert!(f.decide(None, PROMPT).0.is_ok());
        assert!(f.store().dir().join(MIGRATION_MARKER).is_file());

        let later = TempDir::new().unwrap();
        let later_root = fs::canonicalize(later.path()).unwrap();
        Meta::new(&later_root, "0.0.0", "3.13.1")
            .write(&f.cache.path().join("second"))
            .unwrap();
        let r = decide(
            &later_root,
            &f.store(),
            Some(f.cache.path()),
            false,
            PROMPT,
            None,
        );
        assert!(matches!(r, Err(TrustError::Untrusted(_))));
        assert!(!f.store().is_trusted(&later_root).unwrap());
    }

    #[test]
    fn untrust_sticks_after_migration() {
        let f = Fixture::new();
        Meta::new(f.root(), "0.0.0", "3.13.1")
            .write(&f.cache.path().join("some-key"))
            .unwrap();
        assert!(f.decide(None, PROMPT).0.is_ok());
        assert!(f.store().untrust(&f.root()).unwrap());
        let (r, _) = f.decide(None, PROMPT);
        assert!(matches!(r, Err(TrustError::Untrusted(_))));
    }

    #[test]
    fn trust_all_env_consumes_migration() {
        let f = Fixture::new();
        let store = f.store();
        decide(&f.root(), &store, Some(f.cache.path()), true, PROMPT, None).unwrap();
        Meta::new(f.root(), "0.0.0", "3.13.1")
            .write(&f.cache.path().join("some-key"))
            .unwrap();
        let (r, _) = f.decide(None, PROMPT);
        assert!(matches!(r, Err(TrustError::Untrusted(_))));
    }

    #[test]
    fn migration_trusts_every_cached_repo_and_skips_missing() {
        let f = Fixture::new();
        let other = TempDir::new().unwrap();
        let other_root = fs::canonicalize(other.path()).unwrap();
        Meta::new(&other_root, "0.0.0", "3.13.1")
            .write(&f.cache.path().join("other"))
            .unwrap();
        Meta::new(f.root().join("gone"), "0.0.0", "3.13.1")
            .write(&f.cache.path().join("gone"))
            .unwrap();
        fs::create_dir(f.cache.path().join("no-meta")).unwrap();
        fs::write(f.cache.path().join("stray-file"), b"").unwrap();
        let (r, _) = f.decide(None, PROMPT);
        assert!(matches!(r, Err(TrustError::Untrusted(_))));
        let listed: Vec<_> = f
            .store()
            .list()
            .unwrap()
            .into_iter()
            .map(|r| r.repo_path)
            .collect();
        assert_eq!(listed, vec![other_root]);
    }

    #[test]
    fn cache_root_inside_checked_repo_is_not_read() {
        let f = Fixture::new();
        let cache = f.root().join("cache");
        Meta::new(f.root(), "0.0.0", "3.13.1")
            .write(&cache.join("some-key"))
            .unwrap();
        let r = decide(&f.root(), &f.store(), Some(&cache), false, PROMPT, None);
        assert!(matches!(r, Err(TrustError::Untrusted(_))));
        assert!(!f.store().is_trusted(&f.root()).unwrap());
        assert!(!f.store().dir().join(MIGRATION_MARKER).exists());
    }

    #[test]
    fn meta_inside_the_repo_it_names_is_not_migrated() {
        let f = Fixture::new();
        let other = TempDir::new().unwrap();
        let other_root = fs::canonicalize(other.path()).unwrap();
        let cache = other_root.join("cache");
        Meta::new(&other_root, "0.0.0", "3.13.1")
            .write(&cache.join("some-key"))
            .unwrap();
        let r = decide(&f.root(), &f.store(), Some(&cache), false, PROMPT, None);
        assert!(matches!(r, Err(TrustError::Untrusted(_))));
        assert!(!f.store().is_trusted(&other_root).unwrap());
        assert!(f.store().dir().join(MIGRATION_MARKER).is_file());
    }

    #[test]
    fn list_ignores_migration_marker() {
        let f = Fixture::new();
        f.store().migrate_once(Some(f.cache.path()), &f.root());
        assert!(f.store().dir().join(MIGRATION_MARKER).is_file());
        assert!(f.store().list().unwrap().is_empty());
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
    fn store_inside_home_root_is_allowed() {
        let f = Fixture::new();
        let store = TrustStore::at(f.root().join("state"));
        let root = canonical(&f.root());
        assert!(store.guard_with_home(&root, Some(&root)).is_ok());
        assert!(matches!(
            store.guard_with_home(&root, None),
            Err(TrustError::StoreInsideRepo(_))
        ));
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
