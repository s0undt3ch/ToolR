//! Venv side of the trust store: one entry per in-tree venv toolr built (its token and pending
//! claim), and the per-venv sync locks.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{KeyKind, TrustStore, canonical, create_private_dir, shown, write_private};

/// Store subdirectory holding one entry per in-tree venv, out of `list()`'s way.
const VENV_ENTRIES_DIR: &str = "venvs";

/// Store subdirectory holding one sync lock per in-tree venv.
const VENV_LOCKS_DIR: &str = "locks";

/// Held while toolr checks, removes, syncs and stamps one in-tree venv;
/// dropping it deletes the lock file on unix, then releases the lock.
#[derive(Debug)]
pub struct VenvLock {
    file: Option<fs::File>,
    #[cfg_attr(not(unix), allow(dead_code))]
    path: PathBuf,
}

impl Drop for VenvLock {
    fn drop(&mut self) {
        // While still held, so no other toolr has the lock yet. A waiter on this file finds the
        // path gone once it gets the lock and starts over. Only unix can tell the waiter's file
        // from a new one at the path (`is_same_file`), so elsewhere the file stays.
        #[cfg(unix)]
        let _ = fs::remove_file(&self.path);
        drop(self.file.take());
    }
}

/// The token toolr wrote into an in-tree venv it built. One per venv, not per
/// record, so worktrees of one repository don't overwrite each other's.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct VenvEntry {
    /// The trust record this venv belongs to.
    pub(super) key: PathBuf,
    pub(super) kind: KeyKind,
    venv_dir: PathBuf,
    token: String,
    /// toolr is building the venv: a sync that finds the claim waits for the sync lock before it
    /// asks about the venv. It never vouches for the venv, and never outlives the run.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pending: bool,
}

/// What venv entries are keyed by: the canonical parent plus the venv's own
/// name, so a symlinked `tools/.venv` keys by the link toolr resolved.
fn venv_entry_key(venv_dir: &Path) -> PathBuf {
    match (venv_dir.parent(), venv_dir.file_name()) {
        (Some(parent), Some(name)) => canonical(parent).join(name),
        _ => venv_dir.to_path_buf(),
    }
}

/// Random bytes in a venv token; the token is their hex, twice as many characters.
pub(crate) const TOKEN_BYTES: usize = 32;

/// What a sync-lock file's name ends in; it shares the rest of the name with its venv entry.
const LOCK_SUFFIX: &str = ".lock";

fn new_token() -> Result<String> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).context("reading the OS random number generator")?;
    Ok(bytes
        .iter()
        .flat_map(|b| [HEX[usize::from(b >> 4)], HEX[usize::from(b & 0xf)]])
        .map(char::from)
        .collect())
}

/// Delete the lock file at `path` unless another toolr holds it, or it can't be opened. Only on
/// unix: see [`VenvLock`]'s `Drop`.
#[cfg(not(unix))]
fn remove_free_lock(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn remove_free_lock(path: &Path) -> Result<()> {
    let Ok(file) = fs::File::open(path) else {
        return Ok(());
    };
    if file.try_lock().is_err() {
        return Ok(());
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).context("removing venv lock"),
    }
}

/// Whether `path` still names the open `file`. Elsewhere than unix lock files are never deleted,
/// so its existence is enough.
#[cfg(unix)]
fn is_same_file(file: &fs::File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let ids = |m: fs::Metadata| (m.dev(), m.ino());
    fs::metadata(path).map(ids).ok() == file.metadata().map(ids).ok()
}

#[cfg(not(unix))]
fn is_same_file(_file: &fs::File, path: &Path) -> bool {
    path.exists()
}

impl TrustStore {
    fn venv_entry_path(&self, venv_key: &Path) -> PathBuf {
        let digest = blake3::hash(venv_key.as_os_str().as_encoded_bytes());
        self.dir
            .join(VENV_ENTRIES_DIR)
            .join(digest.to_hex().as_str())
    }

    fn venv_lock_path(&self, venv_dir: &Path) -> PathBuf {
        let digest = blake3::hash(venv_entry_key(venv_dir).as_os_str().as_encoded_bytes());
        self.dir
            .join(VENV_LOCKS_DIR)
            .join(format!("{}{LOCK_SUFFIX}", digest.to_hex()))
    }

