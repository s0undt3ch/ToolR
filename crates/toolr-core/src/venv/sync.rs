//! Drive `uv sync` to materialise the tools venv.

use std::fs;
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
    let (store, trust_all) = default_store_for(resolved);
    check_freshness_with(resolved, tools_dir, store.as_ref(), trust_all)
}

/// Only [`Freshness::Foreign`]'s test, for callers that read the venv's files
/// without running it, such as the plugin-fragment glob.
pub fn is_foreign(resolved: &ResolvedVenv, tools_dir: &Path) -> bool {
    let (store, trust_all) = default_store_for(resolved);
    is_foreign_with(resolved, tools_dir, store.as_ref(), trust_all)
}

/// The store a token check needs, and whether `TOOLR_TRUST_ALL` skips it.
/// Cache venvs need neither, so they read no env and no store.
fn default_store_for(resolved: &ResolvedVenv) -> (Option<TrustStore>, bool) {
    if !resolved.is_in_tree() {
        return (None, false);
    }
    let trust_all = trust_all_env();
    let store = (!trust_all)
        .then(TrustStore::open_default)
        .and_then(Result::ok);
    (store, trust_all)
}

fn is_foreign_with(
    resolved: &ResolvedVenv,
    tools_dir: &Path,
    store: Option<&TrustStore>,
    trust_all: bool,
) -> bool {
    resolved.is_in_tree()
        && !trust_all
        && fs::symlink_metadata(&resolved.venv_dir).is_ok()
        && !built_by_toolr(resolved, tools_dir, store)
}

