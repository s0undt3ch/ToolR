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
    uv_script(tmp, &format!("exit {exit_code}"))
}

/// A uv-binary stub at `tmp/uv` that runs the shell `script`.
#[cfg(unix)]
fn uv_script(tmp: &Path, script: &str) -> UvBinary {
    use std::os::unix::fs::PermissionsExt;
    let stub = tmp.join("uv");
    fs::write(&stub, format!("#!/bin/sh\n{script}\n")).unwrap();
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
        // Where `open_default` finds it under `XDG_STATE_HOME=<tmp>/state`.
        let store = TrustStore::at(tmp.path().join("state").join("toolr").join("trust"));
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
    let path = std::ffi::CString::new(t.marker().into_os_string().into_encoded_bytes()).unwrap();
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
            approve_removal(&t.resolved.venv_dir, false, Some((&mut input, &mut out))).unwrap_err(),
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

/// A claim never vouches for a venv, not even while its builder holds the sync lock: only a
/// matching marker does. The stamp settles the claim, and a rebuild keeps the token.
#[test]
fn a_claimed_venv_without_a_marker_is_foreign_until_stamped() {
    let t = InTree::new();
    let root = t.tools.parent().unwrap();
    let venv = &t.resolved.venv_dir;
    assert_eq!(t.check(), Freshness::Foreign);
    let lock = t.store.lock_venv(venv, true).unwrap();
    t.store.claim_venv(root, venv).unwrap();
    assert_eq!(t.check(), Freshness::Foreign, "the builder holds the lock");
    touch_marker(venv).unwrap();
    assert_eq!(t.check(), Freshness::Foreign, "a marker that doesn't match");
    fs::remove_file(t.marker()).unwrap();
    t.stamp();
    drop(lock);
    assert!(!t.store.venv_pending(venv), "the stamp settles it");
    assert_eq!(t.check(), Freshness::Fresh);
    let token = fs::read_to_string(t.marker()).unwrap();
    // A rebuild keeps the token, and its stamp settles the claim again.
    t.store.claim_venv(root, venv).unwrap();
    assert_eq!(t.check(), Freshness::Fresh);
    t.stamp();
    assert_eq!(fs::read_to_string(t.marker()).unwrap(), token);
    assert!(!t.store.venv_pending(venv));
}

/// A sync killed part way leaves its claim and releases its lock; the next toolr to take the
/// lock ends the claim. Neither before nor after does it vouch for the venv.
#[test]
fn a_claim_left_by_an_interrupted_sync_covers_nothing() {
    let t = InTree::new();
    let venv = &t.resolved.venv_dir;
    let killed = t.store.lock_venv(venv, true).unwrap();
    t.store.claim_venv(t.tools.parent().unwrap(), venv).unwrap();
    fs::write(venv.join(".gitignore"), "*\n").unwrap();
    drop(killed);
    assert!(t.store.venv_pending(venv));
    assert_eq!(t.check(), Freshness::Foreign);

    let _lock = t.store.lock_venv(venv, true).unwrap();
    assert!(!t.store.venv_pending(venv), "the lock holder ends it");
    assert_eq!(t.check(), Freshness::Foreign, "under the lock too");
}

/// A killed sync's claim, its lock file left behind (as a SIGKILL leaves it,
/// and as Windows always does), and another toolr holding that lock for a moment, say while
/// it checks the venv. The claim still doesn't vouch for the venv, so nothing runs it.
#[cfg(unix)]
#[test]
fn a_stale_claim_never_vouches_while_another_toolr_holds_the_lock() {
    let t = InTree::new();
    let root = t.tools.parent().unwrap();
    let venv = &t.resolved.venv_dir;
    let killed = t.store.lock_venv(venv, true).unwrap();
    t.store.claim_venv(root, venv).unwrap();
    let lock_file = fs::read_dir(t.store.dir().join("locks"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    drop(killed);
    let held = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_file)
        .unwrap();
    held.try_lock().unwrap();

    assert!(t.store.venv_pending(venv));
    assert_eq!(t.check(), Freshness::Foreign);
    assert!(is_foreign_with(
        &t.resolved,
        root,
        TokenCheck::Store(&t.store)
    ));
}

/// Points the default trust store at `t.store`, and the other state toolr writes at `t`'s
/// temp dir, so the real sync runs against them.
fn default_store_env(t: &InTree) -> crate::test_env::EnvGuard {
    let mut env = crate::test_env::EnvGuard::write();
    let tmp = t._tmp.path();
    env.set("XDG_STATE_HOME", tmp.join("state"));
    env.set("XDG_CACHE_HOME", tmp.join("cache"));
    env.set("XDG_DATA_HOME", tmp.join("data"));
    env.remove("TOOLR_TRUST_ALL");
    env
}

/// What uv leaves when a first sync fails after it created the venv.
#[cfg(unix)]
const UV_FAILS_IN_A_NEW_VENV: &str = r#"mkdir -p "$UV_PROJECT_ENVIRONMENT/bin"
printf '*\n' > "$UV_PROJECT_ENVIRONMENT/.gitignore"
"#;

/// The reported shape, through the real sync: the first sync fails after uv created the
/// venv and its `.gitignore`, then a checkout puts files there. None of it is toolr's.
#[cfg(unix)]
#[test]
fn a_failed_first_sync_leaves_no_venv_and_no_claim() {
    let t = InTree::new();
    let _env = default_store_env(&t);
    let venv = &t.resolved.venv_dir;
    fs::remove_dir_all(venv).unwrap();
    let uv = uv_script(t._tmp.path(), &format!("{UV_FAILS_IN_A_NEW_VENV}exit 1"));
    let err = sync_if_needed(&uv, &t.tools, &t.resolved, false, true, &UpgradeMode::None)
        .expect_err("uv failed");
    assert!(matches!(err, UvError::SyncFailed(Some(1))), "{err:?}");
    assert!(
        fs::symlink_metadata(venv).is_err(),
        "what uv left is removed"
    );
    assert!(!t.store.venv_pending(venv));

    venv_with_interpreter(venv);
    assert_eq!(t.check(), Freshness::Foreign);
}

/// When what uv left can't be removed the directory stays, but the claim still ends, so
/// files a checkout writes into it are asked about.
#[cfg(unix)]
#[test]
fn files_checked_out_into_a_failed_first_syncs_venv_are_foreign() {
    use std::os::unix::fs::PermissionsExt;
    let t = InTree::new();
    let _env = default_store_env(&t);
    let venv = &t.resolved.venv_dir;
    fs::remove_dir_all(venv).unwrap();
    let script = format!(
        "{UV_FAILS_IN_A_NEW_VENV}chmod 555 \"$(dirname \"$UV_PROJECT_ENVIRONMENT\")\"\nexit 1"
    );
    let uv = uv_script(t._tmp.path(), &script);
    let result = sync_if_needed(&uv, &t.tools, &t.resolved, false, true, &UpgradeMode::None);
    fs::set_permissions(&t.tools, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        matches!(result, Err(UvError::SyncFailed(Some(1)))),
        "{result:?}"
    );
    assert!(venv.is_dir(), "tools/ refused the removal");
    assert!(!t.store.venv_pending(venv));

    venv_with_interpreter(venv);
    assert_eq!(t.check(), Freshness::Foreign);
}

/// A failed resync of toolr's own venv leaves the venv in place and toolr's.
#[cfg(unix)]
#[test]
fn a_failed_resync_keeps_toolrs_venv() {
    let t = InTree::new();
    let _env = default_store_env(&t);
    t.stamp();
    let uv = stub_uv(t._tmp.path(), 3);
    let err = sync_if_needed(&uv, &t.tools, &t.resolved, true, true, &UpgradeMode::None)
        .expect_err("uv failed");
    assert!(matches!(err, UvError::SyncFailed(Some(3))), "{err:?}");
    assert!(t.resolved.python.is_file());
    assert!(!t.store.venv_pending(&t.resolved.venv_dir));
    assert_eq!(t.check(), Freshness::Fresh);
}

/// Another toolr is building the venv: the sync waits for its lock rather than asking about
/// the unstamped venv, and finds it fresh once the builder stamps it.
#[cfg(unix)]
#[test]
fn a_sync_waits_for_a_claimed_build_before_asking() {
    let t = InTree::new();
    let _env = default_store_env(&t);
    let venv = &t.resolved.venv_dir;
    let builder = t.store.lock_venv(venv, true).unwrap();
    t.store.claim_venv(t.tools.parent().unwrap(), venv).unwrap();
    let uv = stub_uv(t._tmp.path(), 7);
    std::thread::scope(|scope| {
        let waiter = scope
            .spawn(|| sync_if_needed(&uv, &t.tools, &t.resolved, false, true, &UpgradeMode::None));
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            !waiter.is_finished(),
            "asked instead of waiting for the builder"
        );
        t.stamp();
        drop(builder);
        let synced = waiter.join().unwrap();
        assert!(synced.is_ok(), "{synced:?}");
    });
    assert_eq!(t.check(), Freshness::Fresh);
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
        matches!(&err, UvError::UntrustedVenv(_)) && err.to_string().contains("couldn't remove"),
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
