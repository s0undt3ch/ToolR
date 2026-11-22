//! Repository trust: toolr refuses to run a repository's code until the user
//! has trusted its git repository (all its worktrees) or, outside git, its path.

use std::fs;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::uv::toolr_state_dir;

mod repo_key;
mod venv_entries;
pub use repo_key::{KeyKind, RepairHint, RepoKey};
pub(crate) use venv_entries::TOKEN_BYTES;
pub use venv_entries::VenvLock;

const RECORD_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustRecord {
    pub schema_version: u32,
    /// The git common dir, or the project root outside git. What lookups match.
    pub key: PathBuf,
    pub kind: KeyKind,
    /// The repository path shown to the user.
    pub repo_path: PathBuf,
    pub trusted_at: Timestamp,
    pub toolr_version: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TrustMode {
    /// Never prompt, even on a terminal (`--quiet`).
    pub never_prompt: bool,
}

/// Proof that this run found a repository trusted, or just recorded trust in
/// it. Only this module makes one, so holding it is the only way to skip a
/// later trust check for the same root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trusted {
    key: PathBuf,
    kind: KeyKind,
    root: PathBuf,
    granted_at_prompt: bool,
}

impl Trusted {
    /// The canonical project root the proof was made for. For a proof made in
    /// a worktree this is the worktree's root, not the main checkout's.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether the user granted trust at this run's prompt, which is what lets
    /// dispatch go on to offer a sync. `grant()` sets it false: `toolr project
    /// trust` is an explicit command that runs its own sync step, not a prompt.
    pub fn granted_at_prompt(&self) -> bool {
        self.granted_at_prompt
    }

    /// True when `root` resolves to the repository this proof covers.
    pub fn covers(&self, root: &Path) -> bool {
        let other = resolve_key(root);
        self.key == other.key && self.kind == other.kind
    }

    #[cfg(test)]
    pub(crate) fn for_tests(root: &Path) -> Self {
        let key = resolve_key(root);
        Self::from_key(&key, false)
    }

    fn from_key(key: &RepoKey, granted_at_prompt: bool) -> Self {
        Self {
            key: key.key.clone(),
            kind: key.kind,
            root: key.project_root.clone(),
            granted_at_prompt,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TrustError {
    #[error("{}", untrusted_message(.repo, .repair.as_ref()))]
    Untrusted {
        /// The repository path shown to the user.
        repo: PathBuf,
        /// The worktree `git worktree repair` would bind, if the directory
        /// looks like a moved one.
        repair: Option<RepairHint>,
    },
    #[error(
        "refusing to use a trust store inside the repository being checked: {}\n\
         Set XDG_STATE_HOME to a directory outside the repository.",
        shown(.0)
    )]
    StoreInsideRepo(PathBuf),
}

fn untrusted_message(repo: &Path, repair: Option<&RepairHint>) -> String {
    let mut msg = format!(
        "refusing to run code from an untrusted repository: {}\n\
         Run `toolr project trust` there once to allow it, or set TOOLR_TRUST_ALL=1 for automation.",
        shown(repo)
    );
    // A moved worktree and a forged `.git` file look the same on disk, and the
    // repair hands the directory the repository's trust.
    if let Some(hint) = repair {
        msg.push_str(&format!(
            "\nThis directory claims to be worktree `{}` of {}. Only if you moved that \
             worktree yourself, run `git worktree repair {}` from the main checkout.",
            shown(&hint.worktree),
            shown(&hint.repo),
            shown(&hint.worktree_top)
        ));
    }
    msg
}

/// Canonical form, or the path unchanged when it can't be resolved. Used to
/// canonicalise the home directory that key resolution and the guard compare
/// against.
fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn create_private_dir(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).context("creating trust store")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
            .context("securing trust store")?;
    }
    Ok(())
}

