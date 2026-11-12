//! Untrusted-repository regression tests: toolr's read-only surfaces never
//! run a repository's code, and need no trust to work. The trust gate on
//! dispatch and venv commands is covered in `repo_trust.rs`.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use predicates::prelude::PredicateBooleanExt;
use tempfile::TempDir;

const GREET: &str = "\"\"\"Greetings.\"\"\"\nfrom toolr import command_group\ngroup = command_group(\"greet\", \"Greetings\", \"Greetings.\")\n@group.command\ndef hi(ctx):\n    \"\"\"Say hi.\"\"\"\n";

/// A repository with a real `greet` command whose fresh manifest also names a
/// `planted` one, so it is the manifest cache, not a rebuild, that would show it.
fn planted_repo() -> TempDir {
    let repo = TempDir::new().unwrap();
    let tools = common::write_project(repo.path());
    std::fs::write(tools.join("greet.py"), GREET).unwrap();
    common::build_manifest(repo.path());
    let path = tools.join(".toolr-manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    manifest["groups"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "name": "planted", "title": "Planted", "description": "Planted.", "origin": "static"
        }));
    manifest["commands"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "name": "go", "group": "planted", "module": "planted", "function": "go",
            "summary": "Planted.", "description": "", "arguments": [], "origin": "static"
        }));
    std::fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    repo
}

fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let rel = path.strip_prefix(root).unwrap().display().to_string();
                files.insert(rel, std::fs::read(&path).unwrap());
            }
        }
    }
    files
}

#[test]
fn help_hides_repository_commands_before_trust() {
    let repo = planted_repo();
    let env = common::TrustEnv::new();

    env.toolr(repo.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(
            predicates::str::contains("greet")
                .not()
                .and(predicates::str::contains("planted").not())
                .and(predicates::str::contains("project")),
        )
        .stderr(predicates::str::contains("toolr project trust"));

    // A trusted repository believes its fresh cache, planted entry included.
    common::toolr()
        .current_dir(repo.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("greet").and(predicates::str::contains("planted")));
}

#[test]
fn bare_toolr_hides_repository_commands_before_trust() {
    let repo = planted_repo();
    let env = common::TrustEnv::new();

    env.toolr(repo.path())
        .assert()
        .success()
        .stdout(
            predicates::str::contains("greet")
                .not()
                .and(predicates::str::contains("planted").not())
                .and(predicates::str::contains("project")),
        )
        .stderr(predicates::str::contains("toolr project trust"));
}

#[test]
fn tab_offers_only_builtins_before_trust() {
    let repo = planted_repo();
    let env = common::TrustEnv::new();

    env.toolr(repo.path())
        .arg("__complete")
        .arg(repo.path())
        .arg("")
        .assert()
        .success()
        .stdout(
            predicates::str::contains("greet")
                .not()
                .and(predicates::str::contains("planted").not()),
        )
        .stderr("");
}

#[test]
fn nothing_is_read_before_trust() {
    let repo = TempDir::new().unwrap();
    let tools = common::write_project(repo.path());
    std::fs::write(tools.join("broken.py"), "def (:\n").unwrap();
    std::fs::write(tools.join(".toolr-manifest.json"), "{ not json").unwrap();
    let env = common::TrustEnv::new();

    env.toolr(repo.path())
        .arg("--help")
        .assert()
        .success()
        .stderr(predicates::str::contains("warning").not());
}

#[test]
fn nothing_is_written_before_trust() {
    let repo = planted_repo();
    let env = common::TrustEnv::new();
    let before = snapshot(repo.path());

    env.toolr(repo.path()).arg("--help").assert().success();
    env.toolr(repo.path()).assert().success();
    env.toolr(repo.path())
        .arg("__complete")
        .arg(repo.path())
        .arg("")
        .assert()
        .success();
    env.toolr(repo.path())
        .args(["-q", "greet", "hi"])
        .assert()
        .failure();

    assert_eq!(snapshot(repo.path()), before);
}

#[test]
fn a_builtin_command_help_gets_no_hint() {
    let repo = planted_repo();
    let env = common::TrustEnv::new();

    for args in [["self", "--help"], ["project", "--help"]] {
        env.toolr(repo.path())
            .args(args)
            .assert()
            .success()
            .stderr("");
    }
    env.toolr(repo.path())
        .args(["project", "trust", "--help"])
        .assert()
        .success()
        .stderr("");
}

#[test]
fn quiet_hides_the_hint() {
    let repo = planted_repo();
    let env = common::TrustEnv::new();

    env.toolr(repo.path())
        .args(["-q", "--help"])
        .assert()
        .success()
        .stderr("");
}

#[test]
#[cfg(unix)]
fn help_does_not_execute_repository_code() {
    let out = TempDir::new().unwrap();
    let sentinel = out.path().join("sentinel");
    let repo = common::hostile_repo(&sentinel);
    let env = common::TrustEnv::new();

    env.toolr(repo.path()).arg("--help").assert().success();

    assert!(!sentinel.exists(), "toolr --help executed repository code");
}

#[test]
#[cfg(unix)]
fn bare_invocation_does_not_execute_repository_code() {
    let out = TempDir::new().unwrap();
    let sentinel = out.path().join("sentinel");
    let repo = common::hostile_repo(&sentinel);
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
        "\"\"\"Greetings.\"\"\"\nfrom toolr import command_group\ngroup = command_group(\"greet\", \"Greetings\", \"Greetings.\")\n@group.command\ndef hi(ctx):\n    \"\"\"Say hi.\"\"\"\n",
    )
    .unwrap();

    common::toolr()
        .arg("--help")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("greet"));
}

/// Bootstrap with a missing manifest but an existing in-tree venv globs the
/// venv's plugin fragments (`build_static_manifest_with_venv`), still
/// executing no Python.
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
        "\"\"\"Greetings.\"\"\"\nfrom toolr import command_group\ngroup = command_group(\"greet\", \"Greetings\", \"Greetings.\")\n@group.command\ndef hi(ctx):\n    \"\"\"Say hi.\"\"\"\n",
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
        "\"\"\"Greetings.\"\"\"\nfrom toolr import command_group\ngroup = command_group(\"greet\", \"Greetings\", \"Greetings.\")\n@group.command\ndef hi(ctx):\n    \"\"\"Say hi.\"\"\"\n",
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
        "\"\"\"Greetings.\"\"\"\nfrom toolr import command_group\ngroup = command_group(\"greet\", \"Greetings\", \"Greetings.\")\n@group.command\ndef hi(ctx):\n    \"\"\"Say hi.\"\"\"\n",
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
