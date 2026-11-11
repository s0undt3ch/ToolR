//! Trust first, then sync: `toolr project trust` syncs the tools venv, and a
//! command run that shows the trust prompt syncs a missing venv once the user
//! agrees. Prompts need a terminal on stdin, so those cases run under a pty.
#![cfg(unix)]

mod common;

use std::fs;
use std::path::PathBuf;
use std::process::Output;

use common::{TrustEnv, VenvFixture, add_planted_plugin};

const TRUST_PROMPT: &str = "Trust it? [y/N]";
const SYNC_PROMPT: &str = "Sync it now? [y/N]";
/// Asked once the sync is agreed: the never-synced in-tree venv wasn't built by toolr.
const REPLACE_PROMPT: &str = "Remove it and build a fresh one? [y/N]";
const KEPT: &str = "Nothing was removed";
const UV_PROMPT: &str = "didn't find it on PATH";
const HINT: &str = "run `toolr project venv sync` when you're ready";
const REFUSAL: &str = "refusing to run code from an untrusted repository";
const UV_INSTALL_FAILED: &str = "HTTP error during uv install";
const UNREACHABLE_PROXY: &str = "http://127.0.0.1:9";
const HELLO_TOOLS: &str = "\"\"\"Hi.\"\"\"\nfrom toolr import command_group\ngroup = command_group(\"hello\", \"Hi\", \"Hi.\")\n@group.command\ndef world(ctx):\n    \"\"\"World.\"\"\"\n";

/// A repository with a never-synced tools venv (no sync stamp), a stub uv on
/// `PATH` and a `hello world` command whose interpreter drops a sentinel.
struct Clone {
    fx: VenvFixture,
    env: TrustEnv,
    sentinel: PathBuf,
    venv_dir: PathBuf,
}

impl Clone {
    fn new() -> Self {
        let fx = VenvFixture::new();
        let sentinel = fx.root.join("ran");
        fx.write_python(&format!(
            "#!/bin/sh\necho ran > {}\nexit 0\n",
            sentinel.display()
        ));
        fs::write(fx.tools_dir.join("hello.py"), HELLO_TOOLS).unwrap();
        let venv_dir = fx.venv_dir.clone();
        Self {
            fx,
            env: TrustEnv::new(),
            sentinel,
            venv_dir,
        }
    }

    /// Sync once with trust bypassed, so the venv is fresh, then forget it. A
    /// cache venv: an in-tree one synced without a trust record is rebuilt
    /// once the repository is trusted.
    fn fresh() -> Self {
        let mut clone = Self::new();
        let pyproject = clone.fx.tools_dir.join("pyproject.toml");
        let body = fs::read_to_string(&pyproject)
            .unwrap()
            .replace("venv-location = \"in-tree\"", "venv-location = \"cache\"");
        fs::write(&pyproject, body).unwrap();
        clone
            .toolr()
            .env("TOOLR_TRUST_ALL", "1")
            .args(["project", "venv", "sync"])
            .assert()
            .success();
        let out = clone
            .toolr()
            .env("TOOLR_TRUST_ALL", "1")
            .args(["project", "venv", "path"])
            .output()
            .unwrap();
        clone.venv_dir = PathBuf::from(String::from_utf8(out.stdout).unwrap().trim());
        let _ = fs::remove_file(&clone.fx.uv_argv_log);
        clone
    }

    fn root(&self) -> String {
        fs::canonicalize(&self.fx.root)
            .unwrap()
            .display()
            .to_string()
    }

    fn toolr(&self) -> assert_cmd::Command {
        let mut cmd = self.env.toolr(&self.fx.root);
        cmd.env("PATH", &self.fx.bin_dir)
            .env_remove("TOOLR_AUTO_INSTALL_UV")
            .env_remove("TOOLR_AUTO_SYNC")
            .env_remove("TOOLR_VENV_LOCATION");
        cmd
    }

    /// Bump `tools/uv.lock` past the sync stamp, as a `git pull` would.
    fn make_stale(&self) {
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(self.fx.tools_dir.join("uv.lock"), "version = 1\n# bumped\n").unwrap();
    }

