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
    match token_store(resolved) {
        None => f(TokenCheck::Skip),
        Some(Ok(store)) => f(TokenCheck::Store(&store)),
        Some(Err(_)) => f(TokenCheck::NoStore),
    }
}

/// The one rule for which venvs carry a token: an in-tree venv, unless `TOOLR_TRUST_ALL` is
/// set. `None` when it needs none, else the default store, which may fail to open. The check,
/// the lock, the claim and the stamp all go through this, so they can't disagree.
fn token_store(resolved: &ResolvedVenv) -> Option<Result<TrustStore>> {
    (resolved.is_in_tree() && !trust_all_env()).then(TrustStore::open_default)
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
    // Only the marker vouches: a pending claim may be a killed sync's, and no check made
    // without the lock can tell (see [`wait_for_a_claimed_build`]).
    marker_token(store, &resolved.venv_dir).is_some()
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

/// Whether `venv_dir` sits directly in `tools_dir` and `tools_dir` is a real directory: a
/// `tools` that is a symlink could be another project's, so nothing under it is removed.
fn in_real_tools_dir(tools_dir: &Path, venv_dir: &Path) -> bool {
    fs::symlink_metadata(tools_dir).is_ok_and(|m| m.is_dir())
        && venv_dir.parent() == Some(tools_dir)
}

/// Never follows a symlink: a linked `tools/.venv` loses only the link. A
/// `tools` that is itself a symlink could be another project's, so that
/// layout is refused rather than emptied.
fn remove_foreign_venv(tools_dir: &Path, venv_dir: &Path) -> std::io::Result<()> {
    if !in_real_tools_dir(tools_dir, venv_dir) {
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
    wait_for_a_claimed_build(resolved, quiet);
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
                let absent = claim_before_build(resolved, tools_dir);
                // Not `Http`: its message tells the user to install uv, which they already have.
                let ran = run_uv_sync(uv, tools_dir, resolved, upgrade, quiet);
                if !matches!(&ran, Ok(status) if status.success()) {
                    abandon_build(resolved, tools_dir, absent);
                }
                let status = ran.map_err(|e| {
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

/// Before the first check: a pending claim says a toolr may be building the venv, which reads
/// as foreign until it is stamped. Taking the lock and dropping it waits for that build, or ends
/// a claim a killed sync left, so the check that follows asks only about what is really there,
/// and still asks before the lock. A lock that fails here fails again in [`sync_locked`].
fn wait_for_a_claimed_build(resolved: &ResolvedVenv, quiet: bool) {
    if let Some(Ok(store)) = token_store(resolved)
        && store.venv_pending(&resolved.venv_dir)
    {
        let _ = store.lock_venv(&resolved.venv_dir, quiet);
    }
}

/// The sync lock of an in-tree venv whose token toolr checks; `None` for any other venv.
pub(super) fn lock_in_tree(
    resolved: &ResolvedVenv,
    quiet: bool,
) -> Result<Option<crate::trust::VenvLock>> {
    token_store(resolved)
        .transpose()?
        .map(|store| store.lock_venv(&resolved.venv_dir, quiet))
        .transpose()
}

/// Under the lock, just before uv runs: record that toolr is building this in-tree venv (see
/// [`TrustStore::claim_venv`]), so another toolr's sync that sees it unstamped waits on the
/// lock before asking about it (see [`wait_for_a_claimed_build`]).
/// Returns whether there was no venv, the one case [`abandon_build`] may remove what uv left.
fn claim_before_build(resolved: &ResolvedVenv, tools_dir: &Path) -> bool {
    let absent = fs::symlink_metadata(&resolved.venv_dir)
        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound);
    // Store or claim failures are ignored: another toolr then asks about the venv rather than
    // waiting for it (fail-safe).
    if let Some(Ok(store)) = token_store(resolved) {
        let _ = store.claim_venv(project_root(tools_dir), &resolved.venv_dir);
    }
    absent
}

/// Still under the lock, after a sync that failed: remove the venv uv began when toolr found
/// none before claiming, then end the claim. Nothing the failed sync left counts as toolr's
/// later: a directory that stays, say because it couldn't be removed, is asked about. Only a
/// real directory is removed; `remove_dir_all` doesn't follow a symlink.
fn abandon_build(resolved: &ResolvedVenv, tools_dir: &Path, absent: bool) {
    let Some(Ok(store)) = token_store(resolved) else {
        return;
    };
    let venv_dir = &resolved.venv_dir;
    let left_dir = fs::symlink_metadata(venv_dir).is_ok_and(|m| m.is_dir());
    if absent && left_dir && in_real_tools_dir(tools_dir, venv_dir) {
        let _ = fs::remove_dir_all(venv_dir);
    }
    // Unended, the claim still covers nothing once the lock is released, and the next lock
    // holder ends it.
    let _ = store.end_claim(venv_dir);
}

/// Write the marker after toolr built or changed the venv: for an in-tree
/// venv, with a fresh token recorded in the default trust store.
pub(super) fn stamp_marker(resolved: &ResolvedVenv, tools_dir: &Path) -> Result<()> {
    stamp_marker_with(resolved, tools_dir, token_store(resolved))
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
mod tests;
