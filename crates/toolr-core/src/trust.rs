//! Repository trust: toolr refuses to run a repository's code until the user
//! has trusted its git repository (all its worktrees) or, outside git, its path.

use std::fs;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::uv::toolr_state_dir;

mod repo_key;
pub use repo_key::{KeyKind, RepoKey};

const RECORD_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustRecord {
    pub schema_version: u32,
    /// The git common dir, or the project root outside git. What lookups match.
    pub key: PathBuf,
    pub kind: KeyKind,
    /// The repository path shown to the user.
    pub repo_path: PathBuf,
    pub trusted_at: DateTime<Utc>,
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
    #[error("{}", untrusted_message(.repo, .repair.as_deref()))]
    Untrusted {
        /// The repository path shown to the user.
        repo: PathBuf,
        /// The worktree to pass to `git worktree repair` from the main checkout.
        repair: Option<PathBuf>,
    },
    #[error(
        "refusing to use a trust store inside the repository being checked: {}\n\
         Set XDG_STATE_HOME to a directory outside the repository.",
        shown(.0)
    )]
    StoreInsideRepo(PathBuf),
}

fn untrusted_message(repo: &Path, repair: Option<&Path>) -> String {
    let mut msg = format!(
        "refusing to run code from an untrusted repository: {}\n\
         Run `toolr project trust` there once to allow it, or set TOOLR_TRUST_ALL=1 for automation.",
        shown(repo)
    );
    if let Some(worktree) = repair {
        msg.push_str(&format!(
            "\nIf this worktree was moved, run `git worktree repair {}` from the main checkout.",
            shown(worktree)
        ));
    }
    msg
}

