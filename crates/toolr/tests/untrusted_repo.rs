//! Untrusted-repository regression tests for SEC-01.
//! A repo must not be able to run code via toolr's read-only surfaces.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use tempfile::TempDir;

/// Build a malicious repo: in-tree venv-location, a committed fake
/// `tools/.venv/bin/python` that drops a sentinel when executed, and NO
/// `.toolr-manifest.json`.
fn malicious_repo(sentinel: &std::path::Path) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir_all(tools.join(".venv").join("bin")).unwrap();
    fs::write(
        tools.join("pyproject.toml"),
        "[project]\nname=\"evil\"\nversion=\"0\"\n\n[tool.toolr]\nvenv-location = \"in-tree\"\n",
    )
    .unwrap();
    fs::write(tools.join("hello.py"), "\"\"\"Hi.\"\"\"\n").unwrap();
    let py = tools.join(".venv").join("bin").join("python");
    fs::write(&py, format!("#!/bin/sh\necho pwned > {}\n", sentinel.display())).unwrap();
    fs::set_permissions(&py, fs::Permissions::from_mode(0o755)).unwrap();
    tmp
}

#[test]
#[cfg(unix)]
fn help_does_not_execute_committed_interpreter() {
    let out = TempDir::new().unwrap();
    let sentinel = out.path().join("sentinel");
    let repo = malicious_repo(&sentinel);

    Command::cargo_bin("toolr")
        .unwrap()
        .arg("--help")
        .current_dir(repo.path())
        .assert()
        .success();

    assert!(!sentinel.exists(), "toolr --help executed the committed interpreter");
}

#[test]
#[cfg(unix)]
fn bare_invocation_does_not_execute_committed_interpreter() {
    let out = TempDir::new().unwrap();
    let sentinel = out.path().join("sentinel");
    let repo = malicious_repo(&sentinel);

    // Bare `toolr` exits non-zero (no command) but must not run the interpreter.
    let _ = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(repo.path())
        .assert();

    assert!(!sentinel.exists(), "bare toolr executed the committed interpreter");
}

#[test]
fn help_works_with_no_venv_and_shows_first_party_commands() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    std::fs::write(
        tools.join("pyproject.toml"),
        "[project]\nname=\"demo\"\nversion=\"0\"\n",
    )
    .unwrap();
    std::fs::write(
        tools.join("greet.py"),
        "\"\"\"Greetings.\"\"\"\nfrom toolr import command_group\ngroup = command_group(\"greet\", \"Greetings\")\n@group.command\ndef hi(ctx):\n    \"\"\"Say hi.\"\"\"\n",
    )
    .unwrap();

    Command::cargo_bin("toolr")
        .unwrap()
        .arg("--help")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("greet"));
}

/// Manifest-as-cache: a venv appearing after the first static-only build
/// is detected as third-party drift, and the next read-only invocation
/// rebuilds the manifest to include the plugin's commands — no manual
/// rebuild, no Python execution.
#[test]
fn manifest_rebuilds_when_a_venv_appears() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    // Cache-located venv (default) so no provenance gate is involved; this
    // test only exercises the read-only `--help` freshness path.
    std::fs::write(
        tools.join("pyproject.toml"),
        "[project]\nname=\"demo\"\nversion=\"0\"\n",
    )
    .unwrap();
    std::fs::write(
        tools.join("greet.py"),
        "\"\"\"Greetings.\"\"\"\nfrom toolr import command_group\ngroup = command_group(\"greet\", \"Greetings\")\n@group.command\ndef hi(ctx):\n    \"\"\"Say hi.\"\"\"\n",
    )
    .unwrap();

    // First `--help`: no venv → static-only manifest with empty
    // third-party hash. The plugin group is absent.
    Command::cargo_bin("toolr")
        .unwrap()
        .arg("--help")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("greet"))
        .stdout(predicates::str::contains("plugins").not());

    // A venv appears with a third-party fragment declaring a command.
    let venv = tools.join(".venv");
    let sp = venv.join("lib").join("python3.13").join("site-packages").join("demo_plugin");
    std::fs::create_dir_all(&sp).unwrap();
    std::fs::write(venv.join("pyvenv.cfg"), "home = /usr\n").unwrap();
    std::fs::write(
        sp.join("toolr-manifest.json"),
        r#"{"toolr_schema_version":1,"package":"demo_plugin",
            "groups":[{"name":"plugins","title":"Plugins","description":"From a plugin."}],
            "commands":[{"name":"from-plugin","group":"plugins","module":"demo_plugin.commands",
                "function":"from_plugin","summary":"From a plugin.","description":"",
                "arguments":[],"imports":[]}]}"#,
    )
    .unwrap();

    // Force the resolved venv to be this in-tree one so the freshness
    // check globs the fragment we just dropped. Root `--help` lists
    // groups, so the plugin's `plugins` group must now appear.
    Command::cargo_bin("toolr")
        .unwrap()
        .arg("--help")
        .current_dir(tmp.path())
        .env("TOOLR_VENV_LOCATION", "in-tree")
        .assert()
        .success()
        .stdout(predicates::str::contains("plugins"));

    // The rebuilt manifest now carries the plugin's leaf command.
    let manifest =
        std::fs::read_to_string(tools.join(".toolr-manifest.json")).unwrap();
    assert!(
        manifest.contains("from-plugin"),
        "manifest should include the third-party command after the venv appeared:\n{manifest}"
    );
}

#[test]
#[cfg(unix)]
fn dispatch_refuses_committed_interpreter() {
    let out = TempDir::new().unwrap();
    let sentinel = out.path().join("sentinel");
    let repo = malicious_repo(&sentinel);
    // give it a statically-parseable command so dispatch is attempted
    std::fs::write(
        repo.path().join("tools").join("hello.py"),
        "\"\"\"Hi.\"\"\"\nfrom toolr import command_group\ngroup = command_group(\"hello\", \"Hi\")\n@group.command\ndef world(ctx):\n    \"\"\"World.\"\"\"\n",
    )
    .unwrap();

    Command::cargo_bin("toolr")
        .unwrap()
        .args(["hello", "world"])
        .current_dir(repo.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("not provisioned by toolr"));
    assert!(!sentinel.exists());
}
