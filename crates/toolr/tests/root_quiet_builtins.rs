//! The root `--quiet` / `-q` silences the success lines and advice of the
//! `self` and `project` built-ins, and never an error, refusal or warning.
//! A command's result (the `self trusted` listing) stays.

#![cfg(unix)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use common::{Project, TrustEnv, VenvFixture};
use tempfile::TempDir;

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn assert_silent_success(out: &Output) {
    assert!(out.status.success(), "{out:?}");
    assert_eq!(stdout(out), "", "stdout");
    assert_eq!(stderr(out), "", "stderr");
}

fn uv_got_quiet(fx: &VenvFixture) -> bool {
    fx.uv_argv().lines().any(|l| l == "--quiet")
}

/// `toolr` in the fixture's root with the stub uv as the only thing on `PATH`.
fn venv_toolr(fx: &VenvFixture, args: &[&str]) -> Output {
    common::toolr()
        .env("PATH", &fx.bin_dir)
        .current_dir(&fx.root)
        .args(args)
        .output()
        .unwrap()
}

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A one-command plugin package at `<tmp>/<name>/`.
fn minimal_plugin(tmp: &Path, name: &str) -> PathBuf {
    let pkg = tmp.join(name);
    write(&pkg.join("__init__.py"), "");
    write(
        &pkg.join("commands.py"),
        "from toolr import Context, command_group\n\ng = command_group(\"g\", \"G\", \"G.\")\n\n\
         @g.command\ndef hi(ctx: Context) -> None:\n    \"\"\"Hi.\"\"\"\n",
    );
    pkg
}

fn self_build_manifest(pkg: &Path, root_flag: Option<&str>, extra: &[&str]) -> Output {
    common::toolr()
        .args(root_flag)
        .args(["self", "build-manifest", "--source-dir"])
        .arg(pkg)
        .args(["--package", "mypkg"])
        .args(extra)
        .output()
        .unwrap()
}

#[test]
fn build_manifest_prints_its_wrote_line_without_the_flag() {
    let tmp = TempDir::new().unwrap();
    let pkg = minimal_plugin(tmp.path(), "mypkg");
    let out = self_build_manifest(&pkg, None, &[]);
    assert!(stderr(&out).contains("wrote 1 group(s) / 1 command(s)"));
}

#[test]
fn build_manifest_is_silent_under_root_quiet() {
    for flag in ["--quiet", "-q"] {
        let tmp = TempDir::new().unwrap();
        let pkg = minimal_plugin(tmp.path(), "mypkg");
        assert_silent_success(&self_build_manifest(&pkg, Some(flag), &[]));
        assert!(pkg.join("toolr-manifest.json").is_file());
    }
}