/// Canonical form, or the path unchanged when it can't be resolved (the
/// canonical home directory is only used for comparisons).
fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The one place a project root becomes a trust key.
fn resolve_key(root: &Path) -> RepoKey {
    let home = dirs::home_dir().map(|h| canonical(&h));
    RepoKey::resolve(root, home.as_deref())
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

    /// Every store operation passes through here: the store must not resolve
    /// inside the repository whose trust it holds.
    fn guard(&self, key: &RepoKey) -> Result<(), TrustError> {
        self.guard_with_home(key, dirs::home_dir().map(|h| canonical(&h)).as_deref())
    }

    /// Checks the key, the shown repository path (a worktree outside the
    /// repository would otherwise let a store sit in the main checkout) and the
    /// project root. A `~/tools` makes the home directory one of them, and the
    /// default store always lives under it. Nobody clones a hostile repository
    /// as `$HOME`.
    fn guard_with_home(&self, key: &RepoKey, home: Option<&Path>) -> Result<(), TrustError> {
        for path in [&key.key, &key.display, &key.project_root] {
            if home != Some(path.as_path()) && self.is_inside(path) {
                return Err(TrustError::StoreInsideRepo(self.dir.clone()));
            }
        }
        Ok(())
    }

    pub fn is_trusted(&self, root: &Path) -> Result<bool> {
        let key = resolve_key(root);
        self.guard(&key)?;
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
        let key = resolve_key(root);
        self.guard(&key)?;
        let record = TrustRecord {
            schema_version: RECORD_SCHEMA_VERSION,
            key: key.key.clone(),
            kind: key.kind,
            repo_path: key.display.clone(),
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
        fs::rename(&tmp, self.record_path(&key.key, key.kind)).context("writing trust record")?;
        Ok((record, Trusted::from_key(&key, false)))
    }

    /// Returns the repository path to show and whether a record existed. A
    /// project root that no longer exists resolves per-path, so the record of a
    /// deleted non-git project can still be removed.
    pub fn untrust(&self, root: &Path) -> Result<(PathBuf, bool)> {
        let key = resolve_key(root);
        self.guard(&key)?;
        let existed = self.remove_record(&key.key, key.kind)?;
        Ok((key.display, existed))
    }

    /// Removes the records stored under `key` (of either kind), as `toolr self
    /// trusted --prune` does: the key is never re-resolved as a project root.
    /// Returns whether a record existed.
    pub fn untrust_key(&self, key: &Path) -> Result<bool> {
        let repository = self.remove_record(key, KeyKind::Repository)?;
        let per_path = self.remove_record(key, KeyKind::PerPath)?;
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
    path.display()
        .to_string()
        .chars()
        .map(|c| {
            if c.is_control() {
                c.escape_default().to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

fn confirm(input: &mut dyn BufRead, out: &mut dyn Write, repo: &Path) -> bool {
    let _ = write!(
        out,
        "toolr hasn't run code from this repository before:\n  {}\n\
         Trusting it also trusts its git worktrees.\n\
         Running its commands executes tools/*.py and syncs tools/pyproject.toml with uv.\n\
         Trust it? [y/N] ",
        shown(repo)
    );
    let _ = out.flush();
    let mut answer = String::new();
    match input.read_line(&mut answer) {
        Ok(n) if n > 0 => matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"),
        _ => false,
    }
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
    let key = resolve_key(root);
    let trusted = |granted_at_prompt| Trusted::from_key(&key, granted_at_prompt);
    if all_env {
        return Ok(trusted(false));
    }
    store.guard(&key)?;
    match store.is_trusted(root) {
        Ok(true) => return Ok(trusted(false)),
        Ok(false) => {}
        Err(e) => eprintln!("toolr: warning: could not read the trust store: {e:#}"),
    }

    if !mode.never_prompt
        && let Some((input, out)) = prompt_io
        && confirm(input, out, &key.display)
    {
        if let Err(e) = store.trust(root) {
            let _ = writeln!(out, "warning: could not record trust: {e:#}");
        }
        return Ok(trusted(true));
    }
    Err(TrustError::Untrusted {
        repair: key.repair_hint.then(|| key.project_root.clone()),
        repo: key.display,
    })
}

/// `TOOLR_TRUST_ALL=1`, exactly: the automation bypass.
pub fn trust_all_env() -> bool {
    std::env::var_os("TOOLR_TRUST_ALL").is_some_and(|v| v == "1")
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
mod tests {
    use std::io::Cursor;

    use tempfile::TempDir;

    use super::*;

    struct Fixture {
        repo: TempDir,
        store_dir: TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                repo: TempDir::new().unwrap(),
                store_dir: TempDir::new().unwrap(),
            }
        }
        fn root(&self) -> PathBuf {
            fs::canonicalize(self.repo.path()).unwrap()
        }
        fn store(&self) -> TrustStore {
            TrustStore::at(self.store_dir.path().join("trust"))
        }
        fn decide(
            &self,
            input: Option<&str>,
            mode: TrustMode,
        ) -> (Result<Trusted, TrustError>, String) {
            let store = self.store();
            let mut err = Vec::new();
            let result = match input {
                Some(text) => {
                    let mut cursor = Cursor::new(text.as_bytes().to_vec());
                    decide(
                        &self.root(),
                        &store,
                        false,
                        mode,
                        Some((&mut cursor, &mut err)),
                    )
                }
                None => decide(&self.root(), &store, false, mode, None),
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
        assert!(matches!(r, Err(TrustError::Untrusted { repo, repair: None }) if repo == f.root()));
    }

    #[test]
    fn untrusted_display_is_the_refusal_text() {
        let msg = TrustError::Untrusted {
            repo: PathBuf::from("/r"),
            repair: None,
        }
        .to_string();
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
        let outcome = decide(&f.root(), &store, true, PROMPT, None).unwrap();
        assert!(!outcome.granted_at_prompt());
        assert!(outcome.covers(&f.root()));
        assert!(!store.is_trusted(&f.root()).unwrap());
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn recorded_root_is_allowed() {
        let f = Fixture::new();
        f.store().trust(&f.root()).unwrap();
        let (r, _) = f.decide(None, PROMPT);
        assert!(!r.unwrap().granted_at_prompt());
    }

    #[test]
    fn untrust_removes_record() {
        let f = Fixture::new();
        let store = f.store();
        store.trust(&f.root()).unwrap();
        assert_eq!(store.untrust(&f.root()).unwrap(), (f.root(), true));
        assert!(!store.is_trusted(&f.root()).unwrap());
        assert!(!store.untrust(&f.root()).unwrap().1);
    }

    #[test]
    fn prompt_yes_records_trust() {
        let f = Fixture::new();
        let (r, err) = f.decide(Some("y\n"), PROMPT);
        let proof = r.unwrap();
        assert!(proof.granted_at_prompt());
        assert_eq!(proof.root(), f.root());
        assert!(f.store().is_trusted(&f.root()).unwrap());
        assert!(err.contains("Trust it? [y/N]"));
        assert!(err.contains(&f.root().display().to_string()));
    }

    #[test]
    fn grant_proves_only_the_root_it_recorded() {
        let f = Fixture::new();
        let other = TempDir::new().unwrap();
        let (record, proof) = f.store().grant(&f.root()).unwrap();
        assert_eq!(record.repo_path, f.root());
        assert_eq!(record.key, f.root());
        assert!(!proof.granted_at_prompt());
        assert!(proof.covers(f.repo.path()));
        assert!(!proof.covers(other.path()));
    }

    #[test]
    fn prompt_default_and_eof_refuse() {
        for input in ["\n", ""] {
            let f = Fixture::new();
            let (r, _) = f.decide(Some(input), PROMPT);
            assert!(
                matches!(r, Err(TrustError::Untrusted { .. })),
                "input {input:?}"
            );
            assert!(!f.store().is_trusted(&f.root()).unwrap());
        }
    }

    #[test]
    fn shown_escapes_control_characters() {
        assert_eq!(
            shown(Path::new("evil\r~/mine\x1b[2K")),
            "evil\\r~/mine\\u{1b}[2K"
        );
    }

    #[test]
    fn never_prompt_refuses_even_on_tty() {
        let f = Fixture::new();
        let (r, err) = f.decide(Some("y\n"), TrustMode { never_prompt: true });
        assert!(matches!(r, Err(TrustError::Untrusted { .. })));
        assert!(err.is_empty());
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
        let r = decide(&f.root(), &store, false, PROMPT, None);
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
        let key = RepoKey::resolve(&root, None);
        assert!(store.guard_with_home(&key, Some(&root)).is_ok());
        assert!(matches!(
            store.guard_with_home(&key, None),
            Err(TrustError::StoreInsideRepo(_))
        ));
    }

    #[test]
    fn relative_store_inside_repo_is_rejected() {
        let f = Fixture::new();
        let store = TrustStore::at(PathBuf::from("state/trust"));
        assert!(resolves_inside(store.dir(), &f.root(), &f.root()));
        assert!(!resolves_inside(store.dir(), &f.root(), f.store_dir.path()));
    }

    #[cfg(unix)]
    #[test]
    fn store_reaching_repo_through_symlink_is_rejected() {
        let f = Fixture::new();
        let link_parent = TempDir::new().unwrap();
        let link = link_parent.path().join("link");
        std::os::unix::fs::symlink(f.root(), &link).unwrap();
        let store = TrustStore::at(link.join("state"));
        let r = decide(&f.root(), &store, false, PROMPT, None);
        assert!(matches!(r, Err(TrustError::StoreInsideRepo(_))));
        assert!(!f.root().join("state").exists());
    }

    // Windows reports a path under a file as NotFound, which reads as untrusted.
    #[cfg(unix)]
    #[test]
    fn unusable_store_fails_closed() {
        let f = Fixture::new();
        fs::write(f.store().dir(), b"not a directory").unwrap();
        let (r, _) = f.decide(None, PROMPT);
        assert!(matches!(r, Err(TrustError::Untrusted { .. })));
        assert!(f.store().is_trusted(&f.root()).is_err());
        assert!(f.store().list().is_err());
    }

    #[test]
    fn yes_with_unusable_store_runs_and_warns() {
        let f = Fixture::new();
        fs::write(f.store().dir(), b"not a directory").unwrap();
        let (r, err) = f.decide(Some("y\n"), PROMPT);
        assert!(r.is_ok());
        assert!(err.contains("warning: could not record trust"), "{err}");
    }

    #[test]
    fn record_that_is_a_directory_is_an_error_not_untrusted() {
        let f = Fixture::new();
        let store = f.store();
        store.trust(&f.root()).unwrap();
        let record = store.record_path(&f.root(), KeyKind::PerPath);
        fs::remove_file(&record).unwrap();
        fs::create_dir(&record).unwrap();
        assert!(store.is_trusted(&f.root()).is_err());
        assert!(store.untrust(&f.root()).is_err());
    }

    #[test]
    fn list_skips_leftover_temp_files() {
        let f = Fixture::new();
        let store = f.store();
        store.trust(&f.root()).unwrap();
        fs::write(store.dir().join(".trust.abc123.tmp"), b"{}").unwrap();
        assert_eq!(store.list().unwrap().len(), 1);
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

    /// `TOOLR_TRUST_ALL` answers before the store is consulted, even one inside the
    /// repository; otherwise an unusable store or unknown root is untrusted.
    /// Nothing creates the store directory.
    #[test]
    fn is_trusted_now_reads_env_then_store_and_creates_nothing() {
        let f = Fixture::new();
        let mut env = crate::test_env::EnvGuard::write();
        env.set("XDG_STATE_HOME", f.root());
        env.set("TOOLR_TRUST_ALL", "1");
        assert!(is_trusted_now(&f.root()));

        env.remove("TOOLR_TRUST_ALL");
        assert!(!is_trusted_now(&f.root()));
        env.set("XDG_STATE_HOME", f.store_dir.path());
        assert!(!is_trusted_now(&f.root()));

        assert!(!f.root().join("toolr").exists());
        assert!(!f.store_dir.path().join("toolr").exists());
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args([
                "-c",
                "init.defaultBranch=main",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_CEILING_DIRECTORIES")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn init_repo(path: &Path) -> PathBuf {
        fs::create_dir_all(path).unwrap();
        git(path, &["init", "-q"]);
        git(path, &["commit", "-q", "--allow-empty", "-m", "init"]);
        fs::canonicalize(path).unwrap()
    }

    fn add_worktree(repo: &Path, wt: &Path) -> PathBuf {
        git(
            repo,
            &["worktree", "add", "-q", "--detach", wt.to_str().unwrap()],
        );
        fs::canonicalize(wt).unwrap()
    }

    fn decide_at(
        store: &TrustStore,
        root: &Path,
        input: Option<&str>,
    ) -> (Result<Trusted, TrustError>, String) {
        let mut err = Vec::new();
        let result = match input {
            Some(text) => {
                let mut cursor = Cursor::new(text.as_bytes().to_vec());
                decide(root, store, false, PROMPT, Some((&mut cursor, &mut err)))
            }
            None => decide(root, store, false, PROMPT, None),
        };
        (result, String::from_utf8(err).unwrap())
    }

    #[test]
    fn worktree_proof_covers_main_and_keeps_its_own_root() {
        let f = Fixture::new();
        let tmp = TempDir::new().unwrap();
        let main = init_repo(&tmp.path().join("main"));
        let wt = add_worktree(&main, &tmp.path().join("wt"));
        let unrelated = TempDir::new().unwrap();
        let (_, proof) = f.store().grant(&wt).unwrap();
        assert_eq!(proof.root(), wt);
        assert!(proof.covers(&main));
        assert!(!proof.covers(unrelated.path()));
    }

    #[test]
    fn trust_main_then_worktree_is_trusted() {
        let f = Fixture::new();
        let tmp = TempDir::new().unwrap();
        let main = init_repo(&tmp.path().join("main"));
        let inside = add_worktree(&main, &main.join(".worktrees/in"));
        let outside = add_worktree(&main, &tmp.path().join("out"));
        let store = f.store();
        store.trust(&main).unwrap();
        assert!(store.is_trusted(&inside).unwrap());
        assert!(store.is_trusted(&outside).unwrap());
        let (r, _) = decide_at(&store, &outside, None);
        assert_eq!(r.unwrap().root(), outside);
    }

    #[test]
    fn untrust_from_worktree_untrusts_main() {
        let f = Fixture::new();
        let tmp = TempDir::new().unwrap();
        let main = init_repo(&tmp.path().join("main"));
        let wt = add_worktree(&main, &tmp.path().join("wt"));
        let store = f.store();
        store.trust(&wt).unwrap();
        assert!(store.is_trusted(&main).unwrap());
        assert_eq!(store.untrust(&wt).unwrap(), (main.clone(), true));
        assert!(!store.is_trusted(&main).unwrap());
        assert!(!store.is_trusted(&wt).unwrap());
    }

    #[test]
    fn two_projects_in_one_repo_share_trust() {
        let f = Fixture::new();
        let tmp = TempDir::new().unwrap();
        let main = init_repo(&tmp.path().join("main"));
        fs::create_dir_all(main.join("a")).unwrap();
        fs::create_dir_all(main.join("b")).unwrap();
        let store = f.store();
        store.trust(&main.join("a")).unwrap();
        assert!(store.is_trusted(&main.join("b")).unwrap());
    }

    #[test]
    fn record_stores_key_and_display() {
        let f = Fixture::new();
        let tmp = TempDir::new().unwrap();
        let main = init_repo(&tmp.path().join("main"));
        let record = f.store().trust(&main).unwrap();
        assert_eq!(record.key, main.join(".git"));
        assert_eq!(record.repo_path, main);
        assert_eq!(record.kind, KeyKind::Repository);
        assert_eq!(record.schema_version, 2);
    }

    #[test]
    fn untrust_key_removes_by_stored_key() {
        let f = Fixture::new();
        let tmp = TempDir::new().unwrap();
        let main = init_repo(&tmp.path().join("main"));
        let store = f.store();
        let record = store.trust(&main).unwrap();
        assert!(store.untrust_key(&record.key).unwrap());
        assert!(!store.untrust_key(&record.key).unwrap());
        assert!(!store.is_trusted(&main).unwrap());
    }

    #[test]
    fn list_skips_old_schema_records() {
        let f = Fixture::new();
        let store = f.store();
        store.trust(&f.root()).unwrap();
        fs::write(
            store.dir().join("old.json"),
            r#"{"schema_version":1,"repo_path":"/old","trusted_at":"2026-01-01T00:00:00Z","toolr_version":"0"}"#,
        )
        .unwrap();
        let mut stale = serde_json::to_value(&store.list().unwrap()[0]).unwrap();
        stale["schema_version"] = 1.into();
        fs::write(store.dir().join("stale.json"), stale.to_string()).unwrap();
        assert_eq!(store.list().unwrap().len(), 1);
        fs::remove_file(store.record_path(&f.root(), KeyKind::PerPath)).unwrap();
        fs::write(
            store.record_path(&f.root(), KeyKind::PerPath),
            stale.to_string(),
        )
        .unwrap();
        assert!(!store.is_trusted(&f.root()).unwrap());
    }

    #[test]
    fn missing_project_root_untrusts_cleanly() {
        let f = Fixture::new();
        let tmp = TempDir::new().unwrap();
        let dir = fs::canonicalize(tmp.path()).unwrap().join("gone");
        fs::create_dir(&dir).unwrap();
        let store = f.store();
        store.trust(&dir).unwrap();
        fs::remove_dir(&dir).unwrap();
        assert_eq!(store.untrust(&dir).unwrap(), (dir, true));
    }

    #[test]
    fn store_in_main_checkout_refused_from_external_worktree() {
        let f = Fixture::new();
        let tmp = TempDir::new().unwrap();
        let main = init_repo(&tmp.path().join("main"));
        let wt = add_worktree(&main, &tmp.path().join("wt"));
        let store = TrustStore::at(main.join("state"));
        let (r, _) = decide_at(&store, &wt, None);
        assert!(matches!(r, Err(TrustError::StoreInsideRepo(_))));
        assert!(!main.join("state").exists());
        drop(f);
    }

    #[test]
    fn home_dotgit_project_not_refused_by_guard() {
        let f = Fixture::new();
        let tmp = TempDir::new().unwrap();
        let home = init_repo(&tmp.path().join("home"));
        let store = TrustStore::at(home.join("state/trust"));
        let key = RepoKey::resolve(&home, Some(&home));
        assert_eq!(key.key, home.join(".git"));
        assert_eq!(key.display, home);
        assert!(store.guard_with_home(&key, Some(&home)).is_ok());
        assert!(store.guard_with_home(&key, None).is_err());
        drop(f);
    }

    #[test]
    fn untrusted_error_carries_repair_hint() {
        let f = Fixture::new();
        let tmp = TempDir::new().unwrap();
        let main = init_repo(&tmp.path().join("main"));
        add_worktree(&main, &tmp.path().join("wt"));
        fs::rename(tmp.path().join("wt"), tmp.path().join("moved")).unwrap();
        let moved = fs::canonicalize(tmp.path().join("moved")).unwrap();
        let store = f.store();
        let (r, _) = decide_at(&store, &moved, None);
        let err = r.unwrap_err();
        assert!(matches!(
            &err,
            TrustError::Untrusted { repair: Some(w), .. } if *w == moved
        ));
        let text = err.to_string();
        assert!(text.contains("git worktree repair"), "{text}");
        assert!(text.contains(&moved.display().to_string()), "{text}");
    }

    #[test]
    fn prompt_mentions_worktrees() {
        let f = Fixture::new();
        let (_, err) = f.decide(Some("n\n"), PROMPT);
        assert!(err.contains("worktrees"), "{err}");
    }

    #[test]
    fn forged_worktree_layout_does_not_borrow_a_per_path_record() {
        let f = Fixture::new();
        let tmp = TempDir::new().unwrap();
        let p = fs::canonicalize(tmp.path()).unwrap();
        let store = f.store();
        store.trust(&p).unwrap();
        fs::create_dir_all(p.join("worktrees/proj")).unwrap();
        fs::create_dir_all(p.join("worktrees/g")).unwrap();
        fs::write(p.join("worktrees/proj/.git"), "gitdir: ../g\n").unwrap();
        fs::write(p.join("worktrees/g/gitdir"), "../proj/.git\n").unwrap();
        let forged = p.join("worktrees/proj");
        let key = resolve_key(&forged);
        assert_eq!(
            (key.key.as_path(), key.kind),
            (p.as_path(), KeyKind::Repository)
        );
        assert!(!store.is_trusted(&forged).unwrap());
        assert!(store.is_trusted(&p).unwrap());
    }
}