    /// Block until this process holds the sync lock for the in-tree venv at
    /// `venv_dir`, keyed like its entry, saying so on stderr while it waits
    /// unless `quiet`.
    pub fn lock_venv(&self, venv_dir: &Path, quiet: bool) -> Result<VenvLock> {
        self.lock_venv_with(venv_dir, quiet, &mut std::io::stderr())
    }

    fn lock_venv_with(
        &self,
        venv_dir: &Path,
        quiet: bool,
        out: &mut dyn Write,
    ) -> Result<VenvLock> {
        let failed = || {
            format!(
                "locking the tools venv in the trust store {}",
                shown(&self.dir)
            )
        };
        create_private_dir(&self.dir)
            .and_then(|()| create_private_dir(&self.dir.join(VENV_LOCKS_DIR)))
            .with_context(failed)?;
        let path = self.venv_lock_path(venv_dir);
        loop {
            let file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
                .with_context(failed)?;
            match file.try_lock() {
                Err(fs::TryLockError::WouldBlock) => {
                    if !quiet {
                        let _ = writeln!(
                            out,
                            "toolr: waiting for another toolr to finish syncing {}…",
                            shown(venv_dir)
                        );
                        // Before blocking, so the line shows while toolr waits.
                        let _ = out.flush();
                    }
                    file.lock().with_context(failed)?;
                }
                other => other.map_err(std::io::Error::from).with_context(failed)?,
            }
            // A prune may have removed the file before this process locked it, and another
            // toolr may hold a new file at the path, so only a lock on the current file counts.
            if is_same_file(&file, &path) {
                let lock = VenvLock {
                    file: Some(file),
                    path,
                };
                // No builder is running now, so a claim left is from a toolr that didn't
                // finish; ended, it sends no later sync to wait for a build that isn't running.
                // On failure the lock is released, and the claim vouches for nothing anyway.
                self.end_claim(venv_dir).with_context(failed)?;
                return Ok(lock);
            }
        }
    }

    /// The token toolr last recorded for the in-tree venv at `venv_dir`, if any.
    pub fn venv_token(&self, venv_dir: &Path) -> Result<Option<String>> {
        Ok(self.venv_entry(venv_dir)?.map(|e| e.token))
    }

    /// Whether toolr began building the in-tree venv at `venv_dir` and no sync has stamped or
    /// ended the claim since (see [`Self::claim_venv`]). A killed sync leaves one too, so it
    /// says only to wait for the sync lock, never that the venv is toolr's.
    pub fn venv_pending(&self, venv_dir: &Path) -> bool {
        // An unreadable entry counts as no claim, so the venv is asked about, never trusted.
        self.venv_entry(venv_dir)
            .ok()
            .flatten()
            .is_some_and(|e| e.pending)
    }

