//! Dispatch must not run a tools venv that is out of date with a sync input
//! such as `tools/uv.lock` (#527; see `check_freshness`): a stale venv is
//! re-synced first, and when that can't happen the command is refused rather than run on old code. It also
//! warns when the venv's `toolr-py` minor version differs from the binary (#529).
//! In a trusted repository, an in-tree venv toolr didn't build is rebuilt, never run.
#![cfg(unix)]

use std::fs;
use std::path::Path;

use assert_cmd::Command;

#[path = "common/mod.rs"]
mod common;
use common::{TrustEnv, VenvFixture, add_planted_plugin};
use toolr_core::venv::SYNC_INPUTS_DESCRIPTION;

const HELLO_TOOLS: &str = "\"\"\"Hi.\"\"\"\nfrom toolr import command_group\ngroup = command_group(\"hello\", \"Hi\", \"Hi.\")\n@group.command\ndef world(ctx):\n    \"\"\"World.\"\"\"\n";

struct Project {
    fx: VenvFixture,
    sentinel: std::path::PathBuf,
    cache: std::path::PathBuf,
}

impl Project {
    fn toolr(&self, path: &str) -> Command {
        let mut cmd = common::toolr();
        cmd.current_dir(&self.fx.root)
            .env("PATH", path)
            .env("XDG_CACHE_HOME", &self.cache)
            .env_remove("TOOLR_AUTO_INSTALL_UV")
            .env_remove("TOOLR_AUTO_SYNC")
            .env_remove("TOOLR_VENV_LOCATION");
        cmd
    }

    fn path_with_uv(&self) -> String {
        self.fx.bin_dir.display().to_string()
    }

    fn reset_observations(&self) {
        let _ = fs::remove_file(&self.sentinel);
        let _ = fs::remove_file(&self.fx.uv_argv_log);
    }

    /// Rewrite `tools/uv.lock` so it is newer than the sync stamp, the way a
    /// `git pull` bumping a dependency would.
    fn bump_lock(&self) {
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(self.fx.tools_dir.join("uv.lock"), "version = 1\n# bumped\n").unwrap();
    }

    /// Leave `bin/python` dangling as `mise prune` does (#609); the swapped uv
    /// stub's `sync` restores it, as real uv rebuilds such a venv.
    fn prune_interpreter(&self) {
        let python = self.fx.venv_dir.join("bin").join("python");
        let saved = self.fx.root.join("python-before-prune");
        fs::rename(&python, &saved).unwrap();
        std::os::unix::fs::symlink(self.fx.root.join("pruned").join("python3.13"), &python)
            .unwrap();
        self.fx.write_uv_stub(&format!(
            "/bin/rm -f {python} && /bin/cp {saved} {python}",
            python = python.display(),
            saved = saved.display(),
        ));
    }
}

/// Factory: a project whose tools venv was provisioned by `toolr project
/// venv sync` (stub uv), with a `hello world` command whose interpreter
/// drops a sentinel when it runs.
fn synced_project() -> Project {
    let fx = VenvFixture::new();
    let sentinel = fx.root.join("ran");
    let cache = fx.root.join("xdg-cache");
    fs::write(
        fx.venv_dir.join("bin").join("python"),
        format!("#!/bin/sh\necho ran > {}\nexit 0\n", sentinel.display()),
    )
    .unwrap();
    fs::write(fx.tools_dir.join("hello.py"), HELLO_TOOLS).unwrap();
    let project = Project {
        fx,
        sentinel,
        cache,
    };
    project
        .toolr(&project.path_with_uv())
        .args(["project", "venv", "sync"])
        .assert()
        .success();
    project.reset_observations();
    project
}

fn uv_synced(log: &str) -> bool {
    log.lines().any(|token| token == "sync")
}

fn stamp_is_newer_than_lock(venv: &Path, tools: &Path) -> bool {
    let stamp = fs::metadata(venv.join(".toolr-sync-stamp"))
        .unwrap()
        .modified()
        .unwrap();
    let lock = fs::metadata(tools.join("uv.lock"))
        .unwrap()
        .modified()
        .unwrap();
    stamp >= lock
}

#[test]
fn fresh_venv_dispatches_without_running_uv() {
    let p = synced_project();
    p.toolr(&p.path_with_uv())
        .args(["hello", "world"])
        .assert()
        .success();
    assert!(p.sentinel.exists(), "the command should have run");
    assert_eq!(p.fx.uv_argv(), "", "a fresh venv must not invoke uv");
}

