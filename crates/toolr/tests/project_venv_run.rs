use assert_cmd::Command;
use tempfile::TempDir;

/// A tools/pyproject.toml pinned to an in-tree venv so the resolved venv
/// path is deterministically `tools/.venv` (no cache-key hashing).
const IN_TREE_PYPROJECT: &str =
    "[project]\nname=\"x\"\nversion=\"0\"\n\n[tool.toolr]\nvenv-location = \"in-tree\"\n";

fn write_tools(repo: &std::path::Path, pyproject: &str) {
    let tools = repo.join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    std::fs::write(tools.join("pyproject.toml"), pyproject).unwrap();
}

#[test]
fn no_sync_errors_when_venv_missing() {
    let tmp = TempDir::new().unwrap();
    write_tools(tmp.path(), IN_TREE_PYPROJECT);
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args([
            "project",
            "venv",
            "run",
            "--no-sync",
            "--",
            "python",
            "-c",
            "pass",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("hasn't been created"), "stderr: {stderr}");
}

#[test]
fn requires_a_command() {
    let tmp = TempDir::new().unwrap();
    write_tools(tmp.path(), IN_TREE_PYPROJECT);
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["project", "venv", "run"])
        .output()
        .unwrap();
    // clap rejects the missing required positional before dispatch.
    assert!(!output.status.success());
}

#[test]
fn help_mentions_no_sync() {
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .args(["project", "venv", "run", "--help"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--no-sync"), "help was: {stdout}");
}

/// Build an in-tree fake venv at `tools/.venv` good enough for
/// `validate_venv`: a runnable `bin/python` (body supplied) plus a fake
/// installed `toolr` package. Returns the venv dir. Unix-only: writes a
/// `#!/bin/sh` interpreter and 0o755 perms.
#[cfg(unix)]
fn fake_in_tree_venv(repo: &std::path::Path, python_body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    write_tools(repo, IN_TREE_PYPROJECT);
    let venv = repo.join("tools").join(".venv");
    let bin = venv.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let python = bin.join("python");
    std::fs::write(&python, python_body).unwrap();
    std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).unwrap();
    let site = venv
        .join("lib")
        .join("python3.13")
        .join("site-packages")
        .join("toolr");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("__init__.py"), b"").unwrap();
    venv
}

/// Build an in-tree fake venv at `tools/.venv` good enough for
/// `validate_venv` on any platform: an interpreter placeholder (an empty
/// file — never executed by the tests that use this builder, since they
/// only exercise command-not-found resolution) at the OS-correct path
/// (`bin/python` on Unix, `Scripts\python.exe` on Windows), plus a fake
/// installed `toolr` package at the OS-correct site-packages layout
/// (`lib/pythonX.Y/site-packages/toolr/` on Unix, `Lib/site-packages/toolr/`
/// on Windows — see `toolr_core::venv::validate::candidate_site_packages`).
/// Returns the venv dir.
fn fake_in_tree_venv_cross_platform(repo: &std::path::Path) -> std::path::PathBuf {
    write_tools(repo, IN_TREE_PYPROJECT);
    let venv = repo.join("tools").join(".venv");

    let python = if cfg!(windows) {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    };
    std::fs::create_dir_all(python.parent().unwrap()).unwrap();
    std::fs::write(&python, b"").unwrap();

    let site = if cfg!(windows) {
        venv.join("Lib").join("site-packages").join("toolr")
    } else {
        venv.join("lib")
            .join("python3.13")
            .join("site-packages")
            .join("toolr")
    };
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("__init__.py"), b"").unwrap();

    venv
}

/// Mark the venv Fresh: uv.lock first, then the sync stamp (newer mtime).
fn mark_fresh(repo: &std::path::Path, venv: &std::path::Path) {
    std::fs::write(repo.join("tools").join("uv.lock"), b"lock").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(venv.join(".toolr-sync-stamp"), b"").unwrap();
}

#[cfg(unix)]
#[test]
fn no_sync_errors_when_stale() {
    let tmp = TempDir::new().unwrap();
    let venv = fake_in_tree_venv(tmp.path(), "#!/bin/sh\nexit 0\n");
    // Stamp older than uv.lock → Stale.
    std::fs::write(venv.join(".toolr-sync-stamp"), b"").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(tmp.path().join("tools").join("uv.lock"), b"lock").unwrap();
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["project", "venv", "run", "--no-sync", "--", "python"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("out of date"));
}

#[cfg(unix)]
#[test]
fn no_sync_fresh_passes_exit_code_through() {
    let tmp = TempDir::new().unwrap();
    let venv = fake_in_tree_venv(tmp.path(), "#!/bin/sh\nexit 3\n");
    mark_fresh(tmp.path(), &venv);
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["project", "venv", "run", "--no-sync", "--", "python"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(3),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn no_sync_passes_args_verbatim() {
    let tmp = TempDir::new().unwrap();
    let arglog = tmp.path().join("arglog");
    let body = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\nexit 0\n",
        arglog.display()
    );
    let venv = fake_in_tree_venv(tmp.path(), &body);
    mark_fresh(tmp.path(), &venv);
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args([
            "project",
            "venv",
            "run",
            "--no-sync",
            "--",
            "python",
            "-k",
            "foo",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let logged = std::fs::read_to_string(&arglog).unwrap();
    assert!(logged.lines().any(|l| l == "-k"), "argv: {logged}");
    assert!(logged.lines().any(|l| l == "foo"), "argv: {logged}");
}

#[test]
fn not_found_command_gets_nudge() {
    let tmp = TempDir::new().unwrap();
    let venv = fake_in_tree_venv_cross_platform(tmp.path());
    mark_fresh(tmp.path(), &venv);
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args([
            "project",
            "venv",
            "run",
            "--no-sync",
            "--",
            "toolr-definitely-absent-xyz",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(127),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("couldn't find `toolr-definitely-absent-xyz`"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("tools/pyproject.toml"), "stderr: {stderr}");
}

#[test]
fn project_init_next_steps_mention_venv_run() {
    let tmp = TempDir::new().unwrap();
    // --no-sync keeps this offline and fast; we only assert on the
    // scaffolding next-steps output, not on a real sync.
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["project", "init", "--no-sync"])
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("project venv run"),
        "next-steps should advertise `venv run`, got: {stdout}"
    );
}

/// This is the cross-platform vehicle for coverage that can't be faked
/// without a real synced venv: real `Scripts\*.exe` (Windows) / `bin/*`
/// (Unix) bare-command PATHEXT/PATH resolution, and exit-code passthrough
/// from a real interpreter. Run with `--ignored` on the Windows CI leg to
/// cover the Windows-specific behavior the other (Unix-only) tests in this
/// file can't reach with fake venvs.
#[test]
#[ignore = "network-touching: requires uv to be available or installable"]
fn runs_in_a_real_synced_venv() {
    let tmp = TempDir::new().unwrap();
    write_tools(
        tmp.path(),
        "[project]\nname=\"toolr-tools\"\nversion=\"0\"\nrequires-python=\">=3.11\"\ndependencies=[\"toolr\"]\n\n[tool.toolr]\nvenv-location = \"in-tree\"\n",
    );
    // Default path auto-syncs, then runs `python` from the venv.
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args([
            "project",
            "venv",
            "run",
            "--",
            "python",
            "-c",
            "print('ran-in-venv')",
        ])
        .env("TOOLR_AUTO_INSTALL_UV", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("ran-in-venv"));
}
