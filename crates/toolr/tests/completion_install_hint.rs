//! `self completion install bash` tells the user to source the script when
//! bash-completion isn't there to load it.

mod common;

use std::path::{Path, PathBuf};
use std::process::Output;

use tempfile::TempDir;
use toolr_core::complete::shell_quote;

const HINT: &str = "can't find bash-completion";

/// A HOME, XDG data home and probe sysroot, all under one tempdir, so the
/// host's own bash-completion can't leak into the result.
struct Host {
    tmp: TempDir,
}

impl Host {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        for dir in ["home", "data", "sysroot", "data-dirs"] {
            std::fs::create_dir_all(tmp.path().join(dir)).unwrap();
        }
        Self { tmp }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.tmp.path().join(rel)
    }

    /// Plant a loader at `rel` under `root`, a directory below the tempdir.
    fn plant_loader(&self, root: &str, rel: &str) {
        let path = self.path(root).join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "# loader\n").unwrap();
    }

    fn command(&self, args: &[&str]) -> assert_cmd::Command {
        let mut cmd = common::toolr();
        cmd.env("HOME", self.path("home"))
            .env("XDG_DATA_HOME", self.path("data"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env("XDG_DATA_DIRS", self.path("data-dirs"))
            .env("TOOLR_BASH_COMPLETION_SYSROOT", self.path("sysroot"))
            .env_remove("HOMEBREW_PREFIX")
            .args(args);
        cmd
    }

    fn install(&self, args: &[&str]) -> (String, String) {
        succeeded(args, &self.command(args).output().unwrap())
    }

    fn installed_bash_script(&self) -> PathBuf {
        self.path("data").join("bash-completion/completions/toolr")
    }
}

fn succeeded(args: &[&str], out: &Output) -> (String, String) {
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "{args:?} failed:\n{stdout}\n{stderr}");
    (stdout, stderr)
}

/// Quoted the way the binary quotes it: temp dirs can hold spaces (a
/// `TMPDIR` with one) or backslashes (Windows). The full sentence is
/// checked by the unit tests in `dispatch.rs`.
fn source_line(path: &Path) -> String {
    format!("`source {}`", shell_quote(&path.display().to_string()))
}

const BASH: &[&str] = &["self", "completion", "install", "bash"];

#[test]
fn hint_names_the_installed_path_without_bash_completion() {
    let host = Host::new();
    let (stdout, stderr) = host.install(BASH);
    assert!(stdout.contains(HINT), "{stdout}");
    assert!(
        stdout.contains(&source_line(&host.installed_bash_script())),
        "{stdout}"
    );
    assert!(!stderr.contains(HINT), "hint belongs on stdout: {stderr}");
    assert!(host.installed_bash_script().is_file());
}

#[test]
fn hint_repeats_when_the_script_is_already_installed() {
    let host = Host::new();
    host.install(BASH);
    let (stdout, _) = host.install(BASH);
    assert!(stdout.contains("already up to date"), "{stdout}");
    assert!(stdout.contains(HINT), "{stdout}");
}

#[test]
fn no_hint_when_bash_completion_is_installed() {
    let host = Host::new();
    host.plant_loader("sysroot", "usr/share/bash-completion/bash_completion");
    let (stdout, _) = host.install(BASH);
    assert!(stdout.contains("wrote bash completion script"), "{stdout}");
    assert!(!stdout.contains(HINT), "{stdout}");
}

#[test]
fn no_hint_when_xdg_data_dirs_holds_bash_completion() {
    let host = Host::new();
    host.plant_loader("data-dirs", "bash-completion/bash_completion");
    let (stdout, _) = host.install(BASH);
    assert!(!stdout.contains(HINT), "{stdout}");
}

#[test]
fn quiet_suppresses_the_hint() {
    let host = Host::new();
    let (stdout, _) = host.install(&["--quiet", "self", "completion", "install", "bash"]);
    assert!(!stdout.contains(HINT), "{stdout}");
    assert!(host.installed_bash_script().is_file());
}

#[test]
fn other_shells_get_no_bash_hint() {
    let host = Host::new();
    for shell in ["zsh", "fish"] {
        let (stdout, _) = host.install(&["self", "completion", "install", shell]);
        assert!(!stdout.contains(HINT), "{shell}: {stdout}");
    }
}

#[test]
fn hint_quotes_a_home_with_a_space_and_a_quote() {
    let host = Host::new();
    let home = host.path("it's my home");
    std::fs::create_dir_all(&home).unwrap();
    let out = host
        .command(BASH)
        .env("HOME", &home)
        .env_remove("XDG_DATA_HOME")
        .output()
        .unwrap();
    let (stdout, _) = succeeded(BASH, &out);
    // Joined the way `install_path_for` joins it, so the separators match on Windows.
    let script = home
        .join(".local/share")
        .join("bash-completion/completions/toolr");
    assert!(script.is_file());
    let quoted = format!("'{}'", script.display().to_string().replace('\'', r"'\''"));
    assert!(stdout.contains(HINT), "{stdout}");
    assert!(stdout.contains(&format!("`source {quoted}`")), "{stdout}");
}