/// Atomically replace `dest` (inside `dir`) with `bytes`, readable only by the user.
fn write_private(dir: &Path, dest: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp = tempfile::Builder::new()
        .prefix(".trust.")
        .suffix(".tmp")
        .tempfile_in(dir)?;
    tmp.write_all(bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(tmp.into_temp_path(), dest)?;
    Ok(())
}

/// The canonical home directory, worked out once per lookup for both key resolution and the guard.
fn canonical_home() -> Option<PathBuf> {
    dirs::home_dir().map(|h| canonical(&h))
}

/// The one place a project root becomes a trust key.
fn resolve_key(root: &Path) -> RepoKey {
    RepoKey::resolve(root, canonical_home().as_deref())
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

    fn record_path(&self, key: &Path, kind: KeyKind) -> PathBuf {
        let mut hasher = blake3::Hasher::new();
        hasher.update(match kind {
            KeyKind::Repository => b"repository\0",
            KeyKind::PerPath => b"per-path\0\0\0",
        });
        hasher.update(key.as_os_str().as_encoded_bytes());
        let digest = hasher.finalize();
        self.dir.join(format!("{}.json", digest.to_hex()))
    }

    /// `root`'s key, once the guard has passed it. Every store operation that enters a
    /// repository passes through here or [`decide`] (not `untrust_key`, which only deletes a
    /// record by its stored key): the store must not resolve inside the repository whose trust
    /// it holds.
    fn resolve_guarded(&self, root: &Path) -> Result<RepoKey, TrustError> {
        let home = canonical_home();
        let key = RepoKey::resolve(root, home.as_deref());
        self.guard_with_home(&key, home.as_deref())?;
        Ok(key)
    }

    /// Checks the key, the shown repository path (a worktree outside the
    /// repository would otherwise let a store sit in the main checkout), the
    /// project root and the worktree top above a nested project root. A
    /// `~/tools` makes the home directory one of them, and the default store
    /// always lives under it. Nobody clones a hostile repository as `$HOME`.
    fn guard_with_home(&self, key: &RepoKey, home: Option<&Path>) -> Result<(), TrustError> {
        let paths = [&key.key, &key.display, &key.project_root];
        for path in paths.into_iter().chain(key.worktree_top.as_ref()) {
            if home != Some(path.as_path()) && self.is_inside(path) {
                return Err(TrustError::StoreInsideRepo(self.dir.clone()));
            }
        }
        Ok(())
    }

    pub fn is_trusted(&self, root: &Path) -> Result<bool> {
        let key = self.resolve_guarded(root)?;
        self.is_trusted_key(&key)
    }

    /// Neither resolves nor guards: the caller already did, for this `key`.
    fn is_trusted_key(&self, key: &RepoKey) -> Result<bool> {
        let bytes = match fs::read(self.record_path(&key.key, key.kind)) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e).context("reading trust record"),
        };
        Ok(serde_json::from_slice::<TrustRecord>(&bytes)
            .map(|r| {
                r.schema_version == RECORD_SCHEMA_VERSION && r.key == key.key && r.kind == key.kind
            })
            .unwrap_or(false))
    }

    pub fn trust(&self, root: &Path) -> Result<TrustRecord> {
        self.grant(root).map(|(record, _)| record)
    }

    /// [`Self::trust`], plus the proof that `root` is now trusted. Private, so
    /// proofs only come from the default store (see [`grant_trust`]).
    fn grant(&self, root: &Path) -> Result<(TrustRecord, Trusted)> {
        let key = self.resolve_guarded(root)?;
        self.grant_key(&key)
    }

    /// Neither resolves nor guards: the caller already did, for this `key`.
    fn grant_key(&self, key: &RepoKey) -> Result<(TrustRecord, Trusted)> {
        let record = TrustRecord {
            schema_version: RECORD_SCHEMA_VERSION,
            key: key.key.clone(),
            kind: key.kind,
            repo_path: key.display.clone(),
            trusted_at: Timestamp::now(),
            toolr_version: env!("CARGO_PKG_VERSION").to_string(),
        };
        create_private_dir(&self.dir)?;
        let bytes = serde_json::to_vec_pretty(&record)?;
        write_private(&self.dir, &self.record_path(&key.key, key.kind), &bytes)
            .context("writing trust record")?;
        Ok((record, Trusted::from_key(key, false)))
    }

    /// Returns the repository path to show and whether a record existed. A
    /// project root that no longer exists resolves per-path, so the record of a
    /// deleted non-git project can still be removed.
    pub fn untrust(&self, root: &Path) -> Result<(PathBuf, bool)> {
        let key = self.resolve_guarded(root)?;
        let existed = self.remove_record(&key.key, key.kind)?;
        self.remove_venv_entries_where(|e| {
            e.is_some_and(|e| e.key == key.key && e.kind == key.kind)
        })?;
        Ok((key.display, existed))
    }

    /// Removes the records stored under `key` (of either kind), and their venv
    /// entries, as `toolr self trusted --prune` does: the key is never
    /// re-resolved as a project root. Returns whether a record existed.
    pub fn untrust_key(&self, key: &Path) -> Result<bool> {
        let repository = self.remove_record(key, KeyKind::Repository)?;
        let per_path = self.remove_record(key, KeyKind::PerPath)?;
        self.remove_venv_entries_where(|e| e.is_some_and(|e| e.key == key))?;
        Ok(repository || per_path)
    }

    fn remove_record(&self, key: &Path, kind: KeyKind) -> Result<bool> {
        match fs::remove_file(self.record_path(key, kind)) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e).context("removing trust record"),
        }
    }

    /// Unreadable, corrupt and old-schema record files are skipped.
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
                serde_json::from_slice::<TrustRecord>(&fs::read(path).ok()?)
                    .ok()
                    .filter(|r| r.schema_version == RECORD_SCHEMA_VERSION)
            })
            .collect();
        out.sort_by(|a, b| a.repo_path.cmp(&b.repo_path));
        Ok(out)
    }

    /// True when the store directory resolves under `canonical_root`.
    fn is_inside(&self, canonical_root: &Path) -> bool {
        match std::env::current_dir() {
            Ok(cwd) => resolves_inside(&self.dir, canonical_root, &cwd),
            Err(_) => resolves_inside(&self.dir, canonical_root, Path::new("/")),
        }
    }
}

