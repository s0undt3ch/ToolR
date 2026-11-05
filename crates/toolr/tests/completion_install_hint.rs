//! `self completion install bash` tells the user to source the script when
//! bash-completion isn't there to load it.

mod common;

use std::path::{Path, PathBuf};

use tempfile::TempDir;

const HINT: &str = "bash-completion isn't installed";

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

    fn plant_loader(&self, rel: &str) {
        let path = self.path("sysroot").join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "# loader\n").unwrap();
    }

    fn install(&self, args: &[&str]) -> (String, String) {
        let out = common::toolr()
            .env("HOME", self.path("home"))
            .env("XDG_DATA_HOME", self.path("data"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env("XDG_DATA_DIRS", self.path("data-dirs"))
            .env("TOOLR_BASH_COMPLETION_SYSROOT", self.path("sysroot"))
            .env_remove("HOMEBREW_PREFIX")
            .args(args)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(out.status.success(), "{args:?} failed:\n{stdout}\n{stderr}");
        (stdout, stderr)
    }

    fn installed_bash_script(&self) -> PathBuf {
        self.path("data").join("bash-completion/completions/toolr")
    }
}

fn source_line(path: &Path) -> String {
    format!("add `source {}` to your ~/.bashrc.", path.display())
}

#[test]
fn hint_names_the_installed_path_without_bash_completion() {
    let host = Host::new();
    let (stdout, stderr) = host.install(&["self", "completion", "install", "bash"]);
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
    host.install(&["self", "completion", "install", "bash"]);
    let (stdout, _) = host.install(&["self", "completion", "install", "bash"]);
    assert!(stdout.contains("already up to date"), "{stdout}");
    assert!(stdout.contains(HINT), "{stdout}");
}

#[test]
fn no_hint_when_bash_completion_is_installed() {
    let host = Host::new();
    host.plant_loader("usr/share/bash-completion/bash_completion");
    let (stdout, _) = host.install(&["self", "completion", "install", "bash"]);
    assert!(stdout.contains("wrote bash completion script"), "{stdout}");
    assert!(!stdout.contains(HINT), "{stdout}");
}

#[test]
fn no_hint_when_xdg_data_dirs_holds_bash_completion() {
    let host = Host::new();
    let loader = host
        .path("data-dirs")
        .join("bash-completion/bash_completion");
    std::fs::create_dir_all(loader.parent().unwrap()).unwrap();
    std::fs::write(&loader, "# loader\n").unwrap();
    let (stdout, _) = host.install(&["self", "completion", "install", "bash"]);
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
