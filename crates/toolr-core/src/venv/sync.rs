//! Drive `uv sync` to materialise the tools venv.

use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, ExitStatus};
use std::time::SystemTime;

use anyhow::{Context, Result};

use crate::trust::{TrustStore, shown, trust_all_env};
use crate::uv::{UvBinary, UvError};

use super::pyvenv_cfg::pyvenv_cfg_value;
use super::resolve::ResolvedVenv;

/// Marker file written into the venv after each successful sync.
/// Its mtime is compared against the [`SYNC_INPUTS`] mtimes to decide
/// whether re-sync is needed. In an in-tree venv it holds the token the trust
/// store recorded for it, so toolr can tell a venv it built from a planted one.
const FRESHNESS_MARKER: &str = ".toolr-sync-stamp";

/// Optional files in `tools/` that uv reads to build the venv, besides the
/// required `uv.lock`.
const SYNC_INPUTS: [&str; 3] = ["pyproject.toml", "uv.toml", ".python-version"];

/// `uv.lock` plus [`SYNC_INPUTS`], for messages that name them.
pub const SYNC_INPUTS_DESCRIPTION: &str =
    "tools/pyproject.toml, uv.lock, uv.toml or .python-version";

/// Decision returned by `check_freshness`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// No venv to use: it was never synced there, or the marker or `uv.lock` is
    /// absent. An in-tree venv that exists is `Foreign` or built by toolr, never this.
    Missing,
    /// An in-tree venv toolr didn't build: its marker doesn't hold the token
    /// the trust store has for it. Never reported for a cache-located venv or
    /// under `TOOLR_TRUST_ALL`. Remove it, never run it, then sync.
    Foreign,
    /// `uv.lock` or a [`SYNC_INPUTS`] file changed since the last sync.
    Stale,
    /// Synced once, but the interpreter no longer resolves — typically its
    /// base Python was uninstalled (e.g. `mise prune`). `uv sync` rebuilds it.
    Broken,
    /// Marker at least as new as every sync input, and the interpreter is still there.
    Fresh,
}

/// Argument shape for `uv lock` / `uv sync` upgrade behavior.
///
/// Mirrors uv's two flags exactly:
/// - `--upgrade` / `-U` → re-resolve every package (`All`).
/// - `--upgrade-package <pkg>` / `-P <pkg>` (repeatable) → re-resolve
///   the listed packages (`Packages`).
///
/// `None` is the default and produces no extra args.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum UpgradeMode {
    #[default]
    None,
    All,
    Packages(Vec<String>),
}

impl UpgradeMode {
    /// Append the matching uv argv tokens (`--upgrade` / repeated
    /// `--upgrade-package <pkg>`) onto `cmd`. No-op for `None`.
    pub(crate) fn append_args(&self, cmd: &mut std::process::Command) {
        match self {
            UpgradeMode::None => {}
            UpgradeMode::All => {
                cmd.arg("--upgrade");
            }
            UpgradeMode::Packages(pkgs) => {
                for p in pkgs {
                    cmd.arg("--upgrade-package").arg(p);
                }
            }
        }
    }
}

/// Precedence is `Foreign`, `Missing`, then `Broken`, then `Stale` or `Fresh`.
/// `uv.lock` is required; an absent [`SYNC_INPUTS`] file is ignored. Besides
/// the mtimes it stats the interpreter and reads `pyvenv.cfg`, and for an
/// in-tree venv the default trust store; a store it can't open means `Foreign`.
pub fn check_freshness(resolved: &ResolvedVenv, tools_dir: &Path) -> Freshness {
    with_default_check(resolved, |check| {
        check_freshness_with(resolved, tools_dir, check)
    })
}

/// For callers that read the venv's files without running it, such as the plugin-fragment glob.
pub fn is_foreign(resolved: &ResolvedVenv, root: &Path) -> bool {
    with_default_check(resolved, |check| is_foreign_with(resolved, root, check))
}

/// What vouches for an in-tree venv's token.
#[derive(Clone, Copy)]
pub(crate) enum TokenCheck<'a> {
    /// No check: a cache venv, or `TOOLR_TRUST_ALL`.
    Skip,
    /// The store the token must be recorded in.
    Store(&'a TrustStore),
    /// The default store couldn't be opened, so nothing vouches for the venv (fail-closed).
    NoStore,
}

