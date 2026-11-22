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
    /// asks about the venv. It never vouches for the venv. The sync that ends the build stamps or ends it;
    /// one a killed sync left stays until the next sync lock holder ends it.
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
        // An unreadable entry counts as no claim: nothing to wait for.
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
mod tests;