#[test]
fn build_manifest_quiet_still_prints_warnings_and_stale_check() {
    let tmp = TempDir::new().unwrap();
    let pkg = minimal_plugin(tmp.path(), "mypkg");
    write(
        &pkg.join("other.py"),
        "from toolr import command_group\n\ncommand_group(\"g\", \"Other\")\n",
    );
    let out = self_build_manifest(&pkg, Some("--quiet"), &[]);
    assert!(out.status.success(), "{out:?}");
    assert!(stderr(&out).contains("toolr: warning: group `g` is declared more than once"));
    assert!(!stderr(&out).contains("wrote"));

    fs::write(pkg.join("toolr-manifest.json"), "{}").unwrap();
    let out = self_build_manifest(&pkg, Some("--quiet"), &["--check"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(stderr(&out).contains("is out of date"));
}

/// A store holding one live root and one that no longer exists.
fn store_with_live_and_missing() -> (TrustEnv, TempDir, String, String) {
    let env = TrustEnv::new();
    let live = TempDir::new().unwrap();
    let gone = TempDir::new().unwrap();
    for repo in [&live, &gone] {
        common::write_project(repo.path());
        env.toolr(repo.path())
            .args(["project", "trust", "--no-sync"])
            .assert()
            .success();
    }
    let live_root = fs::canonicalize(live.path()).unwrap().display().to_string();
    let gone_root = fs::canonicalize(gone.path()).unwrap().display().to_string();
    gone.close().unwrap();
    (env, live, live_root, gone_root)
}

#[test]
fn trusted_prune_reports_the_pruned_root_without_the_flag() {
    let (env, live, _live_root, gone_root) = store_with_live_and_missing();
    let out = env
        .toolr(live.path())
        .args(["self", "trusted", "--prune"])
        .output()
        .unwrap();
    assert_eq!(stderr(&out), format!("toolr: pruned {gone_root}\n"));
}

#[test]
fn trusted_prune_quiet_drops_the_pruned_line_and_keeps_the_listing() {
    for flag in ["--quiet", "-q"] {
        let (env, live, live_root, gone_root) = store_with_live_and_missing();
        let out = env
            .toolr(live.path())
            .args([flag, "self", "trusted", "--prune"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        assert_eq!(stdout(&out), format!("{live_root}\n"));
        assert_eq!(stderr(&out), "");
        // The prune itself still happened.
        let out = env
            .toolr(live.path())
            .args(["self", "trusted"])
            .output()
            .unwrap();
        assert!(!stdout(&out).contains(&gone_root), "{out:?}");
    }
}

#[test]
fn trusted_quiet_without_prune_keeps_the_listing_and_missing_marker() {
    let (env, live, live_root, gone_root) = store_with_live_and_missing();
    let out = env
        .toolr(live.path())
        .args(["--quiet", "self", "trusted"])
        .output()
        .unwrap();
    let listing = stdout(&out);
    assert!(listing.contains(&format!("{live_root}\n")), "{listing}");
    assert!(
        listing.contains(&format!("{gone_root} (missing)\n")),
        "{listing}"
    );
}

#[test]
fn manifest_rebuild_prints_its_wrote_line_without_the_flag() {
    let p = Project::new();
    assert!(
        p.stdout(&["project", "manifest", "rebuild"])
            .contains("wrote 1 groups / 1 commands")
    );
}

#[test]
fn manifest_rebuild_is_silent_under_root_quiet() {
    let p = Project::new();
    let args = ["-q", "project", "manifest", "rebuild"];
    assert_silent_success(&p.toolr(&args).output().unwrap());
    assert!(p.tools().join(".toolr-manifest.json").is_file());
}

#[test]
fn manifest_rebuild_quiet_still_prints_plugin_warnings() {
    let p = Project::new();
    p.add_plugin(
        "demo_plugin",
        // A v1 fragment is skipped with a warning, so the rebuild still succeeds.
        r#"{"toolr_schema_version":1,"package":"demo_plugin","groups":[],"commands":[]}"#,
    );
    let out = p
        .toolr(&["--quiet", "project", "manifest", "rebuild"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(
        stderr(&out).contains("toolr: warning: skipping plugin demo_plugin"),
        "{}",
        stderr(&out)
    );
    assert_eq!(stdout(&out), "");
}

/// A stub `uv sync` that leaves behind a venv `validate_venv` accepts.
fn init_project(root_flags: &[&str], init_flags: &[&str]) -> (VenvFixture, TempDir, Output) {
    let fx = VenvFixture::new();
    let cwd = TempDir::new().unwrap();
    let venv = cwd.path().join("tools/.venv");
    let sp = venv.join("lib/python3.13/site-packages/toolr");
    let py = venv.join("bin/python");
    fx.write_uv_stub(&format!(
        "/bin/mkdir -p {sp} {bin}; : > {sp}/__init__.py; \
         printf '#!/bin/sh\\nexit 0\\n' > {py}; /bin/chmod +x {py}",
        sp = sp.display(),
        bin = venv.join("bin").display(),
        py = py.display(),
    ));
    let out = common::toolr()
        .env("PATH", &fx.bin_dir)
        .current_dir(cwd.path())
        .args(root_flags)
        .args([
            "project",
            "init",
            "--venv-location",
            "in-tree",
            "--python",
            ">=3.11",
        ])
        .args(init_flags)
        .output()
        .unwrap();
    (fx, cwd, out)
}

#[test]
fn init_prints_progress_and_next_steps_without_the_flag() {
    let (_fx, _cwd, out) = init_project(&[], &[]);
    assert!(out.status.success(), "{out:?}");
    assert!(stdout(&out).contains("synced venv"), "{}", stdout(&out));
    assert!(stdout(&out).contains("next steps"));
}

#[test]
fn init_is_silent_under_root_quiet_and_quiets_uv() {
    for flag in ["--quiet", "-q"] {
        let (fx, cwd, out) = init_project(&[flag], &[]);
        assert_silent_success(&out);
        assert!(cwd.path().join("tools/pyproject.toml").is_file());
        assert!(fx.uv_ran_sync());
        assert!(uv_got_quiet(&fx), "{}", fx.uv_argv());
    }
}

#[test]
fn init_no_sync_is_silent_under_root_quiet() {
    let cwd = TempDir::new().unwrap();
    let out = common::toolr()
        .current_dir(cwd.path())
        .args(["-q", "project", "init", "--no-sync"])
        .output()
        .unwrap();
    assert_silent_success(&out);
    assert!(cwd.path().join("tools/example.py").is_file());
}

#[test]
fn init_quiet_still_reports_conflicts() {
    let cwd = TempDir::new().unwrap();
    write(&cwd.path().join("tools/pyproject.toml"), "# mine\n");
    let out = common::toolr()
        .current_dir(cwd.path())
        .args(["-q", "project", "init", "--no-sync"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(stderr(&out).contains("toolr: error:"), "{}", stderr(&out));
}

#[test]
fn venv_sync_is_silent_under_root_quiet_and_quiets_uv() {
    for flag in ["--quiet", "-q"] {
        let fx = VenvFixture::new();
        assert_silent_success(&venv_toolr(
            &fx,
            &[flag, "project", "venv", "sync", "--force"],
        ));
        assert!(fx.uv_ran_sync());
        assert!(uv_got_quiet(&fx), "{}", fx.uv_argv());
    }
}

#[test]
fn venv_sync_prints_its_lines_without_the_flag() {
    let fx = VenvFixture::new();
    let out = venv_toolr(&fx, &["project", "venv", "sync", "--force"]);
    assert!(stdout(&out).contains("synced venv"), "{out:?}");
    assert!(!uv_got_quiet(&fx));
}

#[test]
fn venv_lock_add_remove_are_silent_under_root_quiet_and_quiet_uv() {
    for args in [
        &["-q", "project", "venv", "lock"][..],
        &["-q", "project", "venv", "add", "rich"][..],
        &["-q", "project", "venv", "remove", "requests"][..],
    ] {
        let fx = VenvFixture::new();
        assert_silent_success(&venv_toolr(&fx, args));
        assert!(uv_got_quiet(&fx), "{args:?}: {}", fx.uv_argv());
    }
}

#[test]
fn venv_remove_quiet_still_reports_an_undeclared_package() {
    let fx = VenvFixture::new();
    let out = venv_toolr(&fx, &["-q", "project", "venv", "remove", "nope"]);
    assert!(!out.status.success(), "{out:?}");
    assert!(stderr(&out).contains("is not declared"), "{}", stderr(&out));
}

/// `-q` is output control only: "not a toolr repo" still fails loudly. The silent
/// success for the mise hook is `venv sync --if-project`.
#[test]
fn venv_sync_root_quiet_does_not_swallow_a_missing_project() {
    let tmp = TempDir::new().unwrap();
    let out = common::toolr()
        .current_dir(tmp.path())
        .args(["-q", "project", "venv", "sync"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "{out:?}");
    assert!(!stderr(&out).is_empty());
}

/// The fixture's venv has no sync stamp, so both commands auto-sync first.
#[test]
fn venv_shell_and_run_quiet_the_auto_sync_uv() {
    for args in [
        &["-q", "project", "venv", "shell"][..],
        &["-q", "project", "venv", "run", "--", "/usr/bin/true"][..],
        &["--quiet", "project", "venv", "run", "--", "/usr/bin/true"][..],
    ] {
        let fx = VenvFixture::new();
        let out = common::toolr()
            .env("PATH", &fx.bin_dir)
            .env("SHELL", "/usr/bin/true")
            .current_dir(&fx.root)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {out:?}");
        assert!(fx.uv_ran_sync(), "{args:?}");
        assert!(uv_got_quiet(&fx), "{args:?}: {}", fx.uv_argv());
    }
}

#[test]
fn venv_run_without_the_flag_does_not_quiet_uv() {
    let fx = VenvFixture::new();
    let out = venv_toolr(&fx, &["project", "venv", "run", "--", "/usr/bin/true"]);
    assert!(out.status.success(), "{out:?}");
    assert!(fx.uv_ran_sync() && !uv_got_quiet(&fx));
}
