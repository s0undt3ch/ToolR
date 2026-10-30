//! Untrusted-repository regression tests: toolr's read-only surfaces never
//! run a repository's code, and need no trust to work. The trust gate on
//! dispatch and venv commands is covered in `repo_trust.rs`.

mod common;

use predicates::prelude::PredicateBooleanExt;
use tempfile::TempDir;

#[test]
#[cfg(unix)]
fn help_does_not_execute_repository_code() {
    let out = TempDir::new().unwrap();
    let sentinel = out.path().join("sentinel");
    let repo = common::hostile_repo(&sentinel, true);
    let env = common::TrustEnv::new();

    env.toolr(repo.path()).arg("--help").assert().success();

    assert!(!sentinel.exists(), "toolr --help executed repository code");
}

#[test]
#[cfg(unix)]
fn bare_invocation_does_not_execute_repository_code() {
    let out = TempDir::new().unwrap();
    let sentinel = out.path().join("sentinel");
    let repo = common::hostile_repo(&sentinel, true);
    let env = common::TrustEnv::new();

    env.toolr(repo.path()).assert().success();

    assert!(!sentinel.exists(), "bare toolr executed repository code");
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

    common::toolr()
        .arg("--help")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("greet"));
}

/// Bootstrap with a missing manifest but an existing in-tree venv takes the
/// static-build-WITH-venv arm (`build_static_manifest_with_venv`), still
/// executing no Python. Covers the venv-present branch of
/// `ensure_manifest_present_or_bootstrap`.
#[test]
fn help_builds_statically_with_an_existing_in_tree_venv() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    std::fs::create_dir_all(tools.join(".venv")).unwrap();
    std::fs::write(
        tools.join("pyproject.toml"),
        "[project]\nname=\"demo\"\nversion=\"0\"\n\n[tool.toolr]\nvenv-location = \"in-tree\"\n",
    )
    .unwrap();
    std::fs::write(
        tools.join("greet.py"),
        "\"\"\"Greetings.\"\"\"\nfrom toolr import command_group\ngroup = command_group(\"greet\", \"Greetings\")\n@group.command\ndef hi(ctx):\n    \"\"\"Say hi.\"\"\"\n",
    )
    .unwrap();
    // An existing venv (only `pyvenv.cfg` needed) so bootstrap takes the
    // `build_static_manifest_with_venv` arm; no manifest committed.
    std::fs::write(tools.join(".venv").join("pyvenv.cfg"), "home = /usr\n").unwrap();

    common::toolr()
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
    // Cache-located venv (default); this
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
    common::toolr()
        .arg("--help")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("greet"))
        .stdout(predicates::str::contains("plugins").not());

    // A venv appears with a third-party fragment declaring a command.
    let venv = tools.join(".venv");
    let sp = venv
        .join("lib")
        .join("python3.13")
        .join("site-packages")
        .join("demo_plugin");
    std::fs::create_dir_all(&sp).unwrap();
    std::fs::write(venv.join("pyvenv.cfg"), "home = /usr\n").unwrap();
    std::fs::write(
        sp.join("toolr-manifest.json"),
        r#"{"toolr_schema_version":2,"package":"demo_plugin",
            "groups":[{"name":"plugins","title":"Plugins","description":"From a plugin.",
                "origin":"third_party"}],
            "commands":[{"name":"from-plugin","group":"plugins","module":"demo_plugin.commands",
                "function":"from_plugin","summary":"From a plugin.","description":"",
                "arguments":[],"origin":"third_party"}]}"#,
    )
    .unwrap();

    // Force the resolved venv to be this in-tree one so the freshness
    // check globs the fragment we just dropped. Root `--help` lists
    // groups, so the plugin's `plugins` group must now appear.
    common::toolr()
        .arg("--help")
        .current_dir(tmp.path())
        .env("TOOLR_VENV_LOCATION", "in-tree")
        .assert()
        .success()
        .stdout(predicates::str::contains("plugins"));

    // The rebuilt manifest now carries the plugin's leaf command.
    let manifest = std::fs::read_to_string(tools.join(".toolr-manifest.json")).unwrap();
    assert!(
        manifest.contains("from-plugin"),
        "manifest should include the third-party command after the venv appeared:\n{manifest}"
    );
}

/// A v1 plugin fragment is skipped, not loaded, and the skip is warned about.
#[test]
fn v1_plugin_is_skipped_with_a_warning() {
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
    let venv = tools.join(".venv");
    let sp = venv
        .join("lib")
        .join("python3.13")
        .join("site-packages")
        .join("demo_plugin");
    std::fs::create_dir_all(&sp).unwrap();
    std::fs::write(venv.join("pyvenv.cfg"), "home = /usr\n").unwrap();
    std::fs::write(
        sp.join("toolr-manifest.json"),
        r#"{"toolr_schema_version":1,"package":"demo_plugin",
            "groups":[{"name":"plugins","title":"Plugins","description":"From a plugin.",
                "origin":"third_party"}],
            "commands":[{"name":"from-plugin","group":"plugins","module":"demo_plugin.commands",
                "function":"from_plugin","summary":"From a plugin.","description":"",
                "arguments":[],"origin":"third_party"}]}"#,
    )
    .unwrap();

    common::toolr()
        .arg("--help")
        .current_dir(tmp.path())
        .env("TOOLR_VENV_LOCATION", "in-tree")
        .assert()
        .success()
        .stdout(predicates::str::contains("greet"))
        .stdout(predicates::str::contains("plugins").not())
        .stderr(predicates::str::contains(
            "toolr: warning: skipping plugin demo_plugin",
        ));
}
