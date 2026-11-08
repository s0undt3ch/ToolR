//! `self completion install powershell`: where the script lands, the profile hint, and the shared
//! idempotence and `--force` behaviour.

mod common;

use std::path::PathBuf;
use std::process::Output;

use tempfile::TempDir;
use toolr_core::complete::powershell_quote;

/// A HOME and XDG data home under one tempdir, so nothing touches the real profile directories.
struct Host {
    tmp: TempDir,
}

impl Host {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("home")).unwrap();
        Self { tmp }
    }

    fn command(&self, args: &[&str]) -> assert_cmd::Command {
        let mut cmd = common::toolr();
        cmd.env("HOME", self.tmp.path().join("home"))
            .env("XDG_DATA_HOME", self.tmp.path().join("data"))
            .args(args);
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// Joined the way `install_path_for` joins it, so the separators match on Windows.
    fn script(&self) -> PathBuf {
        self.tmp
            .path()
            .join("data")
            .join("toolr/completions/toolr.ps1")
    }
}

const INSTALL: &[&str] = &["self", "completion", "install", "powershell"];

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn assert_ok(out: &Output) {
    assert!(
        out.status.success(),
        "{}\n{}",
        stdout(out),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn printed_script() -> Vec<u8> {
    let out = common::toolr()
        .args(["self", "completion", "print", "powershell"])
        .output()
        .unwrap();
    assert_ok(&out);
    out.stdout
}

#[test]
fn install_powershell_writes_script_under_xdg_data_home() {
    let host = Host::new();
    assert_ok(&host.run(INSTALL));
    assert_eq!(std::fs::read(host.script()).unwrap(), printed_script());
}

#[test]
fn install_powershell_prints_profile_hint() {
    let host = Host::new();
    let out = host.run(INSTALL);
    assert_ok(&out);
    let stdout = stdout(&out);
    let quoted = powershell_quote(host.script().to_str().unwrap());
    assert!(stdout.contains(&format!("`. {quoted}`")), "{stdout}");
    assert!(stdout.contains("$PROFILE"), "{stdout}");
}

#[test]
fn install_pwsh_alias_installs_the_same_file() {
    let host = Host::new();
    let out = host.run(&["self", "completion", "install", "pwsh"]);
    assert_ok(&out);
    assert!(stdout(&out).contains("wrote powershell completion script"));
    assert_eq!(std::fs::read(host.script()).unwrap(), printed_script());
}

#[test]
fn install_powershell_twice_is_already_up_to_date() {
    let host = Host::new();
    assert_ok(&host.run(INSTALL));
    let out = host.run(INSTALL);
    assert_ok(&out);
    let stdout = stdout(&out);
    assert!(stdout.contains("already up to date"), "{stdout}");
    assert!(stdout.contains("$PROFILE"), "{stdout}");
}

fn edit_installed_script(host: &Host) {
    let script = host.script();
    std::fs::create_dir_all(script.parent().unwrap()).unwrap();
    std::fs::write(&script, "# edited by hand\n").unwrap();
}

#[test]
fn install_powershell_refuses_a_changed_file_without_force() {
    let host = Host::new();
    edit_installed_script(&host);
    let out = host.run(INSTALL);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(String::from_utf8_lossy(&out.stderr).contains("refusing to overwrite"));
    assert!(!stdout(&out).contains("$PROFILE"));
    assert_eq!(
        std::fs::read_to_string(host.script()).unwrap(),
        "# edited by hand\n"
    );
}

#[test]
fn install_powershell_force_replaces_a_changed_file() {
    let host = Host::new();
    edit_installed_script(&host);
    let mut args = INSTALL.to_vec();
    args.push("--force");
    let out = host.run(&args);
    assert_ok(&out);
    assert!(stdout(&out).contains("replaced existing powershell completion script"));
    assert_eq!(std::fs::read(host.script()).unwrap(), printed_script());
}

#[test]
fn install_powershell_quiet_drops_the_hint() {
    let host = Host::new();
    let mut args = vec!["--quiet"];
    args.extend_from_slice(INSTALL);
    let out = host.run(&args);
    assert_ok(&out);
    assert_eq!(stdout(&out), "");
    assert!(host.script().is_file());
}

/// A stock Windows account has no `HOME`; the platform home stands in. On Unix the fallback is
/// covered by the `resolve_home` unit tests, since `dirs::home_dir()` would find the real home.
#[cfg(windows)]
#[test]
fn install_without_home_uses_platform_home() {
    let host = Host::new();
    let out = host.command(INSTALL).env_remove("HOME").output().unwrap();
    assert_ok(&out);
    assert!(host.script().is_file());
}