    /// Leave `bin/python` dangling as `mise prune` does; the stub uv's `sync`
    /// restores it, as real uv rebuilds such a venv.
    fn make_broken(&self) {
        let python = self.venv_dir.join("bin").join("python");
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

    /// Run on a pty with no uv anywhere, and any uv download refused at once
    /// by a proxy on a closed local port.
    fn run_without_uv(&self, args: &[&str], answers: &str) -> Output {
        let empty = tempfile::TempDir::new().unwrap();
        let empty = empty.path().to_str().unwrap().to_owned();
        self.run_with_tty_env(
            args,
            answers,
            &[
                ("PATH", &empty),
                ("HTTPS_PROXY", UNREACHABLE_PROXY),
                ("https_proxy", UNREACHABLE_PROXY),
                ("ALL_PROXY", UNREACHABLE_PROXY),
                ("NO_PROXY", ""),
                ("no_proxy", ""),
            ],
        )
    }

    fn is_trusted(&self) -> bool {
        let out = self.toolr().args(["self", "trusted"]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).contains(&self.root())
    }

    /// Run toolr with a pty as stdin, pre-loaded with `answers`.
    fn run_with_tty(&self, args: &[&str], answers: &str) -> Output {
        self.run_with_tty_env(args, answers, &[])
    }

    fn run_with_tty_env(&self, args: &[&str], answers: &str, env: &[(&str, &str)]) -> Output {
        let mut cmd = std::process::Command::new(assert_cmd::cargo::cargo_bin("toolr"));
        cmd.args(args)
            .current_dir(&self.fx.root)
            .env_remove("TOOLR_TRUST_ALL")
            .env_remove("TOOLR_AUTO_INSTALL_UV")
            .env_remove("TOOLR_AUTO_SYNC")
            .env_remove("TOOLR_VENV_LOCATION")
            .env("PATH", &self.fx.bin_dir)
            .env("XDG_STATE_HOME", self.env.state.path())
            .env("XDG_CACHE_HOME", self.env.cache.path())
            .env("XDG_DATA_HOME", self.env.data.path())
            .env("TOOLR_NO_CACHE_HINT", "1")
            .envs(env.iter().copied());
        common::run_on_pty(cmd, answers)
    }
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

// --- toolr project trust ---------------------------------------------------

#[test]
fn trust_then_yes_syncs_the_venv() {
    let c = Clone::new();
    let out = c.run_with_tty(&["project", "trust"], "y\ny\n");
    let err = stderr(&out);
    assert!(out.status.success(), "{err}");
    assert_eq!(stdout(&out), format!("{}\n", c.root()));
    assert!(err.contains(SYNC_PROMPT), "{err}");
    assert_eq!(err.matches(REPLACE_PROMPT).count(), 1, "{err}");
    assert!(!err.contains(UV_PROMPT), "{err}");
    assert!(c.fx.uv_ran_sync(), "the venv should have been synced");
    assert!(c.fx.venv_dir.join(".toolr-sync-stamp").is_file());
    assert!(c.is_trusted());
}

#[test]
fn trust_then_no_keeps_trust_and_hints() {
    let c = Clone::new();
    let out = c.run_with_tty(&["project", "trust"], "n\n");
    let err = stderr(&out);
    assert!(out.status.success(), "{err}");
    assert_eq!(stdout(&out), format!("{}\n", c.root()));
    assert!(err.contains(SYNC_PROMPT), "{err}");
    assert!(err.contains(HINT), "{err}");
    assert!(!c.fx.uv_ran_sync());
    assert!(c.is_trusted());
}

#[test]
fn trust_then_eof_declines() {
    let c = Clone::new();
    let out = c.run_with_tty(&["project", "trust"], "\x04");
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stderr(&out).contains(HINT));
    assert!(!c.fx.uv_ran_sync());
}

#[test]
fn trust_no_sync_asks_nothing() {
    let c = Clone::new();
    let out = c.run_with_tty(&["project", "trust", "--no-sync"], "y\n");
    let err = stderr(&out);
    assert!(out.status.success(), "{err}");
    assert_eq!(err, format!("toolr: trusted {}\n", c.root()));
    assert_eq!(stdout(&out), format!("{}\n", c.root()));
    assert!(!c.fx.uv_ran_sync());
    assert!(c.is_trusted());
}

#[test]
fn trust_quiet_on_a_terminal_declines_silently() {
    for args in [
        &["--quiet", "project", "trust"][..],
        &["-q", "project", "trust"][..],
    ] {
        let c = Clone::new();
        let out = c.run_with_tty(args, "y\n");
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        assert_eq!(stderr(&out), "", "{args:?}");
        assert_eq!(stdout(&out), format!("{}\n", c.root()), "{args:?}");
        assert!(!c.fx.uv_ran_sync(), "{args:?}");
        assert!(c.is_trusted());
    }
}

#[test]
fn trust_without_a_terminal_declines_with_hint() {
    let c = Clone::new();
    c.toolr()
        .args(["project", "trust"])
        .assert()
        .success()
        .stdout(format!("{}\n", c.root()))
        .stderr(predicates::str::contains(HINT));
    assert!(!c.fx.uv_ran_sync());
    assert!(c.is_trusted());
}

/// Consent to sync is not consent to remove a venv toolr didn't build.
#[test]
fn trust_without_a_terminal_and_auto_sync_keeps_a_venv_toolr_didnt_build() {
    let c = Clone::new();
    c.toolr()
        .env("TOOLR_AUTO_SYNC", "1")
        .env("TOOLR_AUTO_INSTALL_UV", "1")
        .args(["project", "trust"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(KEPT));
    assert!(!c.fx.uv_ran_sync());
    assert!(
        c.venv_dir.join("bin/python").is_file(),
        "the venv must stay"
    );
}

#[test]
fn trust_without_a_terminal_replaces_with_the_replace_key() {
    let c = Clone::new();
    c.toolr()
        .env("TOOLR_AUTO_SYNC", "1")
        .env("TOOLR_REPLACE_UNTRUSTED_VENV", "1")
        .args(["project", "trust"])
        .assert()
        .success()
        .stdout(format!("{}\n", c.root()));
    assert!(c.fx.uv_ran_sync());
}

#[test]
fn trust_with_a_fresh_venv_asks_nothing() {
    let c = Clone::fresh();
    let out = c.run_with_tty(&["project", "trust"], "");
    let err = stderr(&out);
    assert!(out.status.success(), "{err}");
    assert!(!err.contains(SYNC_PROMPT), "{err}");
    assert!(!c.fx.uv_ran_sync());
}

/// An in-tree venv toolr built after trust is fresh, so trusting again asks nothing.
#[test]
fn retrust_with_a_venv_toolr_built_asks_nothing() {
    let c = Clone::new();
    c.toolr()
        .env("TOOLR_AUTO_SYNC", "1")
        .env("TOOLR_REPLACE_UNTRUSTED_VENV", "1")
        .args(["project", "trust"])
        .assert()
        .success();
    let _ = fs::remove_file(&c.fx.uv_argv_log);
    let out = c.run_with_tty(&["project", "trust"], "");
    assert!(out.status.success(), "stderr:\n{}", stderr(&out));
    assert!(!stderr(&out).contains(SYNC_PROMPT), "{}", stderr(&out));
    assert!(!c.fx.uv_ran_sync());
}

#[test]
fn trust_reports_a_failed_sync() {
    let c = Clone::new();
    c.fx.write_uv_stub("exit 3");
    let out = c.run_with_tty(&["project", "trust"], "y\ny\n");
    let err = stderr(&out);
    assert!(!out.status.success(), "{err}");
    assert!(err.contains("syncing its tools venv failed"), "{err}");
    assert!(c.is_trusted(), "a failed sync keeps the trust");
}

#[test]
fn trust_then_yes_installs_uv_without_its_own_prompt() {
    let c = Clone::new();
    // A third line would answer a uv prompt, so its absence is checked, not hung on.
    let out = c.run_without_uv(&["project", "trust"], "y\nn\n");
    let err = stderr(&out);
    assert!(err.contains(SYNC_PROMPT), "{err}");
    assert!(!err.contains(UV_PROMPT), "asked about uv again:\n{err}");
    assert!(
        err.contains(UV_INSTALL_FAILED),
        "no uv install was attempted:\n{err}"
    );
    assert!(!out.status.success());
    assert!(c.is_trusted());
}

// --- dispatch --------------------------------------------------------------

#[test]
fn dispatch_trust_and_sync_then_runs() {
    let c = Clone::new();
    let out = c.run_with_tty(&["hello", "world"], "y\ny\ny\n");
    let err = stderr(&out);
    assert!(out.status.success(), "{err}");
    assert!(err.contains(TRUST_PROMPT), "{err}");
    assert_eq!(err.matches(SYNC_PROMPT).count(), 1, "{err}");
    assert_eq!(err.matches(REPLACE_PROMPT).count(), 1, "{err}");
    assert!(!err.contains(UV_PROMPT), "{err}");
    assert!(c.fx.uv_ran_sync());
    assert!(c.sentinel.exists(), "the command should have run");
}

/// The never-synced in-tree venv is foreign once trusted, so its plugin command is unknown
/// until the sync that follows the trust prompt rebuilds it.
#[test]
fn dispatch_trust_and_sync_then_runs_a_plugin_command() {
    let c = Clone::new();
    add_planted_plugin(&c.fx.venv_dir);
    add_planted_plugin(&c.fx.venv_template);
    let out = c.run_with_tty(&["planted", "run"], "y\ny\ny\n");
    let err = stderr(&out);
    assert!(out.status.success(), "{err}");
    assert!(err.contains(TRUST_PROMPT), "{err}");
    assert_eq!(err.matches(SYNC_PROMPT).count(), 1, "{err}");
    assert_eq!(err.matches(REPLACE_PROMPT).count(), 1, "{err}");
    assert!(c.sentinel.exists(), "the command should have run");
}

#[test]
fn dispatch_replace_declined_keeps_the_venv_and_explains() {
    let c = Clone::new();
    let out = c.run_with_tty(&["hello", "world"], "y\ny\nn\n");
    let err = stderr(&out);
    assert!(!out.status.success(), "{err}");
    assert!(err.contains(REPLACE_PROMPT), "{err}");
    assert!(
        err.contains("so toolr won't run it") && err.contains(KEPT),
        "{err}"
    );
    assert!(!c.fx.uv_ran_sync());
    assert!(
        !c.sentinel.exists(),
        "the venv toolr didn't build must not run"
    );
    assert!(
        c.venv_dir.join("bin/python").is_file(),
        "the venv must stay"
    );
}

#[test]
fn dispatch_sync_declined_stops_with_hint() {
    let c = Clone::new();
    let out = c.run_with_tty(&["hello", "world"], "y\nn\n");
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(2), "{err}");
    assert!(err.contains(HINT), "{err}");
    assert!(!c.fx.uv_ran_sync());
    assert!(!c.sentinel.exists(), "the command must not run");
    assert!(c.is_trusted(), "declining the sync keeps the trust");
}

#[test]
fn dispatch_trust_declined_syncs_nothing() {
    let c = Clone::new();
    let out = c.run_with_tty(&["hello", "world"], "n\n");
    let err = stderr(&out);
    assert!(!out.status.success());
    assert!(err.contains(TRUST_PROMPT), "{err}");
    assert!(err.contains(REFUSAL), "{err}");
    assert!(!err.contains(SYNC_PROMPT), "{err}");
    assert!(!c.fx.uv_ran_sync());
    assert!(!c.sentinel.exists());
}

#[test]
fn dispatch_with_a_fresh_venv_asks_only_for_trust() {
    let c = Clone::fresh();
    let out = c.run_with_tty(&["hello", "world"], "y\n");
    let err = stderr(&out);
    assert!(out.status.success(), "{err}");
    assert!(err.contains(TRUST_PROMPT), "{err}");
    assert!(!err.contains(SYNC_PROMPT), "{err}");
    assert!(!c.fx.uv_ran_sync());
    assert!(c.sentinel.exists());
}

#[test]
fn dispatch_auto_install_uv_answers_the_sync_prompt() {
    let c = Clone::new();
    let tty_out = c.run_with_tty_env(&["hello", "world"], "y\ny\n", &[("TOOLR_AUTO_SYNC", "1")]);
    let err = stderr(&tty_out);
    assert!(tty_out.status.success(), "{err}");
    assert!(!err.contains(SYNC_PROMPT), "{err}");
    assert!(c.fx.uv_ran_sync());
    assert!(c.sentinel.exists());
}

#[test]
fn dispatch_trust_all_leaves_a_missing_venv_alone() {
    let c = Clone::new();
    let out = c.run_with_tty_env(&["hello", "world"], "", &[("TOOLR_TRUST_ALL", "1")]);
    let err = stderr(&out);
    // The fixture's interpreter exists, so the never-synced venv runs as it is.
    assert!(out.status.success(), "{err}");
    assert!(c.sentinel.exists(), "the command should have run");
    assert!(!err.contains(TRUST_PROMPT), "{err}");
    assert!(!err.contains(SYNC_PROMPT), "{err}");
    assert!(!c.fx.uv_ran_sync());
}

#[test]
fn dispatch_without_a_terminal_syncs_nothing() {
    let c = Clone::new();
    c.toolr()
        .args(["hello", "world"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(REFUSAL))
        .stderr(predicates::str::contains("Run `toolr project trust`"));
    assert!(!c.fx.uv_ran_sync());
    assert!(!c.sentinel.exists());
}

#[test]
fn dispatch_trust_and_sync_install_uv_without_a_third_prompt() {
    let c = Clone::new();
    let out = c.run_without_uv(&["hello", "world"], "y\ny\nn\n");
    let err = stderr(&out);
    assert!(err.contains(TRUST_PROMPT), "{err}");
    assert!(err.contains(SYNC_PROMPT), "{err}");
    assert!(!err.contains(UV_PROMPT), "asked about uv again:\n{err}");
    assert!(
        err.contains(UV_INSTALL_FAILED),
        "no uv install was attempted:\n{err}"
    );
    assert!(!out.status.success());
    assert!(!c.sentinel.exists());
}

#[test]
fn dispatch_stale_venv_after_trust_gets_the_sync_prompt() {
    let c = Clone::fresh();
    c.make_stale();
    let out = c.run_with_tty(&["hello", "world"], "y\ny\n");
    let err = stderr(&out);
    assert!(out.status.success(), "{err}");
    assert!(err.contains(SYNC_PROMPT), "{err}");
    assert!(c.fx.uv_ran_sync());
    assert!(c.sentinel.exists());
}

#[test]
fn dispatch_broken_venv_after_trust_gets_the_sync_prompt() {
    let c = Clone::fresh();
    c.make_broken();
    let out = c.run_with_tty(&["hello", "world"], "y\ny\n");
    let err = stderr(&out);
    assert!(out.status.success(), "{err}");
    assert!(err.contains(SYNC_PROMPT), "{err}");
    assert!(c.fx.uv_ran_sync());
    assert!(c.sentinel.exists());
}

#[test]
fn dispatch_stale_venv_declined_after_trust_stops() {
    let c = Clone::fresh();
    c.make_stale();
    let out = c.run_with_tty(&["hello", "world"], "y\nn\n");
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains(HINT));
    assert!(!c.fx.uv_ran_sync());
    assert!(!c.sentinel.exists());
}

#[test]
fn already_trusted_stale_venv_resyncs_without_the_sync_prompt() {
    let c = Clone::fresh();
    c.toolr()
        .args(["project", "trust", "--no-sync"])
        .assert()
        .success();
    c.make_stale();
    let out = c.run_with_tty(&["hello", "world"], "");
    let err = stderr(&out);
    assert!(out.status.success(), "{err}");
    assert!(!err.contains(SYNC_PROMPT), "{err}");
    assert!(c.fx.uv_ran_sync());
}