    fn venv_entry(&self, venv_dir: &Path) -> Result<Option<VenvEntry>> {
        let venv_key = venv_entry_key(venv_dir);
        let bytes = match fs::read(self.venv_entry_path(&venv_key)) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).context("reading venv entry"),
        };
        let entry: VenvEntry = serde_json::from_slice(&bytes).context("parsing venv entry")?;
        Ok((entry.venv_dir == venv_key).then_some(entry))
    }

    /// Under the sync lock, before uv builds or rebuilds the in-tree venv at `venv_dir`: mark it
    /// as being built by toolr, keeping any token it has, so another toolr's sync that sees it
    /// before the marker is written waits on the lock instead of asking about it.
    pub fn claim_venv(&self, root: &Path, venv_dir: &Path) -> Result<()> {
        // An unreadable entry keeps no token: a new one is minted and the old marker stops
        // matching, so the venv is asked about again (fail-safe).
        let token = match self.venv_token(venv_dir).ok().flatten() {
            Some(token) => token,
            None => new_token()?,
        };
        self.write_entry(root, venv_dir, &token, true)
    }

    /// End a pending [`Self::claim_venv`] without a stamp, keeping the token: after a failed
    /// sync, or one the lock holder finds left by a toolr that didn't finish. An unreadable
    /// entry holds no claim (see [`Self::venv_pending`]), so only a failed write is an error.
    pub fn end_claim(&self, venv_dir: &Path) -> Result<()> {
        match self.venv_entry(venv_dir) {
            Ok(Some(mut entry)) if entry.pending => {
                entry.pending = false;
                self.store_entry(&entry)
            }
            _ => Ok(()),
        }
    }

    /// Record the in-tree venv toolr just built at `venv_dir` under `root`'s trust key, clearing
    /// any pending claim, and return the token for the venv's marker: `token` ends a
    /// [`Self::claim_venv`] with the one the marker already holds, `None` mints a fresh one.
    pub fn settle_venv(&self, root: &Path, venv_dir: &Path, token: Option<&str>) -> Result<String> {
        let token = match token {
            Some(token) => token.to_owned(),
            None => new_token()?,
        };
        self.write_entry(root, venv_dir, &token, false)?;
        Ok(token)
    }

    fn write_entry(&self, root: &Path, venv_dir: &Path, token: &str, pending: bool) -> Result<()> {
        let key = self.resolve_guarded(root)?;
        self.store_entry(&VenvEntry {
            key: key.key,
            kind: key.kind,
            venv_dir: venv_entry_key(venv_dir),
            token: token.to_owned(),
            pending,
        })
    }

    fn store_entry(&self, entry: &VenvEntry) -> Result<()> {
        let dir = self.dir.join(VENV_ENTRIES_DIR);
        create_private_dir(&self.dir)?;
        create_private_dir(&dir)?;
        let bytes = serde_json::to_vec_pretty(entry)?;
        write_private(&dir, &self.venv_entry_path(&entry.venv_dir), &bytes)
            .context("writing venv entry")
    }

    /// Drop venv entries whose trust record is gone, whose venv no longer
    /// exists, or that don't parse, then the sync locks no entry is left for.
    pub fn prune_venv_entries(&self) -> Result<()> {
        self.remove_venv_entries_where(|e| match e {
            None => true,
            Some(e) => {
                !self.record_path(&e.key, e.kind).exists()
                    || fs::symlink_metadata(&e.venv_dir).is_err()
            }
        })?;
        self.prune_venv_locks()
    }

    /// A lock and its entry share a name, bar `.lock`. A lock another toolr holds stays: it may
    /// be building a venv whose entry isn't written yet.
    fn prune_venv_locks(&self) -> Result<()> {
        let read = match fs::read_dir(self.dir.join(VENV_LOCKS_DIR)) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e).context("reading venv locks"),
        };
        let entries = self.dir.join(VENV_ENTRIES_DIR);
        for item in read {
            let path = item.context("reading venv locks")?.path();
            let Some(stem) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(LOCK_SUFFIX))
                .filter(|stem| !stem.is_empty() && !stem.starts_with('.'))
            else {
                continue;
            };
            if !entries.join(stem).exists() {
                remove_free_lock(&path)?;
            }
        }
        Ok(())
    }

    /// `doomed` sees `None` for an entry that doesn't parse. Leftover temp
    /// files are skipped.
    pub(super) fn remove_venv_entries_where(
        &self,
        doomed: impl Fn(Option<&VenvEntry>) -> bool,
    ) -> Result<()> {
        let read = match fs::read_dir(self.dir.join(VENV_ENTRIES_DIR)) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e).context("reading venv entries"),
        };
        for item in read {
            let path = item.context("reading venv entries")?.path();
            if path
                .file_name()
                .is_some_and(|n| n.as_encoded_bytes().starts_with(b"."))
            {
                continue;
            }
            let entry = fs::read(&path)
                .ok()
                .and_then(|b| serde_json::from_slice::<VenvEntry>(&b).ok());
            if doomed(entry.as_ref()) {
                path.file_name()
                    .map(|name| {
                        self.dir
                            .join(VENV_LOCKS_DIR)
                            .join(format!("{}{LOCK_SUFFIX}", name.display()))
                    })
                    .map_or(Ok(()), |lock| remove_free_lock(&lock))?;
                match fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e).context("removing venv entry"),
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::super::tests::Fixture;
    use super::*;

    fn venv_in(root: &Path, name: &str) -> PathBuf {
        let venv = root.join(name).join("tools/.venv");
        fs::create_dir_all(&venv).unwrap();
        venv
    }

    #[test]
    fn settle_venv_then_venv_token_round_trips() {
        let f = Fixture::new();
        let store = f.store();
        store.trust(&f.root()).unwrap();
        let venv = venv_in(&f.root(), "a");
        assert_eq!(store.venv_token(&venv).unwrap(), None);
        let token = store.settle_venv(&f.root(), &venv, None).unwrap();
        assert_eq!(token.len(), 64);
        assert!(token.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(store.venv_token(&venv).unwrap(), Some(token.clone()));
        let again = store.settle_venv(&f.root(), &venv, None).unwrap();
        assert_ne!(again, token);
        assert_eq!(store.venv_token(&venv).unwrap(), Some(again));
        assert_eq!(store.venv_token(&venv_in(&f.root(), "b")).unwrap(), None);
    }

    #[test]
    fn settling_a_claimed_venv_keeps_the_given_token() {
        let f = Fixture::new();
        let store = f.store();
        store.trust(&f.root()).unwrap();
        let venv = venv_in(&f.root(), "a");
        store.claim_venv(&f.root(), &venv).unwrap();
        assert!(store.venv_pending(&venv));
        let token = store
            .venv_token(&venv)
            .unwrap()
            .expect("a claim records a token");
        assert_eq!(
            store.settle_venv(&f.root(), &venv, Some(&token)).unwrap(),
            token
        );
        assert!(!store.venv_pending(&venv));
        assert_eq!(store.venv_token(&venv).unwrap(), Some(token));
    }

    #[test]
    fn ending_a_claim_keeps_the_token() {
        let f = Fixture::new();
        let store = f.store();
        store.trust(&f.root()).unwrap();
        let venv = venv_in(&f.root(), "a");
        store.end_claim(&venv).unwrap();
        assert_eq!(
            store.venv_token(&venv).unwrap(),
            None,
            "no entry, nothing written"
        );
        store.claim_venv(&f.root(), &venv).unwrap();
        let token = store.venv_token(&venv).unwrap();
        store.end_claim(&venv).unwrap();
        assert!(!store.venv_pending(&venv));
        assert_eq!(store.venv_token(&venv).unwrap(), token);

        // An unreadable entry holds no claim, so there is nothing to end.
        let entries = store.dir().join(VENV_ENTRIES_DIR);
        let entry = fs::read_dir(&entries)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::write(&entry, b"not json").unwrap();
        store.end_claim(&venv).unwrap();
        assert!(!store.venv_pending(&venv));
    }

    /// No builder runs while this process holds the lock, so a claim found then is ended.
    #[test]
    fn taking_the_lock_ends_a_claim_left_behind() {
        let f = Fixture::new();
        let store = f.store();
        store.trust(&f.root()).unwrap();
        let venv = venv_in(&f.root(), "a");
        store.claim_venv(&f.root(), &venv).unwrap();
        let _lock = store.lock_venv(&venv, true).unwrap();
        assert!(!store.venv_pending(&venv));
        assert!(store.venv_token(&venv).unwrap().is_some());
    }

    #[cfg(unix)]
    #[test]
    fn venv_entry_keys_a_symlinked_venv_by_its_link() {
        let f = Fixture::new();
        let store = f.store();
        let target = venv_in(&f.root(), "target");
        let link = f.root().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let token = store.settle_venv(&f.root(), &link, None).unwrap();
        assert_eq!(store.venv_token(&link).unwrap(), Some(token));
        assert_eq!(store.venv_token(&target).unwrap(), None);
    }

    #[test]
    fn pruning_reports_an_entry_it_cannot_remove() {
        let f = Fixture::new();
        let store = f.store();
        let venv = venv_in(&f.root(), "a");
        // A directory where the entry file belongs doesn't parse, so it is
        // doomed, and `remove_file` refuses it.
        fs::create_dir_all(store.venv_entry_path(&venv_entry_key(&venv))).unwrap();
        let err = store.prune_venv_entries().unwrap_err();
        assert!(err.to_string().contains("removing venv entry"), "{err}");
    }

    #[test]
    fn corrupt_venv_entry_is_an_error() {
        let f = Fixture::new();
        let store = f.store();
        let venv = venv_in(&f.root(), "a");
        store.settle_venv(&f.root(), &venv, None).unwrap();
        let entry = fs::read_dir(store.dir().join("venvs"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::write(entry, b"{not json").unwrap();
        assert!(store.venv_token(&venv).is_err());
    }

    #[test]
    fn unreadable_venv_entries_are_errors() {
        let f = Fixture::new();
        let store = f.store();
        let venv = venv_in(&f.root(), "a");
        let entry = store.venv_entry_path(&venv_entry_key(&venv));
        fs::create_dir_all(&entry).unwrap();
        assert!(store.venv_token(&venv).is_err());
        fs::remove_dir(&entry).unwrap();
        fs::remove_dir(entry.parent().unwrap()).unwrap();
        fs::write(entry.parent().unwrap(), b"not a directory").unwrap();
        assert!(store.prune_venv_entries().is_err());
    }

    #[test]
    fn venv_entry_key_keeps_a_path_without_a_name() {
        assert_eq!(venv_entry_key(Path::new("/")), Path::new("/"));
    }

    #[test]
    fn untrust_removes_its_venv_entries() {
        let f = Fixture::new();
        let other = TempDir::new().unwrap();
        let other_root = fs::canonicalize(other.path()).unwrap();
        let store = f.store();
        store.trust(&f.root()).unwrap();
        store.trust(&other_root).unwrap();
        let (a, b) = (venv_in(&f.root(), "a"), venv_in(&f.root(), "b"));
        let kept = venv_in(&other_root, "c");
        for venv in [&a, &b] {
            store.settle_venv(&f.root(), venv, None).unwrap();
        }
        let kept_token = store.settle_venv(&other_root, &kept, None).unwrap();
        store.untrust(&f.root()).unwrap();
        assert_eq!(store.venv_token(&a).unwrap(), None);
        assert_eq!(store.venv_token(&b).unwrap(), None);
        assert_eq!(store.venv_token(&kept).unwrap(), Some(kept_token));
        assert!(store.untrust_key(&other_root).unwrap());
        assert_eq!(store.venv_token(&kept).unwrap(), None);
    }

    #[test]
    fn prune_drops_venv_entries_for_missing_venvs() {
        let f = Fixture::new();
        let orphaned = TempDir::new().unwrap();
        let orphaned_root = fs::canonicalize(orphaned.path()).unwrap();
        let store = f.store();
        store.trust(&f.root()).unwrap();
        store.trust(&orphaned_root).unwrap();
        let (kept, gone) = (venv_in(&f.root(), "kept"), venv_in(&f.root(), "gone"));
        let unrecorded = venv_in(&orphaned_root, "c");
        let kept_token = store.settle_venv(&f.root(), &kept, None).unwrap();
        store.settle_venv(&f.root(), &gone, None).unwrap();
        store
            .settle_venv(&orphaned_root, &unrecorded, None)
            .unwrap();
        fs::remove_dir_all(&gone).unwrap();
        fs::remove_file(store.record_path(&orphaned_root, KeyKind::PerPath)).unwrap();
        fs::write(store.dir().join("venvs/junk"), b"{not json").unwrap();
        fs::write(store.dir().join("venvs/.trust.abc.tmp"), b"{}").unwrap();

        store.prune_venv_entries().unwrap();
        assert_eq!(store.venv_token(&kept).unwrap(), Some(kept_token));
        assert_eq!(store.venv_token(&gone).unwrap(), None);
        assert_eq!(store.venv_token(&unrecorded).unwrap(), None);
        assert!(!store.dir().join("venvs/junk").exists());
        assert!(store.dir().join("venvs/.trust.abc.tmp").exists());
    }

    #[test]
    fn prune_and_untrust_without_venv_entries_are_no_ops() {
        let f = Fixture::new();
        let store = f.store();
        store.prune_venv_entries().unwrap();
        store.trust(&f.root()).unwrap();
        store.untrust(&f.root()).unwrap();
        store.prune_venv_entries().unwrap();
    }

    #[test]
    fn lock_venv_excludes_other_holders_until_dropped() {
        let f = Fixture::new();
        let store = f.store();
        let venv = venv_in(&f.root(), "a");
        let held = store.lock_venv(&venv, false).unwrap();
        let other = fs::File::open(store.venv_lock_path(&venv)).unwrap();
        assert!(matches!(
            other.try_lock(),
            Err(fs::TryLockError::WouldBlock)
        ));
        drop(held);
        #[cfg(unix)]
        assert!(
            !store.venv_lock_path(&venv).exists(),
            "releasing deletes the file"
        );
        // Blocking: a test elsewhere forking right now can hold the
        // descriptor until its child execs.
        other.lock().unwrap();
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn lock_venv_without_a_usable_store_names_it() {
        let f = Fixture::new();
        fs::write(f.store().dir(), b"not a directory").unwrap();
        let err = f
            .store()
            .lock_venv(&venv_in(&f.root(), "a"), false)
            .unwrap_err();
        assert!(
            format!("{err:#}").contains(&shown(f.store().dir())),
            "{err:#}"
        );
    }

    /// Collects what a waiting lock writes, readable from another thread.
    #[derive(Clone, Default)]
    struct SharedOut(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl Write for SharedOut {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl SharedOut {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    #[test]
    fn lock_venv_says_when_it_waits_for_another_holder() {
        let f = Fixture::new();
        let store = f.store();
        let venv = venv_in(&f.root(), "a");
        let mut free = SharedOut::default();
        drop(store.lock_venv_with(&venv, false, &mut free).unwrap());
        assert_eq!(free.text(), "", "an uncontended lock says nothing");

        let held = store.lock_venv(&venv, false).unwrap();
        let out = SharedOut::default();
        let waiter = {
            let (store, venv, mut out) = (store.clone(), venv.clone(), out.clone());
            std::thread::spawn(move || store.lock_venv_with(&venv, false, &mut out).map(drop))
        };
        // The timeout only bounds a failing run; the line appearing is what passes.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !out.text().contains("waiting") && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(
            out.text(),
            format!(
                "toolr: waiting for another toolr to finish syncing {}…\n",
                shown(&venv)
            )
        );
        drop(held);
        waiter.join().unwrap().unwrap();
    }

    /// Waits on `venv`'s lock in a thread while `held` holds it, releases `held` once the waiter
    /// is blocked, and returns what the waiter wrote and the lock it took.
    fn wait_on_held_lock(
        store: &TrustStore,
        venv: &Path,
        quiet: bool,
        held: VenvLock,
    ) -> (String, VenvLock) {
        let out = SharedOut::default();
        let waiter = {
            let (store, venv, mut out) = (store.clone(), venv.to_path_buf(), out.clone());
            std::thread::spawn(move || store.lock_venv_with(&venv, quiet, &mut out))
        };
        if quiet {
            // Nothing to watch for, so give the waiter time to block.
            std::thread::sleep(std::time::Duration::from_millis(300));
        } else {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while !out.text().contains("waiting") && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        drop(held);
        let lock = waiter.join().unwrap().unwrap();
        (out.text(), lock)
    }

    #[test]
    fn lock_venv_waits_silently_when_quiet() {
        let f = Fixture::new();
        let store = f.store();
        let venv = venv_in(&f.root(), "a");
        let held = store.lock_venv(&venv, true).unwrap();
        let (said, _lock) = wait_on_held_lock(&store, &venv, true, held);
        assert_eq!(said, "");
    }

    /// The holder deletes the file the waiter blocks on as it releases it.
    #[cfg(unix)]
    #[test]
    fn a_waiter_whose_file_was_deleted_locks_the_current_one() {
        let f = Fixture::new();
        let store = f.store();
        let venv = venv_in(&f.root(), "a");
        let path = store.venv_lock_path(&venv);
        let held = store.lock_venv(&venv, false).unwrap();
        let (_, lock) = wait_on_held_lock(&store, &venv, false, held);
        // The waiter locked the deleted file first, then created and locked a new one.
        let other = fs::File::open(&path).unwrap();
        assert!(matches!(
            other.try_lock(),
            Err(fs::TryLockError::WouldBlock)
        ));
        drop(lock);
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn untrust_removes_the_free_locks_of_its_venv_entries() {
        let f = Fixture::new();
        let store = f.store();
        store.trust(&f.root()).unwrap();
        let (free, held) = (venv_in(&f.root(), "free"), venv_in(&f.root(), "held"));
        for venv in [&free, &held] {
            store.settle_venv(&f.root(), venv, None).unwrap();
        }
        let holding = store.lock_venv(&held, true).unwrap();
        // Never locked, so no test forking meanwhile can hold it.
        fs::write(store.venv_lock_path(&free), b"").unwrap();
        store.untrust(&f.root()).unwrap();
        assert!(!store.venv_lock_path(&free).exists());
        assert!(store.venv_lock_path(&held).exists(), "a held lock stays");
        assert_eq!(store.venv_token(&held).unwrap(), None);
        drop(holding);
    }

    #[cfg(unix)]
    #[test]
    fn prune_removes_only_free_locks_without_an_entry() {
        let f = Fixture::new();
        let store = f.store();
        store.trust(&f.root()).unwrap();
        let (kept, orphan, held) = (
            venv_in(&f.root(), "kept"),
            venv_in(&f.root(), "orphan"),
            venv_in(&f.root(), "held"),
        );
        store.settle_venv(&f.root(), &kept, None).unwrap();
        let holding = store.lock_venv(&held, true).unwrap();
        // Never locked, so no test forking meanwhile can hold them.
        for venv in [&kept, &orphan] {
            fs::write(store.venv_lock_path(venv), b"").unwrap();
        }
        let locks = store.dir().join(VENV_LOCKS_DIR);
        for junk in ["junk", ".tmp.lock", ".lock"] {
            fs::write(locks.join(junk), b"").unwrap();
        }

        store.prune_venv_entries().unwrap();
        assert!(store.venv_lock_path(&kept).exists());
        assert!(!store.venv_lock_path(&orphan).exists());
        assert!(store.venv_lock_path(&held).exists(), "a held lock stays");
        for junk in ["junk", ".tmp.lock", ".lock"] {
            assert!(locks.join(junk).exists(), "{junk}");
        }
        drop(holding);
    }

    #[cfg(unix)]
    #[test]
    fn prune_skips_an_unreadable_lock_and_reports_an_unremovable_one() {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new();
        let store = f.store();
        let venv = venv_in(&f.root(), "a");
        let lock = store.venv_lock_path(&venv);
        fs::create_dir_all(lock.parent().unwrap()).unwrap();
        fs::write(&lock, b"").unwrap();
        fs::set_permissions(&lock, fs::Permissions::from_mode(0o000)).unwrap();
        // Root opens it regardless, and then removes it.
        let readable = fs::File::open(&lock).is_ok();
        store.prune_venv_entries().unwrap();
        assert_eq!(
            fs::symlink_metadata(&lock).is_ok(),
            !readable,
            "an unreadable lock stays"
        );
        let _ = fs::remove_file(&lock);
        fs::create_dir(&lock).unwrap();
        let err = store.prune_venv_entries().unwrap_err();
        assert!(format!("{err:#}").contains("removing venv lock"), "{err:#}");
    }

    #[test]
    fn prune_with_an_unreadable_locks_dir_is_an_error() {
        let f = Fixture::new();
        let store = f.store();
        store.trust(&f.root()).unwrap();
        fs::write(store.dir().join(VENV_LOCKS_DIR), b"not a directory").unwrap();
        let err = store.prune_venv_entries().unwrap_err();
        assert!(format!("{err:#}").contains("reading venv locks"), "{err:#}");
    }

    #[test]
    fn prune_without_a_locks_dir_is_a_no_op() {
        let f = Fixture::new();
        let store = f.store();
        store.trust(&f.root()).unwrap();
        store.prune_venv_entries().unwrap();
        assert!(!store.dir().join(VENV_LOCKS_DIR).exists());
    }

    #[test]
    fn list_ignores_venv_entries() {
        let f = Fixture::new();
        let store = f.store();
        store.trust(&f.root()).unwrap();
        store
            .settle_venv(&f.root(), &venv_in(&f.root(), "a"), None)
            .unwrap();
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn venv_entry_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new();
        let store = f.store();
        store
            .settle_venv(&f.root(), &venv_in(&f.root(), "a"), None)
            .unwrap();
        let dir = store.dir().join("venvs");
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let file = fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
        assert_eq!(
            fs::metadata(file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