/// True when `path` resolves under `canonical_root`. `path` may not exist
/// yet, so its nearest existing ancestor stands in; a relative `path` (only
/// possible through `TrustStore::at`) is resolved against `cwd` first.
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

/// A crafted directory name could otherwise use control characters to
/// overwrite the path the user is asked to trust.
pub fn shown(path: &Path) -> String {
    crate::text::escape_control_chars(&path.display().to_string())
}

fn confirm(input: &mut dyn BufRead, out: &mut dyn Write, repo: &Path) -> bool {
    let _ = write!(
        out,
        "toolr hasn't run code from this repository before:\n  {}\n\
         Trusting it also trusts its git worktrees.\n\
         Running its commands executes tools/*.py and syncs tools/pyproject.toml with uv.\n",
        shown(repo)
    );
    crate::prompt::yes_no(input, out, "Trust it?", false)
}

/// The decision flow: env override, existing record, prompt, refuse.
/// Private, so the env override and the store behind a proof are always the
/// real ones (see [`require_trust`]).
fn decide(
    root: &Path,
    store: &TrustStore,
    all_env: bool,
    mode: TrustMode,
    prompt_io: Option<(&mut dyn BufRead, &mut dyn Write)>,
) -> Result<Trusted, TrustError> {
    let home = canonical_home();
    let key = RepoKey::resolve(root, home.as_deref());
    let trusted = |granted_at_prompt| Trusted::from_key(&key, granted_at_prompt);
    if all_env {
        return Ok(trusted(false));
    }
    store.guard_with_home(&key, home.as_deref())?;
    match store.is_trusted_key(&key) {
        Ok(true) => return Ok(trusted(false)),
        Ok(false) => {}
        Err(e) => eprintln!("toolr: warning: could not read the trust store: {e:#}"),
    }

    if !mode.never_prompt
        && let Some((input, out)) = prompt_io
        && confirm(input, out, &key.display)
    {
        if let Err(e) = store.grant_key(&key) {
            let _ = writeln!(out, "warning: could not record trust: {e:#}");
        }
        return Ok(trusted(true));
    }
    Err(TrustError::Untrusted {
        repair: key.repair,
        repo: key.display,
    })
}

/// `TOOLR_TRUST_ALL` set to a true value: the automation bypass.
pub fn trust_all_env() -> bool {
    crate::env::env_truthy("TOOLR_TRUST_ALL")
}

/// Whether `root` is trusted right now, for callers that only decide whether
/// to leave files behind. Never prompts, writes or prints; any error is `false`.
pub fn is_trusted_now(root: &Path) -> bool {
    trust_all_env() || TrustStore::open_default().is_ok_and(|s| s.is_trusted(root).unwrap_or(false))
}

/// Record trust in `root` in the default store, and prove it.
pub fn grant_trust(root: &Path) -> Result<(TrustRecord, Trusted)> {
    TrustStore::open_default()?.grant(root)
}

/// [`require_trust`] that asks on a terminal unless `quiet`, which never prompts.
pub fn require_trust_quietly(root: &Path, quiet: bool) -> Result<Trusted> {
    require_trust(
        root,
        TrustMode {
            never_prompt: quiet,
        },
    )
}

pub fn require_trust(root: &Path, mode: TrustMode) -> Result<Trusted> {
    let root = fs::canonicalize(root)
        .with_context(|| format!("resolving repository root {}", root.display()))?;
    let store = TrustStore::open_default()?;
    let all_env = trust_all_env();
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut stderr = std::io::stderr();
    let prompt_io = stdin.is_terminal().then_some((
        &mut input as &mut dyn BufRead,
        &mut stderr as &mut dyn Write,
    ));
    decide(&root, &store, all_env, mode, prompt_io).map_err(|e| anyhow!(e))
}

#[cfg(test)]
mod tests;