pub(crate) fn check_freshness_with(
    resolved: &ResolvedVenv,
    tools_dir: &Path,
    store: Option<&TrustStore>,
    trust_all: bool,
) -> Freshness {
    if is_foreign_with(resolved, tools_dir, store, trust_all) {
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
fn built_by_toolr(resolved: &ResolvedVenv, tools_dir: &Path, store: Option<&TrustStore>) -> bool {
    let (Some(store), Some(root)) = (store, tools_dir.parent()) else {
        return false;
    };
    if !store.is_trusted(root).unwrap_or(false) {
        return false;
    }
    marker_token(store, &resolved.venv_dir).is_some()
}

/// The store's token for `venv_dir`, when the venv's marker holds it.
fn marker_token(store: &TrustStore, venv_dir: &Path) -> Option<String> {
    let marker = fs::read_to_string(venv_dir.join(FRESHNESS_MARKER)).ok()?;
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
    let lock = || {
        let (store, _) = default_store_for(resolved);
        store.map(|s| s.lock_venv(&resolved.venv_dir))
    };
    sync_locked(
        resolved,
        tools_dir,
        bypass_stamp,
        || check_freshness(resolved, tools_dir),
        lock,
        || {
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
    )
}

/// Check, lock, re-check, then remove a foreign venv and sync, all under the
/// lock: another toolr may be building this venv, and until it stamps the
/// marker the venv reads as foreign.
fn sync_locked<L>(
    resolved: &ResolvedVenv,
    tools_dir: &Path,
    bypass_stamp: bool,
    check: impl Fn() -> Freshness,
    lock: impl FnOnce() -> L,
    sync: impl FnOnce() -> Result<(), UvError>,
) -> Result<(), UvError> {
    if !bypass_stamp && check() == Freshness::Fresh {
        return Ok(());
    }
    let _lock = lock();
    match check() {
        // uv runs an existing venv's interpreter to inspect it, so a planted one goes first.
        Freshness::Foreign => remove_foreign_venv(tools_dir, &resolved.venv_dir).map_err(|e| {
            let msg = format!("couldn't remove {}: {e}", shown(&resolved.venv_dir));
            UvError::Io(std::io::Error::other(msg))
        })?,
        Freshness::Fresh if !bypass_stamp => return Ok(()),
        _ => {}
    }
    sync()
}

/// Write the marker after toolr built or changed the venv: for an in-tree
/// venv, with a fresh token recorded in the default trust store.
pub(super) fn stamp_marker(resolved: &ResolvedVenv, tools_dir: &Path) -> Result<()> {
    let store = (resolved.is_in_tree() && !trust_all_env()).then(TrustStore::open_default);
    stamp_marker_with(resolved, tools_dir, store)
}

/// `store` is `None` when the venv needs no token. A marker that already holds
/// the store's token keeps it, so a resync never makes toolr's own venv read
/// as foreign. A store that fails leaves the marker empty, so the next run
/// rebuilds the venv rather than the sync failing.
fn stamp_marker_with(
    resolved: &ResolvedVenv,
    tools_dir: &Path,
    store: Option<Result<TrustStore>>,
) -> Result<()> {
    let root = tools_dir.parent().unwrap_or(tools_dir);
    let venv_dir = &resolved.venv_dir;
    let token = store.map(|s| {
        s.and_then(|s| match marker_token(&s, venv_dir) {
            Some(token) => Ok(token),
            None => s.record_venv(root, venv_dir),
        })
    });
    let token = match token {
        None => String::new(),
        Some(Ok(token)) => token,
        Some(Err(e)) => {
            eprintln!(
                "toolr: warning: couldn't record the tools venv in the trust store, so the next run rebuilds it: {e:#}"
            );
            String::new()
        }
    };
    fs::create_dir_all(&resolved.venv_dir)?;
    fs::write(resolved.venv_dir.join(FRESHNESS_MARKER), token)?;
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
            let store = TrustStore::at(tmp.path().join("state").join("trust"));
            store.trust(&root).unwrap();
            let resolved = ResolvedVenv {
                repo_key: String::new(),
                ..dummy_resolved(venv)
            };
            std::thread::sleep(Duration::from_millis(20));
            Self {
                _tmp: tmp,
                tools,
                store,
                resolved,
            }
        }

        fn check(&self) -> Freshness {
            check_freshness_with(&self.resolved, &self.tools, Some(&self.store), false)
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
        let no_store = check_freshness_with(&t.resolved, &t.tools, None, false);
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
            check_freshness_with(&resolved, tmp.path(), None, false),
            Freshness::Fresh
        );
    }

    #[test]
    fn trust_all_skips_the_token_check() {
        let t = InTree::new();
        touch_marker(&t.resolved.venv_dir).unwrap();
        assert_eq!(
            check_freshness_with(&t.resolved, &t.tools, None, true),
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

    /// Another toolr finishes building the venv while this one waits for the
    /// lock: the re-check sees it fresh, so nothing is removed or synced.
    #[test]
    fn sync_locked_rechecks_after_waiting_for_the_lock() {
        let t = InTree::new();
        sync_locked(
            &t.resolved,
            &t.tools,
            false,
            || t.check(),
            || t.stamp(),
            sync_fails,
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
            || {
                events.borrow_mut().push("check");
                t.check()
            },
            || events.borrow_mut().push("lock"),
            || {
                assert!(!t.resolved.venv_dir.exists(), "removed before the sync");
                events.borrow_mut().push("sync");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*events.borrow(), ["check", "lock", "check", "sync"]);
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
            || t.check(),
            || (),
            sync_fails,
        )
        .unwrap_err();
        assert!(err.to_string().contains("sync must not run"), "{err}");
    }

    #[test]
    fn sync_locked_skips_the_lock_when_fresh() {
        let t = InTree::new();
        t.stamp();
        let locked = std::cell::Cell::new(false);
        sync_locked(
            &t.resolved,
            &t.tools,
            false,
            || t.check(),
            || locked.set(true),
            sync_fails,
        )
        .unwrap();
        assert!(!locked.get());
    }

    #[test]
    fn is_foreign_matches_the_freshness_check() {
        let t = InTree::new();
        assert!(is_foreign_with(
            &t.resolved,
            &t.tools,
            Some(&t.store),
            false
        ));
        assert!(!is_foreign_with(
            &t.resolved,
            &t.tools,
            Some(&t.store),
            true
        ));
        t.stamp();
        assert!(!is_foreign_with(
            &t.resolved,
            &t.tools,
            Some(&t.store),
            false
        ));
    }

    /// Through the real env: the default store under `XDG_STATE_HOME`.
    #[cfg(unix)]
    #[test]
    fn sync_if_needed_fails_when_a_foreign_venv_cant_be_removed() {
        use std::os::unix::fs::PermissionsExt;
        let t = InTree::new();
        let mut env = crate::test_env::EnvGuard::write();
        env.set("XDG_STATE_HOME", t._tmp.path().join("default-state"));
        env.remove("TOOLR_TRUST_ALL");
        let uv = stub_uv(t._tmp.path(), 0);
        fs::set_permissions(&t.tools, fs::Permissions::from_mode(0o555)).unwrap();
        let result = sync_if_needed(&uv, &t.tools, &t.resolved, false, true, &UpgradeMode::None);
        fs::set_permissions(&t.tools, fs::Permissions::from_mode(0o755)).unwrap();
        let err = result.expect_err("a venv that stays must not be synced");
        assert!(
            matches!(&err, UvError::Io(_)) && err.to_string().contains("couldn't remove"),
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