#[test]
fn hint_prints_when_only_a_bash_completion_1x_loader_exists() {
    // Homebrew's 1.x layout; the unit tests cover every 1.x path.
    let host = Host::new();
    host.plant_loader("sysroot", "opt/homebrew/etc/bash_completion");
    host.plant_loader("sysroot", "opt/homebrew/etc/profile.d/bash_completion.sh");
    let (stdout, _) = host.install(BASH);
    assert!(stdout.contains(HINT), "{stdout}");
}

#[test]
fn no_hint_when_the_install_is_refused_without_force() {
    let host = Host::new();
    let script = host.installed_bash_script();
    std::fs::create_dir_all(script.parent().unwrap()).unwrap();
    std::fs::write(&script, "# edited by hand\n").unwrap();
    let out = host.command(BASH).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stdout}\n{stderr}");
    assert!(stderr.contains("refusing to overwrite"), "{stderr}");
    assert!(!stdout.contains(HINT), "{stdout}");
    assert!(!stderr.contains(HINT), "{stderr}");
}

#[test]
fn no_hint_when_homebrew_prefix_holds_bash_completion() {
    let host = Host::new();
    host.plant_loader("brew", "share/bash-completion/bash_completion");
    let out = host
        .command(BASH)
        .env("HOMEBREW_PREFIX", host.path("brew"))
        .output()
        .unwrap();
    let (stdout, _) = succeeded(BASH, &out);
    assert!(stdout.contains("wrote bash completion script"), "{stdout}");
    assert!(!stdout.contains(HINT), "{stdout}");
}

#[test]
fn force_reinstall_prints_the_hint() {
    let host = Host::new();
    host.install(BASH);
    let args = &[BASH, &["--force"]].concat();
    let (stdout, _) = host.install(args);
    assert!(stdout.contains("content unchanged"), "{stdout}");
    assert!(stdout.contains(HINT), "{stdout}");
    assert!(
        stdout.contains(&source_line(&host.installed_bash_script())),
        "{stdout}"
    );
}

/// Runs `args` with empty XDG homes from a fresh cwd, and checks nothing
/// lands in that cwd.
fn install_with_empty_xdg_homes(host: &Host, args: &[&str]) -> String {
    let cwd = host.path("cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    let out = host
        .command(args)
        .env("XDG_DATA_HOME", "")
        .env("XDG_CONFIG_HOME", "")
        .current_dir(&cwd)
        .output()
        .unwrap();
    let (stdout, _) = succeeded(args, &out);
    assert_eq!(std::fs::read_dir(&cwd).unwrap().count(), 0);
    stdout
}

#[test]
fn empty_xdg_data_home_reads_as_unset_for_bash() {
    let host = Host::new();
    let stdout = install_with_empty_xdg_homes(&host, BASH);
    let script = host
        .path("home")
        .join(".local/share")
        .join("bash-completion/completions/toolr");
    assert!(script.is_file());
    assert!(stdout.contains(HINT), "{stdout}");
    assert!(stdout.contains(&source_line(&script)), "{stdout}");
}

#[test]
fn empty_xdg_config_home_reads_as_unset_for_fish() {
    let host = Host::new();
    install_with_empty_xdg_homes(&host, &["self", "completion", "install", "fish"]);
    assert!(
        host.path("home")
            .join(".config/fish/completions/toolr.fish")
            .is_file()
    );
}

#[test]
fn empty_homebrew_prefix_does_not_probe_the_cwd() {
    let host = Host::new();
    host.plant_loader("cwd", "share/bash-completion/bash_completion");
    let out = host
        .command(BASH)
        .env("HOMEBREW_PREFIX", "")
        .current_dir(host.path("cwd"))
        .output()
        .unwrap();
    let (stdout, _) = succeeded(BASH, &out);
    assert!(stdout.contains(HINT), "{stdout}");
}

/// APFS rejects non-UTF-8 file names, so this runs on Linux only.
#[cfg(target_os = "linux")]
#[test]
fn hint_names_no_path_when_home_is_not_utf8() {
    use std::os::unix::ffi::OsStrExt;
    let host = Host::new();
    let home = host
        .tmp
        .path()
        .join(std::ffi::OsStr::from_bytes(b"home-\xff"));
    std::fs::create_dir_all(&home).unwrap();
    let out = host
        .command(BASH)
        .env("HOME", &home)
        .env_remove("XDG_DATA_HOME")
        .output()
        .unwrap();
    let (stdout, _) = succeeded(BASH, &out);
    assert!(
        home.join(".local/share/bash-completion/completions/toolr")
            .is_file()
    );
    let hint = stdout
        .lines()
        .find(|line| line.contains(HINT))
        .unwrap_or_else(|| panic!("no hint: {stdout}"));
    assert!(hint.contains("a `source` line"), "{hint}");
    assert!(!hint.contains("`source /"), "{hint}");
}

#[test]
fn empty_home_fails_completion_install() {
    let host = Host::new();
    let output = host
        .command(&["self", "completion", "install", "bash"])
        .env("HOME", "")
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("$HOME is not set"), "stderr:\n{stderr}");
}
