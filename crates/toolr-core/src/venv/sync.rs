//! Drive `uv sync` to materialise the tools venv.

use std::fs;
use std::path::Path;
use std::process::{Command, ExitStatus};
use std::time::SystemTime;

use anyhow::{Context, Result};

use crate::uv::{UvBinary, UvError};

use super::resolve::ResolvedVenv;

/// Marker file written into the venv after each successful sync.
/// Its mtime is compared against `tools/uv.lock`'s mtime to decide
/// whether re-sync is needed.
const FRESHNESS_MARKER: &str = ".toolr-sync-stamp";

/// Decision returned by `is_fresh`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// Venv has never been synced (marker absent or venv missing).
    Missing,
    /// Lock has been edited since last sync.
    Stale,
    /// Marker mtime >= lock mtime.
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

pub fn check_freshness(resolved: &ResolvedVenv, tools_dir: &Path) -> Freshness {
    let marker = resolved.venv_dir.join(FRESHNESS_MARKER);
    let lock = tools_dir.join("uv.lock");
    let (Ok(marker_meta), Ok(lock_meta)) = (fs::metadata(&marker), fs::metadata(&lock)) else {
        return Freshness::Missing;
    };
    let marker_t = marker_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let lock_t = lock_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    if marker_t >= lock_t {
        Freshness::Fresh
    } else {
        Freshness::Stale
    }
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
        touch_marker(&resolved.venv_dir)?;
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
    if !bypass_stamp && matches!(check_freshness(resolved, tools_dir), Freshness::Fresh) {
        return Ok(());
    }
    let status = run_uv_sync(uv, tools_dir, resolved, upgrade, quiet)
        .map_err(|e| UvError::Http(e.to_string()))?;
    if !status.success() {
        return Err(UvError::SyncFailed(status.code()));
    }
    Ok(())
}

fn touch_marker(venv_dir: &Path) -> Result<()> {
    fs::create_dir_all(venv_dir)?;
    fs::write(venv_dir.join(FRESHNESS_MARKER), b"")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::TempDir;

    fn dummy_resolved(venv_dir: std::path::PathBuf) -> ResolvedVenv {
        ResolvedVenv {
            venv_dir: venv_dir.clone(),
            python: venv_dir.join("bin").join("python"),
            repo_key: "x".into(),
            python_version: "3.13".into(),
            config: Default::default(),
        }
    }

    #[test]
    fn missing_marker_or_lock_reports_missing() {
        let tmp = TempDir::new().unwrap();
        let resolved = dummy_resolved(tmp.path().join("venv"));
        assert_eq!(
            check_freshness(&resolved, tmp.path()),
            Freshness::Missing
        );
    }

    #[test]
    fn marker_older_than_lock_reports_stale() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join("venv");
        fs::create_dir_all(&venv).unwrap();
        touch_marker(&venv).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(tmp.path().join("uv.lock"), b"locks").unwrap();
        let resolved = dummy_resolved(venv);
        assert_eq!(
            check_freshness(&resolved, tmp.path()),
            Freshness::Stale
        );
    }

    #[test]
    fn marker_newer_than_lock_reports_fresh() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join("venv");
        fs::create_dir_all(&venv).unwrap();
        std::fs::write(tmp.path().join("uv.lock"), b"locks").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        touch_marker(&venv).unwrap();
        let resolved = dummy_resolved(venv);
        assert_eq!(
            check_freshness(&resolved, tmp.path()),
            Freshness::Fresh
        );
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
        fs::create_dir_all(&venv).unwrap();
        fs::write(tmp.path().join("uv.lock"), b"locks").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        touch_marker(&venv).unwrap();

        let uv = stub_uv(tmp.path(), 99);
        let resolved = dummy_resolved(venv);
        sync_if_needed(&uv, tmp.path(), &resolved, false, false, &UpgradeMode::None).expect("fresh should short-circuit");
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
        // status() returns Err → sync_if_needed maps that via
        // `map_err(|e| UvError::Http(e.to_string()))`.
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
        assert!(matches!(err, UvError::Http(_)));
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

        run_uv_sync(&uv, tmp.path(), &resolved, &UpgradeMode::None, /*quiet=*/ true)
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

        run_uv_sync(&uv, tmp.path(), &resolved, &UpgradeMode::None, /*quiet=*/ false)
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
        writeln!(f, "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}", argdump.display()).unwrap();
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

        run_uv_sync(&uv, tmp.path(), &resolved, &UpgradeMode::None, /*quiet=*/ false).expect("stub uv should succeed");
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
        writeln!(f, "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}", argdump.display()).unwrap();
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

        run_uv_sync(&uv, tmp.path(), &resolved, &UpgradeMode::All, /*quiet=*/ false)
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
        writeln!(f, "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}", argdump.display()).unwrap();
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
        writeln!(f, "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}", argdump.display()).unwrap();
        drop(f);
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
        let uv = UvBinary {
            path: stub,
            version: (0, 4, 0),
            source: crate::uv::UvSource::Path,
        };
        let venv = tmp.path().join("venv");
        let resolved = dummy_resolved(venv);

        run_uv_lock(&uv, tmp.path(), &resolved, &UpgradeMode::None, /*quiet=*/ false)
            .expect("stub uv should succeed");

        let dump = fs::read_to_string(&argdump).unwrap();
        assert!(dump.contains("lock"), "args: {dump}");
        assert!(dump.contains("--project"), "args: {dump}");
        assert!(!dump.contains("--upgrade"), "args should not contain --upgrade: {dump}");
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
        writeln!(f, "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}", argdump.display()).unwrap();
        drop(f);
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
        let uv = UvBinary {
            path: stub,
            version: (0, 4, 0),
            source: crate::uv::UvSource::Path,
        };
        let venv = tmp.path().join("venv");
        let resolved = dummy_resolved(venv);

        run_uv_lock(&uv, tmp.path(), &resolved, &UpgradeMode::All, /*quiet=*/ false)
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
        writeln!(f, "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}", argdump.display()).unwrap();
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
        assert_eq!(occurrences, 2, "expected 2 --upgrade-package tokens: {dump}");
        assert!(dump.contains("foo"));
        assert!(dump.contains("bar"));
    }

    #[test]
    fn touch_marker_creates_venv_dir_if_missing() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join("nested").join("venv");
        // Parent missing — create_dir_all should materialise it.
        touch_marker(&venv).unwrap();
        assert!(venv.join(FRESHNESS_MARKER).exists());
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