/// Runs `f` with the check the default store and `TOOLR_TRUST_ALL` call for. Cache venvs need
/// neither, so they read no env and no store.
fn with_default_check<R>(resolved: &ResolvedVenv, f: impl FnOnce(TokenCheck<'_>) -> R) -> R {
    if !resolved.is_in_tree() || trust_all_env() {
        return f(TokenCheck::Skip);
    }
    match TrustStore::open_default() {
        Ok(store) => f(TokenCheck::Store(&store)),
        Err(_) => f(TokenCheck::NoStore),
    }
}

fn is_foreign_with(resolved: &ResolvedVenv, root: &Path, check: TokenCheck<'_>) -> bool {
    resolved.is_in_tree()
        && !matches!(check, TokenCheck::Skip)
        && fs::symlink_metadata(&resolved.venv_dir).is_ok()
        && !built_by_toolr(resolved, root, check)
}

pub(crate) fn check_freshness_with(
    resolved: &ResolvedVenv,
    tools_dir: &Path,
    check: TokenCheck<'_>,
) -> Freshness {
    if is_foreign_with(resolved, project_root(tools_dir), check) {
        return Freshness::Foreign;
    }
    let marker = resolved.venv_dir.join(FRESHNESS_MARKER);
    let lock = tools_dir.join("uv.lock");
    let (Ok(marker_meta), Ok(lock_meta)) = (fs::metadata(&marker), fs::metadata(&lock)) else {
        return Freshness::Missing;
    };
    // `is_file` follows symlinks, so a dangling interpreter link counts as gone.
    if !resolved.python.is_file() || base_python_removed(&resolved.venv_dir) {
        return Freshness::Broken;
    }
    let mtime = |meta: fs::Metadata| meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let newest_input = SYNC_INPUTS
        .iter()
        .filter_map(|name| fs::metadata(tools_dir.join(name)).ok())
        .map(mtime)
        .fold(mtime(lock_meta), SystemTime::max);
    if mtime(marker_meta) >= newest_input {
        Freshness::Fresh
    } else {
        Freshness::Stale
    }
}

/// The store vouches for a venv only while it trusts the repository, a check
/// that also refuses a store inside it.
fn built_by_toolr(resolved: &ResolvedVenv, root: &Path, check: TokenCheck<'_>) -> bool {
    let TokenCheck::Store(store) = check else {
        return false;
    };
    if !store.is_trusted(root).unwrap_or(false) {
        return false;
    }
    marker_token(store, &resolved.venv_dir).is_some() || pending_build(store, &resolved.venv_dir)
}

/// toolr claimed the venv before uv ran and no marker was written since: a sync that failed
/// or was killed part way. A marker that doesn't match still means someone else's venv.
fn pending_build(store: &TrustStore, venv_dir: &Path) -> bool {
    store.venv_pending(venv_dir) && fs::symlink_metadata(venv_dir.join(FRESHNESS_MARKER)).is_err()
}

/// The project root above `tools_dir`.
fn project_root(tools_dir: &Path) -> &Path {
    tools_dir.parent().unwrap_or(tools_dir)
}

/// The longest marker toolr writes: a hex token and a newline.
const MAX_MARKER: u64 = (crate::trust::TOKEN_BYTES * 2 + 1) as u64;

/// The marker's text, when it is a regular file no longer than [`MAX_MARKER`]: a committed
/// marker could be a symlink to `/dev/zero` or a FIFO.
fn read_marker(venv_dir: &Path) -> Option<String> {
    use std::io::Read;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let path = venv_dir.join(FRESHNESS_MARKER);
    // Unix refuses a link at open, which leaves no gap for a swap.
    #[cfg(not(unix))]
    if !fs::symlink_metadata(&path).ok()?.is_file() {
        return None;
    }
    let file = options.open(&path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    file.take(MAX_MARKER + 1).read_to_string(&mut text).ok()?;
    if text.len() as u64 > MAX_MARKER {
        return None;
    }
    Some(text.strip_suffix('\n').unwrap_or(&text).to_owned())
}

/// The store's token for `venv_dir`, when the venv's marker holds it.
fn marker_token(store: &TrustStore, venv_dir: &Path) -> Option<String> {
    let marker = read_marker(venv_dir)?;
    store
        .venv_token(venv_dir)
        .ok()
        .flatten()
        .filter(|token| *token == marker)
}

/// Never follows a symlink: a linked `tools/.venv` loses only the link. A
/// `tools` that is itself a symlink could be another project's, so that
/// layout is refused rather than emptied.
fn remove_foreign_venv(tools_dir: &Path, venv_dir: &Path) -> std::io::Result<()> {
    let real_tools = fs::symlink_metadata(tools_dir).is_ok_and(|m| m.is_dir());
    if !real_tools || venv_dir.parent() != Some(tools_dir) {
        return Err(std::io::Error::other(format!(
            "refusing to remove {}: {} isn't a real directory holding it. Make tools/ a \
             plain directory of the repository, then run `toolr project venv sync`",
            shown(venv_dir),
            shown(tools_dir)
        )));
    }
    eprintln!(
        "toolr: removing {}: toolr didn't build this in-tree venv",
        shown(venv_dir)
    );
    let meta = fs::symlink_metadata(venv_dir)?;
    if meta.is_dir() {
        return fs::remove_dir_all(venv_dir);
    }
    // Windows links a directory with a symlink `remove_file` can't delete.
    #[cfg(windows)]
    if meta.file_type().is_symlink() && fs::remove_dir(venv_dir).is_ok() {
        return Ok(());
    }
    fs::remove_file(venv_dir)
}

/// Whether `pyvenv.cfg`'s `home` is gone. Windows venvs hold a copied launcher,
/// not a symlink, so their own interpreter outlives the base Python.
fn base_python_removed(venv_dir: &Path) -> bool {
    // Only a confirmed absence counts: an unreadable `home` would otherwise
    // re-sync a working venv on every dispatch.
    pyvenv_cfg_value(venv_dir, "home")
        .is_some_and(|home| matches!(Path::new(&home).try_exists(), Ok(false)))
}

/// Run `uv sync --project <tools>` synchronously, inheriting stdio.
/// When `quiet` is true, passes `--quiet` to uv so the subprocess
/// produces no informational output on success.
pub fn run_uv_sync(
    uv: &UvBinary,
    tools_dir: &Path,
    resolved: &ResolvedVenv,
    upgrade: &UpgradeMode,
    quiet: bool,
) -> Result<ExitStatus> {
    // Ensure the parent of an off-tree venv exists so uv can write into it.
    if let Some(parent) = resolved.venv_dir.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut cmd = Command::new(&uv.path); // nosemgrep: rust.actix.command-injection.rust-actix-command-injection.rust-actix-command-injection
    cmd.arg("sync")
        .arg("--project")
        .arg(tools_dir)
        .env("UV_PROJECT_ENVIRONMENT", &resolved.venv_dir)
        // Unset any outer VIRTUAL_ENV so uv doesn't warn about a mismatch
        // with the tools venv (e.g. when invoked inside a mise-managed .venv).
        .env_remove("VIRTUAL_ENV");
    if quiet {
        cmd.arg("--quiet");
    }
    if let Some(version) = resolved.config.python_version.as_ref() {
        cmd.arg("--python").arg(version);
    }
    upgrade.append_args(&mut cmd);
    let status = cmd
        .status()
        .with_context(|| format!("spawning uv at {}", uv.path.display()))?;
    if status.success() {
        stamp_marker(resolved, tools_dir)?;
    }
    Ok(status)
}

/// Run `uv lock --project <tools>` synchronously, inheriting stdio.
/// Used by `toolr project venv lock` to refresh `tools/uv.lock` without
/// applying the new pins to the venv. The `upgrade` arg controls
/// whether `uv` is asked to re-resolve some or all dependencies; see
/// [`UpgradeMode`].
pub fn run_uv_lock(
    uv: &UvBinary,
    tools_dir: &Path,
    resolved: &ResolvedVenv,
    upgrade: &UpgradeMode,
    quiet: bool,
) -> Result<ExitStatus> {
    let mut cmd = Command::new(&uv.path); // nosemgrep: rust.actix.command-injection.rust-actix-command-injection.rust-actix-command-injection
    cmd.arg("lock")
        .arg("--project")
        .arg(tools_dir)
        .env("UV_PROJECT_ENVIRONMENT", &resolved.venv_dir)
        .env_remove("VIRTUAL_ENV");
    if quiet {
        cmd.arg("--quiet");
    }
    if let Some(version) = resolved.config.python_version.as_ref() {
        cmd.arg("--python").arg(version);
    }
    upgrade.append_args(&mut cmd);
    cmd.status()
        .with_context(|| format!("spawning uv at {}", uv.path.display()))
}

/// Convenience wrapper that maps a failure to `UvError::SyncFailed`.
/// `quiet` is forwarded to `run_uv_sync` so the inner uv subprocess
/// inherits the same output discipline.
pub fn sync_if_needed(
    uv: &UvBinary,
    tools_dir: &Path,
    resolved: &ResolvedVenv,
    force: bool,
    quiet: bool,
    upgrade: &UpgradeMode,
) -> Result<(), UvError> {
    // -U / -P explicitly ask for movement; never short-circuit on freshness.
    let bypass_stamp = force || !matches!(upgrade, UpgradeMode::None);
    // Only an in-tree venv's token needs the store; without its lock toolr can't record one.
    let lock = || lock_in_tree(resolved, quiet);
    sync_locked(
        resolved,
        tools_dir,
        bypass_stamp,
        SyncSteps {
            check: || check_freshness(resolved, tools_dir),
            lock,
            approve: || approve_removal_on_tty(&resolved.venv_dir, quiet),
            sync: || {
                claim_before_build(resolved, tools_dir);
                // Not `Http`: its message tells the user to install uv, which they already have.
                let status = run_uv_sync(uv, tools_dir, resolved, upgrade, quiet).map_err(|e| {
                    let msg = format!("couldn't run `uv sync` with {}: {e:#}", uv.path.display());
                    UvError::Io(std::io::Error::other(msg))
                })?;
                if !status.success() {
                    return Err(UvError::SyncFailed(status.code()));
                }
                Ok(())
            },
        },
    )
}

/// Set truthy, it approves replacing an in-tree venv toolr didn't build without asking, for
/// runs with no terminal. Syncing consent (`TOOLR_AUTO_SYNC`) never does.
pub const REPLACE_VENV_ENV: &str = "TOOLR_REPLACE_UNTRUSTED_VENV";

/// Why toolr won't run an in-tree venv it didn't build: the one wording for the removal prompt,
/// the refusal and every command that would read or run the venv.
pub fn untrusted_venv_reason(venv_dir: &Path) -> String {
    format!(
        "toolr didn't build {} since you trusted this repository, so toolr won't run it: \
         it may have come with the repository, and running it would run code you never approved",
        shown(venv_dir)
    )
}

/// Approve removing a venv toolr didn't build: [`REPLACE_VENV_ENV`], or a `y` on the terminal.
/// `quiet` never asks.
fn approve_removal_on_tty(venv_dir: &Path, quiet: bool) -> Result<(), UvError> {
    use std::io::IsTerminal as _;
    let by_env = crate::env::env_truthy(REPLACE_VENV_ENV);
    let stdin = std::io::stdin();
    let ask = !by_env && !quiet && stdin.is_terminal();
    let prompt_io = ask.then(|| (stdin.lock(), std::io::stderr()));
    match prompt_io {
        Some((mut input, mut out)) => {
            approve_removal(venv_dir, by_env, Some((&mut input, &mut out)))
        }
        None => approve_removal(venv_dir, by_env, None),
    }
}

fn approve_removal(
    venv_dir: &Path,
    by_env: bool,
    prompt_io: Option<(&mut dyn std::io::BufRead, &mut dyn std::io::Write)>,
) -> Result<(), UvError> {
    if by_env {
        return Ok(());
    }
    let reason = untrusted_venv_reason(venv_dir);
    if let Some((input, out)) = prompt_io {
        let _ = writeln!(out, "toolr: {reason}.");
        if crate::prompt::yes_no(input, out, "Remove it and build a fresh one?", false) {
            return Ok(());
        }
    }
    Err(UvError::UntrustedVenv(format!(
        "{reason}. Nothing was removed. To replace it, run `toolr project venv sync` on a \
         terminal and answer y, set {REPLACE_VENV_ENV}=1, or remove {} yourself",
        shown(venv_dir)
    )))
}

/// Which venv sits at `venv_dir`, so a removal approved before the lock is applied only to
/// the venv the user was asked about. `None` when there is no venv or it can't be read, so an
/// approval never carries over a lock then.
fn venv_identity(venv_dir: &Path) -> Option<(u64, u64)> {
    let meta = fs::symlink_metadata(venv_dir).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some((meta.dev(), meta.ino()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // NTFS name tunnelling can keep the creation time across a fast delete and recreate;
        // acceptable, as the window is only between the user's answer and taking the lock.
        Some((meta.creation_time(), 0))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = meta;
        None
    }
}

/// An approval holds only for the venv it was given for: an unknown identity never matches.
fn still_approved(approved: Option<(u64, u64)>, now: Option<(u64, u64)>) -> bool {
    approved.is_some() && approved == now
}

/// The seams of [`sync_locked`], so tests drive it with closures.
struct SyncSteps<C, L, A, S> {
    /// The venv's freshness now.
    check: C,
    /// Take the sync lock; the guard it returns is held until the sync is done.
    lock: L,
    /// Approve removing a venv toolr didn't build.
    approve: A,
    /// Run uv.
    sync: S,
}

/// Check; for a venv toolr didn't build, ask `approve` before taking the lock, so no other
/// toolr waits on the answer. Then lock, re-check, and remove such a venv only if it is still
/// the one approved, asking again only for a different one; then sync. Another toolr may be
/// building this venv, and until it stamps the marker the venv reads as one toolr didn't
/// build. Without the lock toolr never removes such a venv: it fails naming the store, which
/// couldn't record a rebuilt venv's token either. Any other venv syncs unlocked.
fn sync_locked<G>(
    resolved: &ResolvedVenv,
    tools_dir: &Path,
    bypass_stamp: bool,
    steps: SyncSteps<
        impl Fn() -> Freshness,
        impl FnOnce() -> Result<G>,
        impl FnMut() -> Result<(), UvError>,
        impl FnOnce() -> Result<(), UvError>,
    >,
) -> Result<(), UvError> {
    let SyncSteps {
        check,
        lock,
        mut approve,
        sync,
    } = steps;
    let before = check();
    if !bypass_stamp && before == Freshness::Fresh {
        return Ok(());
    }
    let venv_dir = &resolved.venv_dir;
    let approved = if before == Freshness::Foreign {
        approve()?;
        venv_identity(venv_dir)
    } else {
        None
    };
    let lock = lock();
    let state = check();
    let _lock = match lock {
        Err(e) if state == Freshness::Foreign => {
            return Err(UvError::UntrustedVenv(format!("{e:#}")));
        }
        held => held.ok(),
    };
    match state {
        // uv runs an existing venv's interpreter to inspect it, so a planted one goes first.
        Freshness::Foreign => {
            if !still_approved(approved, venv_identity(venv_dir)) {
                approve()?;
            }
            remove_foreign_venv(tools_dir, venv_dir).map_err(|e| {
                UvError::UntrustedVenv(format!("couldn't remove {}: {e}", shown(venv_dir)))
            })?;
        }
        Freshness::Fresh if !bypass_stamp => return Ok(()),
        _ => {}
    }
    sync()
}

/// The sync lock of an in-tree venv whose token toolr checks; `None` for any other venv.
pub(super) fn lock_in_tree(
    resolved: &ResolvedVenv,
    quiet: bool,
) -> Result<Option<crate::trust::VenvLock>> {
    if !resolved.is_in_tree() || trust_all_env() {
        return Ok(None);
    }
    TrustStore::open_default()?
        .lock_venv(&resolved.venv_dir, quiet)
        .map(Some)
}

/// Under the lock, just before uv runs: record that toolr is building this in-tree venv (see
/// [`TrustStore::claim_venv`]). Best effort: without the claim a failed first sync leaves a
/// venv toolr must ask about before replacing, as it did before claims existed.
fn claim_before_build(resolved: &ResolvedVenv, tools_dir: &Path) {
    if !resolved.is_in_tree() || trust_all_env() {
        return;
    }
    // Store or claim failures are ignored: the venv then reads as one toolr didn't build and
    // the user is asked before it is replaced (fail-safe).
    if let Ok(store) = TrustStore::open_default() {
        let _ = store.claim_venv(project_root(tools_dir), &resolved.venv_dir);
    }
}

/// Write the marker after toolr built or changed the venv: for an in-tree
/// venv, with a fresh token recorded in the default trust store.
pub(super) fn stamp_marker(resolved: &ResolvedVenv, tools_dir: &Path) -> Result<()> {
    let store = (resolved.is_in_tree() && !trust_all_env()).then(TrustStore::open_default);
    stamp_marker_with(resolved, tools_dir, store)
}

/// `store` is `None` when the venv needs no token, and the marker then keeps
/// what it holds, so a sync under `TOOLR_TRUST_ALL` leaves a valid token in
/// place. A marker that already holds the store's token keeps it, so a resync
/// never makes toolr's own venv read as foreign. A store that fails leaves the
/// marker empty, so the venv reads as foreign rather than the sync failing.
fn stamp_marker_with(
    resolved: &ResolvedVenv,
    tools_dir: &Path,
    store: Option<Result<TrustStore>>,
) -> Result<()> {
    let root = project_root(tools_dir);
    let venv_dir = &resolved.venv_dir;
    let token = store.map(|s| {
        s.and_then(|s| match marker_token(&s, venv_dir) {
            Some(token) => {
                if s.venv_pending(venv_dir) {
                    s.settle_venv(root, venv_dir, Some(&token))?;
                }
                Ok(token)
            }
            None => s.settle_venv(root, venv_dir, None),
        })
    });
    let token = match token {
        None => read_marker(venv_dir).unwrap_or_default(),
        Some(Ok(token)) => token,
        Some(Err(e)) => {
            eprintln!(
                "toolr: warning: couldn't record the tools venv in the trust store, so toolr treats it as one it didn't build: {e:#}"
            );
            String::new()
        }
    };
    fs::create_dir_all(venv_dir)?;
    // Renamed over the old marker, so a concurrent check never sees the venv without one, and
    // a committed symlink is replaced, not written through.
    let mut tmp = tempfile::Builder::new()
        .prefix(".toolr-sync-stamp.")
        .suffix(".tmp")
        .tempfile_in(venv_dir)?;
    tmp.write_all(token.as_bytes())?;
    tmp.persist(venv_dir.join(FRESHNESS_MARKER))
        .map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;
    use tempfile::TempDir;

    fn touch_marker(venv_dir: &Path) -> Result<()> {
        fs::create_dir_all(venv_dir)?;
        fs::write(venv_dir.join(FRESHNESS_MARKER), b"")?;
        Ok(())
    }

    fn dummy_resolved(venv_dir: std::path::PathBuf) -> ResolvedVenv {
        ResolvedVenv {
            venv_dir: venv_dir.clone(),
            python: venv_dir.join("bin").join("python"),
            repo_key: "x".into(),
            python_version: "3.13".into(),
            config: Default::default(),
        }
    }

    /// Create `venv_dir` with the interpreter `dummy_resolved` points at.
    fn venv_with_interpreter(venv_dir: &Path) {
        fs::create_dir_all(venv_dir.join("bin")).unwrap();
        fs::write(venv_dir.join("bin").join("python"), b"").unwrap();
    }

    /// A fresh venv whose interpreter is a dangling symlink, the way a venv
    /// looks after `mise prune` removes the Python it was built from.
    #[cfg(unix)]
    fn fresh_venv_with_dead_interpreter(tmp: &Path) -> std::path::PathBuf {
        let venv = tmp.join("venv");
        fs::create_dir_all(venv.join("bin")).unwrap();
        std::os::unix::fs::symlink(
            tmp.join("pruned").join("python3.13"),
            venv.join("bin").join("python"),
        )
        .unwrap();
        fs::write(tmp.join("uv.lock"), b"locks").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        touch_marker(&venv).unwrap();
        venv
    }

    #[test]
    fn missing_marker_or_lock_reports_missing() {
        let tmp = TempDir::new().unwrap();
        let resolved = dummy_resolved(tmp.path().join("venv"));
        assert_eq!(check_freshness(&resolved, tmp.path()), Freshness::Missing);
    }

    #[test]
    fn marker_older_than_lock_reports_stale() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join("venv");
        venv_with_interpreter(&venv);
        touch_marker(&venv).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(tmp.path().join("uv.lock"), b"locks").unwrap();
        let resolved = dummy_resolved(venv);
        assert_eq!(check_freshness(&resolved, tmp.path()), Freshness::Stale);
    }

    #[test]
    fn marker_newer_than_lock_reports_fresh() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join("venv");
        venv_with_interpreter(&venv);
        std::fs::write(tmp.path().join("uv.lock"), b"locks").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        touch_marker(&venv).unwrap();
        let resolved = dummy_resolved(venv);
        assert_eq!(check_freshness(&resolved, tmp.path()), Freshness::Fresh);
    }

    /// Write `uv.lock` (and any `extra` inputs), then the marker, so the
    /// venv starts `Fresh`.
    fn synced_venv(tmp: &Path, extra: &[&str]) -> ResolvedVenv {
        let venv = tmp.join("venv");
        venv_with_interpreter(&venv);
        fs::write(tmp.join("uv.lock"), b"locks").unwrap();
        for name in extra {
            fs::write(tmp.join(name), b"x").unwrap();
        }
        std::thread::sleep(Duration::from_millis(20));
        touch_marker(&venv).unwrap();
        dummy_resolved(venv)
    }

    #[test]
    fn pyproject_newer_than_marker_reports_stale() {
        let tmp = TempDir::new().unwrap();
        let resolved = synced_venv(tmp.path(), &["pyproject.toml"]);
        std::thread::sleep(Duration::from_millis(20));
        fs::write(tmp.path().join("pyproject.toml"), b"edited").unwrap();
        assert_eq!(check_freshness(&resolved, tmp.path()), Freshness::Stale);
    }

    #[test]
    fn python_version_newer_than_marker_reports_stale() {
        let tmp = TempDir::new().unwrap();
        let resolved = synced_venv(tmp.path(), &["pyproject.toml", ".python-version"]);
        std::thread::sleep(Duration::from_millis(20));
        fs::write(tmp.path().join(".python-version"), b"3.14").unwrap();
        assert_eq!(check_freshness(&resolved, tmp.path()), Freshness::Stale);
    }

    #[test]
    fn uv_toml_newer_than_marker_reports_stale() {
        let tmp = TempDir::new().unwrap();
        let resolved = synced_venv(tmp.path(), &["pyproject.toml"]);
        std::thread::sleep(Duration::from_millis(20));
        fs::write(
            tmp.path().join("uv.toml"),
            b"exclude-newer = \"2024-01-01T00:00:00Z\"",
        )
        .unwrap();
        assert_eq!(check_freshness(&resolved, tmp.path()), Freshness::Stale);
    }

    #[test]
    fn absent_python_version_is_ignored() {
        let tmp = TempDir::new().unwrap();
        let resolved = synced_venv(tmp.path(), &["pyproject.toml"]);
        assert_eq!(check_freshness(&resolved, tmp.path()), Freshness::Fresh);
    }

    #[test]
    fn missing_lock_still_reports_missing_with_pyproject_present() {
        let tmp = TempDir::new().unwrap();
        let resolved = synced_venv(tmp.path(), &["pyproject.toml"]);
        fs::remove_file(tmp.path().join("uv.lock")).unwrap();
        assert_eq!(check_freshness(&resolved, tmp.path()), Freshness::Missing);
    }

    #[test]
    fn removed_base_python_reports_broken_even_with_a_venv_launcher() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join("venv");
        venv_with_interpreter(&venv);
        let home = tmp.path().join("pruned").join("bin");
        fs::write(
            venv.join("pyvenv.cfg"),
            format!("home = {}\n", home.display()),
        )
        .unwrap();
        fs::write(tmp.path().join("uv.lock"), b"locks").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        touch_marker(&venv).unwrap();
        let resolved = dummy_resolved(venv.clone());
        assert_eq!(check_freshness(&resolved, tmp.path()), Freshness::Broken);

        fs::create_dir_all(&home).unwrap();
        assert_eq!(check_freshness(&resolved, tmp.path()), Freshness::Fresh);
    }

    #[cfg(unix)]
    #[test]
    fn dead_interpreter_reports_broken_despite_fresh_marker() {
        let tmp = TempDir::new().unwrap();
        let resolved = dummy_resolved(fresh_venv_with_dead_interpreter(tmp.path()));
        assert_eq!(check_freshness(&resolved, tmp.path()), Freshness::Broken);
    }

    #[cfg(unix)]
    #[test]
    fn sync_if_needed_invokes_uv_when_interpreter_is_dead() {
        // The stub's non-zero exit proves uv ran instead of short-circuiting.
        let tmp = TempDir::new().unwrap();
        let resolved = dummy_resolved(fresh_venv_with_dead_interpreter(tmp.path()));
        let uv = stub_uv(tmp.path(), 17);
        let err = sync_if_needed(&uv, tmp.path(), &resolved, false, false, &UpgradeMode::None)
            .expect_err("a dead interpreter must trigger a sync");
        assert!(matches!(err, UvError::SyncFailed(Some(17))));
    }

    /// Build a uv-binary stub that, when invoked, returns `exit_code`.
    /// On non-Unix the test that uses it is skipped — the stub script
    /// relies on `#!/bin/sh` + 0o755 perms.
    #[cfg(unix)]
    fn stub_uv(tmp: &Path, exit_code: i32) -> UvBinary {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let stub = tmp.join("uv");
        let mut f = fs::File::create(&stub).unwrap();
        writeln!(f, "#!/bin/sh\nexit {exit_code}").unwrap();
        drop(f);
        let mut perms = fs::metadata(&stub).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&stub, perms).unwrap();
        UvBinary {
            path: stub,
            version: (0, 4, 0),
            source: crate::uv::UvSource::Path,
        }
    }

    #[cfg(unix)]
    #[test]
    fn sync_if_needed_skips_run_when_fresh_and_force_off() {
        // Pre-populate a fresh marker so check_freshness returns Fresh.
        // The stub `uv` deliberately exits non-zero — but it should
        // never be invoked.
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join("venv");
        venv_with_interpreter(&venv);
        fs::write(tmp.path().join("uv.lock"), b"locks").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        touch_marker(&venv).unwrap();

        let uv = stub_uv(tmp.path(), 99);
        let resolved = dummy_resolved(venv);
        sync_if_needed(&uv, tmp.path(), &resolved, false, false, &UpgradeMode::None)
            .expect("fresh should short-circuit");
    }

    #[cfg(unix)]
    #[test]
    fn sync_if_needed_invokes_uv_when_force_set_even_if_fresh() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join("venv");
        fs::create_dir_all(&venv).unwrap();
        fs::write(tmp.path().join("uv.lock"), b"locks").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        touch_marker(&venv).unwrap();

        // Stub exits 0 — success path. Marker should get re-stamped.
        let uv = stub_uv(tmp.path(), 0);
        let resolved = dummy_resolved(venv.clone());
        sync_if_needed(&uv, tmp.path(), &resolved, true, false, &UpgradeMode::None)
            .expect("force=true should run and succeed");
        assert!(venv.join(FRESHNESS_MARKER).exists());
    }

    #[cfg(unix)]
    #[test]
    fn sync_if_needed_propagates_nonzero_exit_as_sync_failed() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join("venv");
        // No prior marker → check_freshness returns Missing → uv runs.
        let uv = stub_uv(tmp.path(), 17);
        let resolved = dummy_resolved(venv);
        let err = sync_if_needed(&uv, tmp.path(), &resolved, false, false, &UpgradeMode::None)
            .expect_err("non-zero exit must surface as SyncFailed");
        assert!(matches!(err, UvError::SyncFailed(Some(17))));
    }

    #[cfg(unix)]
    #[test]
    fn sync_if_needed_translates_spawn_failure_to_uv_error() {
        // `UvBinary` pointed at a nonexistent path → run_uv_sync's
        // status() returns Err → sync_if_needed reports which uv failed to start.
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join("venv");
        let uv = UvBinary {
            path: tmp.path().join("definitely-not-uv"),
            version: (0, 4, 0),
            source: crate::uv::UvSource::Path,
        };
        let resolved = dummy_resolved(venv);
        let err = sync_if_needed(&uv, tmp.path(), &resolved, true, false, &UpgradeMode::None)
            .expect_err("spawn failure should surface");
        assert!(
            matches!(&err, UvError::Io(_)) && err.to_string().contains("definitely-not-uv"),
            "{err:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_uv_sync_passes_quiet_when_requested() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let argv_log = tmp.path().join("argv.log");
        let stub = tmp.path().join("uv");
        let mut f = fs::File::create(&stub).unwrap();
        writeln!(
            f,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\nexit 0",
            argv_log.display(),
        )
        .unwrap();
        drop(f);
        let mut perms = fs::metadata(&stub).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&stub, perms).unwrap();
        let uv = UvBinary {
            path: stub,
            version: (0, 4, 0),
            source: crate::uv::UvSource::Path,
        };
        let venv = tmp.path().join("venv");
        fs::create_dir_all(&venv).unwrap();
        let resolved = dummy_resolved(venv);

        run_uv_sync(
            &uv,
            tmp.path(),
            &resolved,
            &UpgradeMode::None,
            /*quiet=*/ true,
        )
        .expect("stub uv must exit 0");

        let captured = fs::read_to_string(&argv_log).unwrap();
        assert!(
            captured.lines().any(|l| l == "--quiet"),
            "expected `--quiet` in uv argv, got: {captured}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_uv_sync_omits_quiet_by_default() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let argv_log = tmp.path().join("argv.log");
        let stub = tmp.path().join("uv");
        let mut f = fs::File::create(&stub).unwrap();
        writeln!(
            f,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\nexit 0",
            argv_log.display(),
        )
        .unwrap();
        drop(f);
        let mut perms = fs::metadata(&stub).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&stub, perms).unwrap();
        let uv = UvBinary {
            path: stub,
            version: (0, 4, 0),
            source: crate::uv::UvSource::Path,
        };
        let venv = tmp.path().join("venv");
        fs::create_dir_all(&venv).unwrap();
        let resolved = dummy_resolved(venv);

        run_uv_sync(
            &uv,
            tmp.path(),
            &resolved,
            &UpgradeMode::None,
            /*quiet=*/ false,
        )
        .expect("stub uv must exit 0");

        let captured = fs::read_to_string(&argv_log).unwrap();
        assert!(
            !captured.lines().any(|l| l == "--quiet"),
            "did not expect `--quiet` in uv argv, got: {captured}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_uv_sync_passes_python_flag_when_config_pins_version() {
        // Stub uv prints its arguments to a file we can inspect.
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let argdump = tmp.path().join("argdump");
        let stub = tmp.path().join("uv");
        let mut f = fs::File::create(&stub).unwrap();
        writeln!(
            f,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}",
            argdump.display()
        )
        .unwrap();
        drop(f);
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
        let uv = UvBinary {
            path: stub,
            version: (0, 4, 0),
            source: crate::uv::UvSource::Path,
        };

        let venv = tmp.path().join("venv");
        let mut resolved = dummy_resolved(venv);
        resolved.config.python_version = Some("3.13".into());

        run_uv_sync(
            &uv,
            tmp.path(),
            &resolved,
            &UpgradeMode::None,
            /*quiet=*/ false,
        )
        .expect("stub uv should succeed");
        let dump = fs::read_to_string(&argdump).unwrap();
        assert!(dump.contains("sync"));
        assert!(dump.contains("--python"));
        assert!(dump.contains("3.13"));
        assert!(dump.contains("--project"));
    }

    #[cfg(unix)]
    #[test]
    fn run_uv_sync_passes_upgrade_flag_for_all_mode() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let argdump = tmp.path().join("argdump");
        let stub = tmp.path().join("uv");
        let mut f = fs::File::create(&stub).unwrap();
        writeln!(
            f,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}",
            argdump.display()
        )
        .unwrap();
        drop(f);
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
        let uv = UvBinary {
            path: stub,
            version: (0, 4, 0),
            source: crate::uv::UvSource::Path,
        };

        let venv = tmp.path().join("venv");
        fs::create_dir_all(&venv).unwrap();
        let resolved = dummy_resolved(venv);

        run_uv_sync(
            &uv,
            tmp.path(),
            &resolved,
            &UpgradeMode::All,
            /*quiet=*/ false,
        )
        .expect("stub uv should succeed");

        let dump = fs::read_to_string(&argdump).unwrap();
        assert!(dump.contains("sync"));
        assert!(dump.lines().any(|l| l == "--upgrade"));
    }

    #[cfg(unix)]
    #[test]
    fn run_uv_sync_passes_upgrade_package_flag_for_packages_mode() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let argdump = tmp.path().join("argdump");
        let stub = tmp.path().join("uv");
        let mut f = fs::File::create(&stub).unwrap();
        writeln!(
            f,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}",
            argdump.display()
        )
        .unwrap();
        drop(f);
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
        let uv = UvBinary {
            path: stub,
            version: (0, 4, 0),
            source: crate::uv::UvSource::Path,
        };

        let venv = tmp.path().join("venv");
        fs::create_dir_all(&venv).unwrap();
        let resolved = dummy_resolved(venv);

        let mode = UpgradeMode::Packages(vec!["foo".into(), "bar".into()]);
        run_uv_sync(&uv, tmp.path(), &resolved, &mode, /*quiet=*/ false)
            .expect("stub uv should succeed");

        let dump = fs::read_to_string(&argdump).unwrap();
        let mut iter = dump.lines();
        let mut saw_foo = false;
        let mut saw_bar = false;
        while let Some(line) = iter.next() {
            if line == "--upgrade-package" {
                match iter.next() {
                    Some("foo") => saw_foo = true,
                    Some("bar") => saw_bar = true,
                    _ => {}
                }
            }
        }
        assert!(saw_foo && saw_bar, "expected both packages in argv: {dump}");
    }

    #[cfg(unix)]
    #[test]
    fn run_uv_lock_with_none_mode_calls_uv_lock_with_no_upgrade_args() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let argdump = tmp.path().join("argdump");
        let stub = tmp.path().join("uv");
        let mut f = fs::File::create(&stub).unwrap();
        writeln!(
            f,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}",
            argdump.display()
        )
        .unwrap();
        drop(f);
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
        let uv = UvBinary {
            path: stub,
            version: (0, 4, 0),
            source: crate::uv::UvSource::Path,
        };
        let venv = tmp.path().join("venv");
        let resolved = dummy_resolved(venv);

        run_uv_lock(
            &uv,
            tmp.path(),
            &resolved,
            &UpgradeMode::None,
            /*quiet=*/ false,
        )
        .expect("stub uv should succeed");

        let dump = fs::read_to_string(&argdump).unwrap();
        assert!(dump.contains("lock"), "args: {dump}");
        assert!(dump.contains("--project"), "args: {dump}");
        assert!(
            !dump.contains("--upgrade"),
            "args should not contain --upgrade: {dump}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_uv_lock_with_all_mode_passes_upgrade() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let argdump = tmp.path().join("argdump");
        let stub = tmp.path().join("uv");
        let mut f = fs::File::create(&stub).unwrap();
        writeln!(
            f,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}",
            argdump.display()
        )
        .unwrap();
        drop(f);
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
        let uv = UvBinary {
            path: stub,
            version: (0, 4, 0),
            source: crate::uv::UvSource::Path,
        };
        let venv = tmp.path().join("venv");
        let resolved = dummy_resolved(venv);

        run_uv_lock(
            &uv,
            tmp.path(),
            &resolved,
            &UpgradeMode::All,
            /*quiet=*/ false,
        )
        .expect("stub uv should succeed");

        let dump = fs::read_to_string(&argdump).unwrap();
        assert!(dump.lines().any(|l| l == "--upgrade"), "args: {dump}");
    }

    #[cfg(unix)]
    #[test]
    fn run_uv_lock_with_packages_mode_passes_each_upgrade_package() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let argdump = tmp.path().join("argdump");
        let stub = tmp.path().join("uv");
        let mut f = fs::File::create(&stub).unwrap();
        writeln!(
            f,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}",
            argdump.display()
        )
        .unwrap();
        drop(f);
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
        let uv = UvBinary {
            path: stub,
            version: (0, 4, 0),
            source: crate::uv::UvSource::Path,
        };
        let venv = tmp.path().join("venv");
        let resolved = dummy_resolved(venv);

        let mode = UpgradeMode::Packages(vec!["foo".into(), "bar".into()]);
        run_uv_lock(&uv, tmp.path(), &resolved, &mode, /*quiet=*/ false)
            .expect("stub uv should succeed");

        let dump = fs::read_to_string(&argdump).unwrap();
        let occurrences = dump.lines().filter(|l| *l == "--upgrade-package").count();
        assert_eq!(
            occurrences, 2,
            "expected 2 --upgrade-package tokens: {dump}"
        );
        assert!(dump.contains("foo"));
        assert!(dump.contains("bar"));
    }

    #[test]
    fn stamp_marker_creates_venv_dir_if_missing() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join("nested").join("venv");
        stamp_marker_with(&dummy_resolved(venv.clone()), tmp.path(), None).unwrap();
        assert_eq!(fs::read(venv.join(FRESHNESS_MARKER)).unwrap(), b"");
    }

    /// Set `path`'s mtime, so tests order sync inputs without sleeping.
    fn set_mtime(path: &Path, when: SystemTime) {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    /// A trusted repository with an in-tree venv and its own trust store.
    struct InTree {
        _tmp: TempDir,
        tools: PathBuf,
        store: TrustStore,
        resolved: ResolvedVenv,
    }

    impl InTree {
        fn new() -> Self {
            let tmp = TempDir::new().unwrap();
            let root = fs::canonicalize(tmp.path()).unwrap().join("repo");
            let tools = root.join("tools");
            let venv = tools.join(".venv");
            venv_with_interpreter(&venv);
            fs::write(tools.join("uv.lock"), b"locks").unwrap();
            set_mtime(
                &tools.join("uv.lock"),
                SystemTime::now() - Duration::from_secs(60),
            );
            let store = TrustStore::at(tmp.path().join("state").join("trust"));
            store.trust(&root).unwrap();
            let resolved = ResolvedVenv {
                repo_key: String::new(),
                ..dummy_resolved(venv)
            };
            Self {
                _tmp: tmp,
                tools,
                store,
                resolved,
            }
        }

        fn check(&self) -> Freshness {
            check_freshness_with(&self.resolved, &self.tools, TokenCheck::Store(&self.store))
        }

        fn stamp(&self) {
            stamp_marker_with(&self.resolved, &self.tools, Some(Ok(self.store.clone()))).unwrap();
        }

        fn marker(&self) -> PathBuf {
            self.resolved.venv_dir.join(FRESHNESS_MARKER)
        }
    }

    #[test]
    fn in_tree_venv_without_a_token_is_foreign() {
        let t = InTree::new();
        assert_eq!(t.check(), Freshness::Foreign, "no marker");
        touch_marker(&t.resolved.venv_dir).unwrap();
        assert_eq!(t.check(), Freshness::Foreign, "empty marker");
        fs::write(t.marker(), "0".repeat(64)).unwrap();
        assert_eq!(t.check(), Freshness::Foreign, "forged marker");
        let no_store = check_freshness_with(&t.resolved, &t.tools, TokenCheck::NoStore);
        assert_eq!(no_store, Freshness::Foreign, "no store");
    }

    #[test]
    fn in_tree_without_a_venv_is_missing() {
        let t = InTree::new();
        fs::remove_dir_all(&t.resolved.venv_dir).unwrap();
        assert_eq!(t.check(), Freshness::Missing);
    }

    #[test]
    fn matching_token_is_fresh() {
        let t = InTree::new();
        t.stamp();
        assert_eq!(t.check(), Freshness::Fresh);
        assert_eq!(fs::read_to_string(t.marker()).unwrap().len(), 64);
        let root = t.tools.parent().unwrap();
        t.store.untrust(root).unwrap();
        assert_eq!(t.check(), Freshness::Foreign, "untrusted again");
    }

    #[test]
    fn a_token_from_an_unusable_store_is_never_written() {
        let t = InTree::new();
        let not_a_dir = t.tools.join("store-file");
        fs::write(&not_a_dir, b"").unwrap();
        stamp_marker_with(&t.resolved, &t.tools, Some(Ok(TrustStore::at(not_a_dir)))).unwrap();
        assert_eq!(fs::read(t.marker()).unwrap(), b"");
        stamp_marker_with(
            &t.resolved,
            &t.tools,
            Some(Err(anyhow::anyhow!("no state dir"))),
        )
        .unwrap();
        assert_eq!(fs::read(t.marker()).unwrap(), b"");
        assert_eq!(t.check(), Freshness::Foreign);
    }

    #[test]
    fn cache_located_venv_needs_no_token() {
        let tmp = TempDir::new().unwrap();
        let resolved = synced_venv(tmp.path(), &[]);
        assert_eq!(
            check_freshness_with(&resolved, tmp.path(), TokenCheck::Skip),
            Freshness::Fresh
        );
    }

    #[test]
    fn trust_all_skips_the_token_check() {
        let t = InTree::new();
        touch_marker(&t.resolved.venv_dir).unwrap();
        assert_eq!(
            check_freshness_with(&t.resolved, &t.tools, TokenCheck::Skip),
            Freshness::Fresh
        );
    }

    #[test]
    fn removing_a_foreign_venv_directory_removes_it() {
        let t = InTree::new();
        remove_foreign_venv(&t.tools, &t.resolved.venv_dir).unwrap();
        assert!(!t.resolved.venv_dir.exists());
        assert!(remove_foreign_venv(&t.tools, &t.resolved.venv_dir).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_tools_dir_is_never_emptied() {
        let tmp = TempDir::new().unwrap();
        let other_tools = tmp.path().join("other").join("tools");
        venv_with_interpreter(&other_tools.join(".venv"));
        let tools = tmp.path().join("repo").join("tools");
        fs::create_dir_all(tools.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&other_tools, &tools).unwrap();
        let err = remove_foreign_venv(&tools, &tools.join(".venv")).unwrap_err();
        assert!(err.to_string().contains("toolr project venv sync"), "{err}");
        assert!(other_tools.join(".venv/bin/python").is_file());
    }

    #[test]
    fn a_venv_outside_tools_is_never_removed() {
        let t = InTree::new();
        let elsewhere = t._tmp.path().join("elsewhere");
        venv_with_interpreter(&elsewhere);
        assert!(remove_foreign_venv(&t.tools, &elsewhere).is_err());
        assert!(elsewhere.join("bin/python").is_file());
    }

    #[test]
    fn a_matching_marker_keeps_its_token_across_a_resync() {
        let t = InTree::new();
        t.stamp();
        let token = fs::read_to_string(t.marker()).unwrap();
        t.stamp();
        assert_eq!(fs::read_to_string(t.marker()).unwrap(), token);
        assert_eq!(
            t.store.venv_token(&t.resolved.venv_dir).unwrap(),
            Some(token)
        );
        assert_eq!(t.check(), Freshness::Fresh);
    }

    #[test]
    fn sync_locked_fails_without_touching_a_foreign_venv_when_the_store_cant_lock() {
        let t = InTree::new();
        fs::write(t.store.dir().join("locks"), b"not a directory").unwrap();
        let err = sync_locked(
            &t.resolved,
            &t.tools,
            false,
            SyncSteps {
                check: || t.check(),
                lock: || t.store.lock_venv(&t.resolved.venv_dir, false),
                approve,
                sync: sync_fails,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains(&shown(t.store.dir())), "{err}");
        assert!(t.resolved.python.is_file(), "the venv must stay");
    }

    /// toolr's own stale venv keeps its token without writing to the store, so
    /// a lock that fails doesn't stop its resync.
    #[test]
    fn sync_locked_resyncs_its_own_venv_when_the_store_cant_lock() {
        let t = InTree::new();
        t.stamp();
        let token = fs::read_to_string(t.marker()).unwrap();
        set_mtime(&t.marker(), SystemTime::now() - Duration::from_secs(3600));
        assert_eq!(t.check(), Freshness::Stale);
        let synced = std::cell::Cell::new(false);
        sync_locked(
            &t.resolved,
            &t.tools,
            false,
            SyncSteps {
                check: || t.check(),
                lock: || -> Result<()> { Err(anyhow::anyhow!("read-only store")) },
                approve,
                sync: || {
                    synced.set(true);
                    t.stamp();
                    Ok(())
                },
            },
        )
        .unwrap();
        assert!(synced.get());
        assert_eq!(fs::read_to_string(t.marker()).unwrap(), token);
        assert_eq!(t.check(), Freshness::Fresh);
    }

    #[test]
    fn a_stamp_that_needs_no_store_keeps_the_token_and_refreshes_the_marker() {
        let t = InTree::new();
        t.stamp();
        let token = fs::read_to_string(t.marker()).unwrap();
        let old = SystemTime::now() - Duration::from_secs(3600);
        set_mtime(&t.marker(), old);
        stamp_marker_with(&t.resolved, &t.tools, None).unwrap();
        assert_eq!(fs::read_to_string(t.marker()).unwrap(), token);
        assert!(fs::metadata(t.marker()).unwrap().modified().unwrap() > old);
        assert_eq!(t.check(), Freshness::Fresh);
    }

    #[test]
    fn only_a_marker_of_token_size_is_read() {
        let t = InTree::new();
        t.stamp();
        let token = fs::read_to_string(t.marker()).unwrap();
        fs::write(t.marker(), format!("{token}\n")).unwrap();
        assert_eq!(t.check(), Freshness::Fresh, "a trailing newline");
        fs::write(t.marker(), format!("{token}\n\n")).unwrap();
        assert_eq!(t.check(), Freshness::Foreign, "one byte over");
        fs::write(t.marker(), token.repeat(1000)).unwrap();
        assert_eq!(t.check(), Freshness::Foreign, "far over");
    }

    #[cfg(unix)]
    #[test]
    fn a_marker_that_is_a_symlink_or_a_fifo_is_never_read() {
        let t = InTree::new();
        t.stamp();
        let elsewhere = t._tmp.path().join("token");
        fs::rename(t.marker(), &elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, t.marker()).unwrap();
        assert_eq!(t.check(), Freshness::Foreign, "a link to the right token");
        fs::remove_file(t.marker()).unwrap();
        let path =
            std::ffi::CString::new(t.marker().into_os_string().into_encoded_bytes()).unwrap();
        // SAFETY: `path` is a valid NUL-terminated string.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        assert_eq!(
            t.check(),
            Freshness::Foreign,
            "a FIFO, read without hanging"
        );
    }

    /// Another toolr finishes building the venv while this one waits for the
    /// lock: the re-check sees it fresh, so nothing is removed or synced.
    #[test]
    fn sync_locked_rechecks_after_waiting_for_the_lock() {
        let t = InTree::new();
        sync_locked(
            &t.resolved,
            &t.tools,
            false,
            SyncSteps {
                check: || t.check(),
                lock: || {
                    t.stamp();
                    Ok(())
                },
                approve,
                sync: sync_fails,
            },
        )
        .unwrap();
        assert!(t.resolved.python.is_file());
    }

    #[test]
    fn sync_locked_removes_a_foreign_venv_only_under_the_lock() {
        let t = InTree::new();
        let events = std::cell::RefCell::new(Vec::new());
        sync_locked(
            &t.resolved,
            &t.tools,
            false,
            SyncSteps {
                check: || {
                    events.borrow_mut().push("check");
                    t.check()
                },
                lock: || {
                    events.borrow_mut().push("lock");
                    Ok(())
                },
                approve,
                sync: || {
                    assert!(!t.resolved.venv_dir.exists(), "removed before the sync");
                    events.borrow_mut().push("sync");
                    Ok(())
                },
            },
        )
        .unwrap();
        assert_eq!(*events.borrow(), ["check", "lock", "check", "sync"]);
    }

    #[test]
    fn an_untrusted_venv_stays_unless_the_user_or_the_env_approves() {
        let t = InTree::new();
        let refusal = |e: UvError| {
            let msg = e.to_string();
            assert!(
                msg.contains("toolr didn't build")
                    && msg.contains("Nothing was removed")
                    && msg.contains("toolr project venv sync")
                    && msg.contains("TOOLR_REPLACE_UNTRUSTED_VENV=1"),
                "{msg}"
            );
        };
        for answer in ["n\n", "\n", ""] {
            let (mut input, mut out) = (std::io::Cursor::new(answer), Vec::new());
            refusal(
                approve_removal(&t.resolved.venv_dir, false, Some((&mut input, &mut out)))
                    .unwrap_err(),
            );
            let asked = String::from_utf8(out).unwrap();
            assert!(
                asked.contains("so toolr won't run it")
                    && asked.contains("Remove it and build a fresh one? [y/N]"),
                "{asked}"
            );
        }
        refusal(approve_removal(&t.resolved.venv_dir, false, None).unwrap_err());
        // `quiet` never asks, so no terminal is needed to show it refuses.
        refusal(approve_removal_on_tty(&t.resolved.venv_dir, true).unwrap_err());
        approve_removal(&t.resolved.venv_dir, true, None).unwrap();
        let (mut input, mut out) = (std::io::Cursor::new("y\n"), Vec::new());
        approve_removal(&t.resolved.venv_dir, false, Some((&mut input, &mut out))).unwrap();

        let refused = || approve_removal(&t.resolved.venv_dir, false, None);
        sync_locked(
            &t.resolved,
            &t.tools,
            false,
            SyncSteps {
                check: || t.check(),
                lock: lock_must_not_run,
                approve: refused,
                sync: sync_fails,
            },
        )
        .unwrap_err();
        assert!(t.resolved.python.is_file(), "a refused venv must stay");
    }

    /// The approval is asked before the lock is taken, so no other toolr waits on a human.
    #[test]
    fn sync_locked_asks_before_it_locks() {
        let t = InTree::new();
        let events = std::cell::RefCell::new(Vec::new());
        sync_locked(
            &t.resolved,
            &t.tools,
            false,
            SyncSteps {
                check: || {
                    events.borrow_mut().push("check");
                    t.check()
                },
                lock: || {
                    events.borrow_mut().push("lock");
                    Ok(())
                },
                approve: || {
                    events.borrow_mut().push("approve");
                    Ok(())
                },
                sync: || {
                    events.borrow_mut().push("sync");
                    Ok(())
                },
            },
        )
        .unwrap();
        assert_eq!(
            *events.borrow(),
            ["check", "approve", "lock", "check", "sync"]
        );
    }

    /// Another toolr built the venv while the user answered: the approval isn't used.
    #[test]
    fn sync_locked_respects_a_state_that_changed_after_approval() {
        let t = InTree::new();
        sync_locked(
            &t.resolved,
            &t.tools,
            false,
            SyncSteps {
                check: || t.check(),
                lock: || {
                    t.stamp();
                    Ok(())
                },
                approve,
                sync: sync_fails,
            },
        )
        .unwrap();
        assert!(t.resolved.python.is_file(), "the built venv must stay");
    }

    /// A different venv toolr didn't build replaced the approved one: ask again.
    #[test]
    fn sync_locked_asks_again_for_a_different_venv() {
        let t = InTree::new();
        let asked = std::cell::Cell::new(0);
        let count = || {
            asked.set(asked.get() + 1);
            Ok(())
        };
        sync_locked(
            &t.resolved,
            &t.tools,
            false,
            SyncSteps {
                check: || t.check(),
                lock: || Ok(()),
                approve: count,
                sync: || Ok(()),
            },
        )
        .unwrap();
        assert_eq!(asked.get(), 1, "the same venv is asked about once");

        let t = InTree::new();
        asked.set(0);
        let swap = || {
            let other = t.resolved.venv_dir.with_extension("other");
            venv_with_interpreter(&other);
            fs::remove_dir_all(&t.resolved.venv_dir).unwrap();
            fs::rename(&other, &t.resolved.venv_dir).unwrap();
            Ok(())
        };
        let refuse_second = || {
            asked.set(asked.get() + 1);
            if asked.get() == 1 {
                Ok(())
            } else {
                approve_removal(&t.resolved.venv_dir, false, None)
            }
        };
        sync_locked(
            &t.resolved,
            &t.tools,
            false,
            SyncSteps {
                check: || t.check(),
                lock: swap,
                approve: refuse_second,
                sync: sync_fails,
            },
        )
        .unwrap_err();
        assert_eq!(asked.get(), 2);
        assert!(t.resolved.python.is_file(), "the unapproved venv must stay");
    }

    /// A first build that failed after uv created the venv leaves toolr's own venv, not one to
    /// ask about; a marker that doesn't match still isn't toolr's.
    #[test]
    fn a_claimed_venv_without_a_marker_is_toolrs_until_stamped() {
        let t = InTree::new();
        let root = t.tools.parent().unwrap();
        assert_eq!(t.check(), Freshness::Foreign);
        t.store.claim_venv(root, &t.resolved.venv_dir).unwrap();
        assert!(t.store.venv_pending(&t.resolved.venv_dir));
        assert_eq!(t.check(), Freshness::Missing, "toolr's half-built venv");
        touch_marker(&t.resolved.venv_dir).unwrap();
        assert_eq!(t.check(), Freshness::Foreign, "a marker that doesn't match");
        fs::remove_file(t.marker()).unwrap();

        t.stamp();
        assert!(
            !t.store.venv_pending(&t.resolved.venv_dir),
            "the stamp settles it"
        );
        assert_eq!(t.check(), Freshness::Fresh);
        let token = fs::read_to_string(t.marker()).unwrap();
        // A rebuild keeps the token, and its stamp settles the claim again.
        t.store.claim_venv(root, &t.resolved.venv_dir).unwrap();
        assert_eq!(t.check(), Freshness::Fresh);
        t.stamp();
        assert_eq!(fs::read_to_string(t.marker()).unwrap(), token);
        assert!(!t.store.venv_pending(&t.resolved.venv_dir));
    }

    /// A lock that must not be taken: a fresh venv, or a removal refused before locking.
    fn lock_must_not_run() -> Result<()> {
        unreachable!("no lock is taken here")
    }

    /// The user's `y` to removing a foreign venv.
    fn approve() -> Result<(), UvError> {
        Ok(())
    }

    /// A sync that must not run: passing it to `sync_locked().unwrap()` fails the
    /// test if it does.
    fn sync_fails() -> Result<(), UvError> {
        Err(UvError::Io(std::io::Error::other("sync must not run")))
    }

    #[test]
    fn sync_locked_returns_the_sync_error() {
        let t = InTree::new();
        let err = sync_locked(
            &t.resolved,
            &t.tools,
            false,
            SyncSteps {
                check: || t.check(),
                lock: || Ok(()),
                approve,
                sync: sync_fails,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("sync must not run"), "{err}");
    }

    #[test]
    fn sync_locked_skips_the_lock_when_fresh() {
        let t = InTree::new();
        t.stamp();
        sync_locked(
            &t.resolved,
            &t.tools,
            false,
            SyncSteps {
                check: || t.check(),
                lock: lock_must_not_run,
                approve,
                sync: sync_fails,
            },
        )
        .unwrap();
    }

    #[test]
    fn an_approval_needs_a_known_identity_that_still_matches() {
        let id = Some((1, 2));
        assert!(still_approved(id, id));
        assert!(!still_approved(id, Some((1, 3))));
        assert!(!still_approved(id, None));
        // No identity, as on a platform without one: nothing carries over the lock.
        assert!(!still_approved(None, None));
        let tmp = TempDir::new().unwrap();
        assert_eq!(venv_identity(&tmp.path().join("missing")), None);
    }

    #[test]
    fn is_foreign_matches_the_freshness_check() {
        let t = InTree::new();
        assert!(is_foreign_with(
            &t.resolved,
            project_root(&t.tools),
            TokenCheck::Store(&t.store)
        ));
        assert!(!is_foreign_with(
            &t.resolved,
            project_root(&t.tools),
            TokenCheck::Skip
        ));
        t.stamp();
        assert!(!is_foreign_with(
            &t.resolved,
            project_root(&t.tools),
            TokenCheck::Store(&t.store)
        ));
    }

    /// Through the real env: the default store under `XDG_STATE_HOME`.
    #[cfg(unix)]
    #[test]
    fn sync_if_needed_quiet_never_removes_a_foreign_venv() {
        let t = InTree::new();
        let mut env = crate::test_env::EnvGuard::write();
        env.set("XDG_STATE_HOME", t._tmp.path().join("default-state"));
        env.remove("TOOLR_TRUST_ALL");
        env.set("TOOLR_AUTO_SYNC", "1");
        let uv = stub_uv(t._tmp.path(), 0);
        let err = sync_if_needed(&uv, &t.tools, &t.resolved, false, true, &UpgradeMode::None)
            .expect_err("a venv that stays must not be synced");
        assert!(err.to_string().contains("Nothing was removed"), "{err:?}");
        assert!(t.resolved.python.is_file());
    }

    #[test]
    fn only_the_replace_key_approves_without_a_terminal() {
        let t = InTree::new();
        let mut env = crate::test_env::EnvGuard::write();
        env.set("TOOLR_AUTO_SYNC", "1");
        env.set("TOOLR_AUTO_INSTALL_UV", "1");
        for off in [None, Some("0"), Some("")] {
            match off {
                Some(v) => env.set(REPLACE_VENV_ENV, v),
                None => env.remove(REPLACE_VENV_ENV),
            }
            assert!(
                approve_removal_on_tty(&t.resolved.venv_dir, true).is_err(),
                "{off:?}"
            );
        }
        env.set(REPLACE_VENV_ENV, "1");
        approve_removal_on_tty(&t.resolved.venv_dir, true).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn sync_locked_fails_when_an_approved_foreign_venv_cant_be_removed() {
        use std::os::unix::fs::PermissionsExt;
        let t = InTree::new();
        fs::set_permissions(&t.tools, fs::Permissions::from_mode(0o555)).unwrap();
        let result = sync_locked(
            &t.resolved,
            &t.tools,
            false,
            SyncSteps {
                check: || t.check(),
                lock: || Ok(()),
                approve,
                sync: sync_fails,
            },
        );
        fs::set_permissions(&t.tools, fs::Permissions::from_mode(0o755)).unwrap();
        let err = result.expect_err("a venv that stays must not be synced");
        assert!(
            matches!(&err, UvError::UntrustedVenv(_))
                && err.to_string().contains("couldn't remove"),
            "{err:?}"
        );
        assert!(t.resolved.venv_dir.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn removing_a_symlinked_venv_keeps_its_target() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("elsewhere");
        venv_with_interpreter(&target);
        let link = tmp.path().join(".venv");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        remove_foreign_venv(tmp.path(), &link).unwrap();
        assert!(fs::symlink_metadata(&link).is_err());
        assert!(target.join("bin").join("python").is_file());
    }

    #[test]
    fn upgrade_mode_default_is_none() {
        assert!(matches!(UpgradeMode::default(), UpgradeMode::None));
    }

    #[test]
    fn upgrade_mode_packages_preserves_order() {
        let m = UpgradeMode::Packages(vec!["foo".into(), "bar".into()]);
        if let UpgradeMode::Packages(p) = m {
            assert_eq!(p, vec!["foo".to_string(), "bar".to_string()]);
        } else {
            panic!("expected Packages variant");
        }
    }
}