#[test]
fn stale_venv_is_synced_before_dispatch() {
    let p = synced_project();
    p.bump_lock();
    p.toolr(&p.path_with_uv())
        .args(["hello", "world"])
        .assert()
        .success();
    let log = p.fx.uv_argv();
    assert!(
        uv_synced(&log),
        "expected a `uv sync` before dispatch, uv argv was:\n{log}"
    );
    assert!(stamp_is_newer_than_lock(&p.fx.venv_dir, &p.fx.tools_dir));
    assert!(p.sentinel.exists(), "the command should run after the sync");
}

#[test]
fn pyproject_edit_resyncs_before_dispatch() {
    let p = synced_project();
    p.fx.add_unlocked_dependency();
    p.toolr(&p.path_with_uv())
        .args(["hello", "world"])
        .assert()
        .success();
    let log = p.fx.uv_argv();
    assert!(
        uv_synced(&log),
        "expected a `uv sync` before dispatch, uv argv was:\n{log}"
    );
    assert!(p.sentinel.exists(), "the command should run after the sync");
}

#[test]
fn venv_with_dead_interpreter_is_rebuilt_before_dispatch() {
    let p = synced_project();
    p.prune_interpreter();
    let output = p
        .toolr(&p.path_with_uv())
        .args(["hello", "world"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr:\n{stderr}");
    assert!(
        stderr.contains("interpreter no longer exists"),
        "stderr:\n{stderr}"
    );
    let log = p.fx.uv_argv();
    assert!(uv_synced(&log), "expected a `uv sync`, uv argv was:\n{log}");
    assert!(
        p.sentinel.exists(),
        "the command should run after the rebuild"
    );
}

#[test]
fn venv_sync_rebuilds_a_dead_interpreter() {
    let p = synced_project();
    p.prune_interpreter();
    p.toolr(&p.path_with_uv())
        .args(["project", "venv", "sync"])
        .assert()
        .success();
    let log = p.fx.uv_argv();
    assert!(uv_synced(&log), "expected a `uv sync`, uv argv was:\n{log}");
    assert!(p.fx.venv_dir.join("bin").join("python").is_file());
}

#[test]
fn stale_venv_without_uv_refuses_to_run() {
    let p = synced_project();
    p.bump_lock();
    let output = p
        .toolr(&p.fx.root.join("empty-path").display().to_string())
        .args(["--quiet", "hello", "world"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "a stale venv must not run; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains(&format!("out of date with {SYNC_INPUTS_DESCRIPTION}")),
        "stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("toolr project venv sync"),
        "stderr:\n{stderr}"
    );
    assert!(
        !p.sentinel.exists(),
        "the stale interpreter must not have run"
    );
}

fn install_toolr_py(p: &Project, version: &str) {
    let site_packages = p.fx.venv_dir.join("lib/python3.13/site-packages");
    fs::create_dir_all(site_packages.join(format!("toolr_py-{version}.dist-info"))).unwrap();
}

fn dispatch_stderr(p: &Project) -> String {
    let output = p
        .toolr(&p.path_with_uv())
        .args(["hello", "world"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "stderr:\n{stderr}");
    assert!(
        p.sentinel.exists(),
        "a version mismatch warns but still runs"
    );
    stderr
}

#[test]
fn older_venv_toolr_py_warns_to_upgrade_the_package() {
    let p = synced_project();
    install_toolr_py(&p, "0.0.1");
    let stderr = dispatch_stderr(&p);
    assert!(
        stderr.contains("toolr-py 0.0.1") && stderr.contains(env!("CARGO_PKG_VERSION")),
        "expected a warning naming both versions; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("toolr project venv sync -P toolr-py"),
        "a plain sync reinstalls the locked version; stderr:\n{stderr}"
    );
}

#[test]
fn newer_venv_toolr_py_warns_to_upgrade_the_binary() {
    let p = synced_project();
    install_toolr_py(&p, "999.0.0");
    let stderr = dispatch_stderr(&p);
    assert!(stderr.contains("toolr-py 999.0.0"), "stderr:\n{stderr}");
    assert!(
        stderr.contains("upgrade the toolr binary"),
        "stderr:\n{stderr}"
    );
    assert!(!stderr.contains("-P toolr-py"), "stderr:\n{stderr}");
}

#[test]
fn no_warning_when_venv_toolr_py_matches_binary() {
    let p = synced_project();
    install_toolr_py(&p, env!("CARGO_PKG_VERSION"));
    let output = p
        .toolr(&p.path_with_uv())
        .args(["hello", "world"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr:\n{stderr}");
    assert!(!stderr.contains("toolr-py"), "stderr:\n{stderr}");
}

#[test]
fn quiet_keeps_the_version_warning() {
    let p = synced_project();
    install_toolr_py(&p, "0.0.1");
    let output = p
        .toolr(&p.path_with_uv())
        .args(["--quiet", "hello", "world"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr:\n{stderr}");
    assert!(stderr.contains("toolr-py 0.0.1"), "stderr:\n{stderr}");
}

/// A trusted repository (real trust store, no `TOOLR_TRUST_ALL`) with an
/// in-tree venv toolr hasn't built. A venv uv builds runs a `python` that
/// drops `built`. [`Trusted::toolr`] approves replacing the venv with
/// `TOOLR_REPLACE_UNTRUSTED_VENV`; [`Trusted::unapproved`] doesn't.
struct Trusted {
    fx: VenvFixture,
    env: TrustEnv,
    built: std::path::PathBuf,
}

impl Trusted {
    fn new() -> Self {
        let fx = VenvFixture::new();
        let built = fx.root.join("built");
        fx.write_python(&format!(
            "#!/bin/sh\necho ran > {}\nexit 0\n",
            built.display()
        ));
        fs::write(fx.tools_dir.join("hello.py"), HELLO_TOOLS).unwrap();
        let env = TrustEnv::new();
        let t = Self { fx, env, built };
        t.toolr()
            .args(["project", "trust", "--no-sync"])
            .assert()
            .success();
        t
    }

    fn toolr(&self) -> Command {
        let mut cmd = self.unapproved();
        cmd.env(REPLACE_KEY, "1");
        cmd
    }

    fn unapproved(&self) -> Command {
        let mut cmd = self.env.toolr(&self.fx.root);
        cmd.env("PATH", &self.fx.bin_dir)
            .env("TOOLR_AUTO_INSTALL_UV", "1")
            .env_remove(REPLACE_KEY)
            .env_remove("TOOLR_AUTO_SYNC")
            .env_remove("TOOLR_VENV_LOCATION");
        cmd
    }

    /// Run unapproved on a pty pre-loaded with `answers`.
    fn on_tty(&self, args: &[&str], answers: &str) -> std::process::Output {
        let mut cmd = std::process::Command::new(assert_cmd::cargo::cargo_bin("toolr"));
        cmd.args(args)
            .current_dir(&self.fx.root)
            .env_remove("TOOLR_TRUST_ALL")
            .env_remove(REPLACE_KEY)
            .env_remove("TOOLR_AUTO_SYNC")
            .env_remove("TOOLR_VENV_LOCATION")
            .env("TOOLR_AUTO_INSTALL_UV", "1")
            .env("PATH", &self.fx.bin_dir)
            .env("XDG_STATE_HOME", self.env.state.path())
            .env("XDG_CACHE_HOME", self.env.cache.path())
            .env("XDG_DATA_HOME", self.env.data.path())
            .env("TOOLR_NO_CACHE_HINT", "1");
        common::run_on_pty(cmd, answers)
    }

    /// Plant an interpreter that drops `pwned` if it ever runs.
    fn plant(&self) -> std::path::PathBuf {
        let pwned = self.fx.root.join("pwned");
        common::write_executable(
            &self.fx.venv_dir.join("bin").join("python"),
            &format!("#!/bin/sh\necho pwned > {}\nexit 1\n", pwned.display()),
        );
        pwned
    }
}

const REPLACE_KEY: &str = "TOOLR_REPLACE_UNTRUSTED_VENV";
const REPLACE_PROMPT: &str = "Remove it and build a fresh one? [y/N]";
const KEPT: &str = "Nothing was removed";

/// Asserts the venv toolr didn't build was explained, kept and never run.
fn assert_kept(t: &Trusted, out: &std::process::Output, pwned: &Path) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "stderr:\n{stderr}");
    assert!(
        stderr.contains("so toolr won't run it")
            && stderr.contains(KEPT)
            && stderr.contains("toolr project venv sync")
            && stderr.contains(REPLACE_KEY),
        "stderr:\n{stderr}"
    );
    assert!(!pwned.exists(), "the planted interpreter ran");
    assert!(!t.fx.uv_ran_sync(), "uv argv:\n{}", t.fx.uv_argv());
    assert!(
        t.fx.venv_dir.join("bin/python").is_file(),
        "the venv must stay"
    );
}

#[test]
fn a_venv_toolr_didnt_build_stays_without_approval() {
    for args in [&["hello", "world"][..], &["project", "venv", "sync"][..]] {
        let t = Trusted::new();
        let pwned = t.plant();
        // `TOOLR_AUTO_INSTALL_UV` and `TOOLR_AUTO_SYNC` consent to syncing, never to removing.
        let out = t
            .unapproved()
            .env("TOOLR_AUTO_SYNC", "1")
            .args(args)
            .output()
            .unwrap();
        assert_kept(&t, &out, &pwned);
        for off in ["0", ""] {
            let out = t
                .unapproved()
                .env(REPLACE_KEY, off)
                .args(args)
                .output()
                .unwrap();
            assert_kept(&t, &out, &pwned);
        }
    }
}

/// The refusal, once, with nothing in front of it: no I/O-error wrapper, no repeated cause.
fn refusal(t: &Trusted) -> String {
    let venv = toolr_core::trust::shown(&fs::canonicalize(&t.fx.venv_dir).unwrap());
    format!(
        "toolr: toolr didn't build {venv} since you trusted this repository, so toolr won't run \
         it: it may have come with the repository, and running it would run code you never \
         approved. Nothing was removed. To replace it, run `toolr project venv sync` on a \
         terminal and answer y, set {REPLACE_KEY}=1, or remove {venv} yourself\n"
    )
}

#[test]
fn the_refusal_is_printed_once() {
    let t = Trusted::new();
    let out = t
        .unapproved()
        .args(["-q", "hello", "world"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stderr), refusal(&t));
}

/// The word isn't read from the venv to tell a typo from a plugin command, so clap's error
/// comes first and the venv's hint follows.
#[test]
fn a_typo_with_a_venv_toolr_didnt_build_gets_claps_error_then_the_hint() {
    let t = Trusted::new();
    let out = t.unapproved().arg("typo").output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "stderr:\n{stderr}");
    assert!(
        stderr.starts_with("error: unrecognized subcommand 'typo'"),
        "stderr:\n{stderr}"
    );
    assert!(stderr.ends_with(&refusal(&t)), "stderr:\n{stderr}");
    assert!(
        t.fx.venv_dir.join("bin/python").is_file(),
        "the venv must stay"
    );
}

/// With no venv path to resolve there is nothing to rebuild: clap's error stands alone.
#[test]
fn a_typo_with_an_unreadable_venv_setting_gets_only_claps_error() {
    let t = Trusted::new();
    let pyproject = t.fx.tools_dir.join("pyproject.toml");
    let body = fs::read_to_string(&pyproject)
        .unwrap()
        .replace("venv-location = \"in-tree\"", "venv-location = \"nowhere\"");
    fs::write(&pyproject, body).unwrap();
    let out = t.unapproved().arg("typo").output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "stderr:\n{stderr}");
    assert!(
        stderr.starts_with("error: unrecognized subcommand 'typo'")
            && !stderr.contains("toolr didn't build"),
        "stderr:\n{stderr}"
    );
}

/// uv creates the venv before it resolves, so a first sync that fails would leave a venv no
/// marker vouches for; toolr removes it, and the next sync builds one without asking.
#[test]
fn a_failed_first_sync_leaves_no_venv_to_ask_about() {
    let t = Trusted::new();
    fs::remove_dir_all(&t.fx.venv_dir).unwrap();
    t.fx.write_uv_stub("exit 3");
    let out = t
        .unapproved()
        .args(["project", "venv", "sync"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        fs::symlink_metadata(&t.fx.venv_dir).is_err(),
        "the venv uv created is removed"
    );
    t.fx.write_uv_stub("");
    let out = t
        .unapproved()
        .args(["project", "venv", "sync"])
        .write_stdin("")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr:\n{stderr}");
    assert!(!stderr.contains("toolr didn't build"), "stderr:\n{stderr}");
    t.unapproved().args(["hello", "world"]).assert().success();
    assert!(t.built.exists());
}

#[test]
fn a_venv_toolr_didnt_build_is_replaced_only_on_y() {
    let t = Trusted::new();
    let pwned = t.plant();
    for (answers, args) in [
        ("n\n", &["hello", "world"][..]),
        ("\x04", &["hello", "world"][..]),
        ("y\n", &["-q", "hello", "world"][..]),
    ] {
        let out = t.on_tty(args, answers);
        assert_kept(&t, &out, &pwned);
        let asked = String::from_utf8_lossy(&out.stderr).contains(REPLACE_PROMPT);
        assert_eq!(asked, args[0] != "-q", "{args:?}");
    }
    let out = t.on_tty(&["hello", "world"], "y\n");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr:\n{stderr}");
    assert_eq!(
        stderr.matches(REPLACE_PROMPT).count(),
        1,
        "stderr:\n{stderr}"
    );
    assert!(!pwned.exists(), "the planted interpreter ran");
    assert!(t.fx.uv_ran_sync());
    assert!(t.built.exists(), "the rebuilt venv should run the command");
}

#[test]
fn planted_in_tree_venv_is_rebuilt_not_run() {
    let t = Trusted::new();
    let pwned = t.fx.root.join("pwned");
    common::write_executable(
        &t.fx.venv_dir.join("bin").join("python"),
        &format!("#!/bin/sh\necho pwned > {}\nexit 1\n", pwned.display()),
    );
    fs::write(t.fx.venv_dir.join(".toolr-sync-stamp"), "").unwrap();
    let output = t.toolr().args(["hello", "world"]).output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr:\n{stderr}");
    assert!(!pwned.exists(), "the planted interpreter ran");
    assert!(
        stderr.contains("toolr: removing ")
            && stderr.contains(".venv: toolr didn't build this in-tree venv"),
        "stderr:\n{stderr}"
    );
    assert!(t.fx.uv_ran_sync(), "uv argv:\n{}", t.fx.uv_argv());
    assert!(t.built.exists(), "the rebuilt venv should run the command");
    let locks = t.env.state.path().join("toolr/trust/locks");
    let left: Vec<_> = fs::read_dir(&locks).unwrap().collect();
    assert!(
        left.is_empty(),
        "a finished sync leaves no lock file: {left:?}"
    );
}

#[test]
fn quiet_still_names_the_removed_venv() {
    let t = Trusted::new();
    let output = t
        .toolr()
        .args(["--quiet", "hello", "world"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr:\n{stderr}");
    assert!(
        stderr.contains("toolr didn't build this in-tree venv"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn own_in_tree_venv_is_reused() {
    let t = Trusted::new();
    t.toolr()
        .args(["project", "venv", "sync"])
        .assert()
        .success();
    assert!(t.fx.uv_ran_sync());
    fs::remove_file(&t.fx.uv_argv_log).unwrap();
    let output = t.toolr().args(["hello", "world"]).output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr:\n{stderr}");
    assert_eq!(t.fx.uv_argv(), "", "toolr's own venv must not be re-synced");
    assert!(!stderr.contains("removing"), "stderr:\n{stderr}");
    assert!(t.built.exists());
}

#[test]
fn own_stale_in_tree_venv_is_synced_in_place() {
    let t = Trusted::new();
    t.toolr()
        .args(["project", "venv", "sync"])
        .assert()
        .success();
    let marker = t.fx.venv_dir.join("keep-me");
    fs::write(&marker, "").unwrap();
    fs::remove_file(&t.fx.uv_argv_log).unwrap();
    // Age the sync stamp rather than sleep, so the bumped lock reads newer.
    fs::File::options()
        .write(true)
        .open(t.fx.venv_dir.join(".toolr-sync-stamp"))
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(60))
        .unwrap();
    fs::write(t.fx.tools_dir.join("uv.lock"), "version = 1\n# bumped\n").unwrap();
    let output = t.toolr().args(["hello", "world"]).output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr:\n{stderr}");
    assert!(t.fx.uv_ran_sync(), "uv argv:\n{}", t.fx.uv_argv());
    assert!(!stderr.contains("removing"), "stderr:\n{stderr}");
    assert!(
        marker.exists(),
        "a stale venv toolr built must not be rebuilt"
    );
    fs::remove_file(&t.fx.uv_argv_log).unwrap();
    t.toolr().args(["hello", "world"]).assert().success();
    assert_eq!(t.fx.uv_argv(), "", "the resync must leave a valid token");
}

#[test]
fn foreign_venv_is_refused_without_uv() {
    let t = Trusted::new();
    let output = t
        .toolr()
        .env("PATH", t.fx.root.join("empty-path"))
        .env_remove("TOOLR_AUTO_INSTALL_UV")
        .args(["--quiet", "hello", "world"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr:\n{stderr}");
    assert!(
        stderr.contains("so toolr won't run it") && stderr.contains("toolr project venv sync"),
        "stderr:\n{stderr}"
    );
    assert!(!t.built.exists(), "the foreign interpreter must not run");
}

#[test]
fn venv_run_no_sync_refuses_a_foreign_venv() {
    let t = Trusted::new();
    let output = t
        .toolr()
        .args(["project", "venv", "run", "--no-sync", "true"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr:\n{stderr}");
    assert!(
        stderr.contains("so toolr won't run it")
            && stderr.contains("run `toolr project venv sync`"),
        "stderr:\n{stderr}"
    );
    assert_eq!(t.fx.uv_argv(), "");
}

#[test]
fn forced_sync_removes_a_foreign_venv_first() {
    let t = Trusted::new();
    let planted = t.fx.venv_dir.join("planted");
    fs::write(&planted, "").unwrap();
    t.toolr()
        .args(["project", "venv", "sync", "--force"])
        .assert()
        .success();
    assert!(!planted.exists(), "uv ran against the planted venv");
    assert!(t.fx.uv_ran_sync());
}

fn help(t: &Trusted) -> String {
    let out = t.toolr().arg("--help").output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr:\n{stderr}");
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn a_foreign_venvs_plugins_stay_out_of_help_until_toolr_builds_it() {
    let t = Trusted::new();
    add_planted_plugin(&t.fx.venv_dir);
    add_planted_plugin(&t.fx.venv_template);
    let before = help(&t);
    assert!(before.contains("hello"), "help:\n{before}");
    assert!(!before.contains("planted"), "help:\n{before}");
    assert_eq!(t.fx.uv_argv(), "", "help must not rebuild the venv");
    t.toolr()
        .args(["project", "venv", "sync"])
        .assert()
        .success();
    let after = help(&t);
    assert!(after.contains("planted"), "help:\n{after}");
}

/// A plugin command is unknown until the foreign venv is rebuilt, so the rebuild runs before
/// clap, and dispatch doesn't sync a second time.
#[test]
fn a_plugin_command_rebuilds_a_foreign_venv_and_runs() {
    let t = Trusted::new();
    add_planted_plugin(&t.fx.venv_dir);
    add_planted_plugin(&t.fx.venv_template);
    let output = t.toolr().args(["planted", "run"]).output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr:\n{stderr}");
    assert!(
        stderr.contains(".venv: toolr didn't build this in-tree venv"),
        "stderr:\n{stderr}"
    );
    let syncs = t.fx.uv_argv().lines().filter(|w| *w == "sync").count();
    assert_eq!(syncs, 1, "uv argv:\n{}", t.fx.uv_argv());
    assert!(t.built.exists(), "the command should reach dispatch");
}

#[test]
fn a_plugin_command_whose_venv_cant_be_rebuilt_gets_claps_error_then_the_fix() {
    let t = Trusted::new();
    add_planted_plugin(&t.fx.venv_dir);
    let output = t
        .toolr()
        .env("PATH", t.fx.root.join("empty-path"))
        .env_remove("TOOLR_AUTO_INSTALL_UV")
        .args(["--quiet", "planted", "run"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr:\n{stderr}");
    assert!(
        stderr.starts_with("error: unrecognized subcommand 'planted'")
            && stderr.contains("so toolr won't run it")
            && stderr.contains("run `toolr project venv sync`"),
        "stderr:\n{stderr}"
    );
    assert!(
        !t.built.exists(),
        "the venv toolr didn't build must not run"
    );
}

#[test]
fn manifest_commands_never_read_a_foreign_venv() {
    let t = Trusted::new();
    add_planted_plugin(&t.fx.venv_dir);
    let hint = "so toolr won't run it";
    let out = t
        .toolr()
        .args(["self", "build-manifest", "planted"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "stderr:\n{stderr}");
    assert!(
        stderr.contains(hint) && stderr.contains("run `toolr project venv sync`"),
        "stderr:\n{stderr}"
    );
    assert!(!t.fx.tools_dir.join("planted").exists());

    let out = t
        .toolr()
        .args(["project", "manifest", "rebuild"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr:\n{stderr}");
    assert!(
        stderr.contains("leaving out its plugin commands") && stderr.contains(hint),
        "stderr:\n{stderr}"
    );
    let manifest = fs::read_to_string(t.fx.tools_dir.join(".toolr-manifest.json")).unwrap();
    assert!(!manifest.contains("planted"), "{manifest}");
}
