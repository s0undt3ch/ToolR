//! Binary-level tests for plugin warnings: skipped plugins, shadowed plugin
//! commands, nested groups and the freshness behaviour around them.

use std::path::PathBuf;

use assert_cmd::Command;
use tempfile::TempDir;

const GREET: &str = "\"\"\"Greetings.\"\"\"\nfrom toolr import command_group\ngroup = command_group(\"greet\", \"Greetings\")\n@group.command\ndef hi(ctx):\n    \"\"\"Say hi.\"\"\"\n";

const LOCAL_CI_LINT: &str = "\"\"\"CI.\"\"\"\nfrom toolr import command_group\nci = command_group(\"ci\", \"CI\")\n@ci.command\ndef lint(ctx):\n    \"\"\"Local lint.\"\"\"\n";

const LOCAL_CI_IMAGE: &str = "\"\"\"CI.\"\"\"\nfrom toolr import command_group\nci = command_group(\"ci\", \"CI\")\nimage = ci.command_group(\"image\", \"Image\")\n@image.command\ndef build(ctx):\n    \"\"\"Local build.\"\"\"\n";

struct Project {
    tmp: TempDir,
}

impl Project {
    /// A project with one local group and an (initially empty) in-tree venv.
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::write(
            tools.join("pyproject.toml"),
            "[project]\nname=\"demo\"\nversion=\"0\"\n",
        )
        .unwrap();
        std::fs::write(tools.join("greet.py"), GREET).unwrap();
        let venv = tools.join(".venv");
        std::fs::create_dir_all(&venv).unwrap();
        std::fs::write(venv.join("pyvenv.cfg"), "home = /usr\n").unwrap();
        Self { tmp }
    }

    fn tools(&self) -> PathBuf {
        self.tmp.path().join("tools")
    }

    fn write_tool(&self, name: &str, body: &str) {
        std::fs::write(self.tools().join(name), body).unwrap();
    }

    fn add_plugin(&self, package: &str, fragment: &str) {
        let dir = self
            .tools()
            .join(".venv/lib/python3.13/site-packages")
            .join(package);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("toolr-manifest.json"), fragment).unwrap();
    }

    fn toolr(&self, args: &[&str]) -> Command {
        let mut cmd = Command::cargo_bin("toolr").unwrap();
        cmd.args(args)
            .current_dir(self.tmp.path())
            .env("TOOLR_VENV_LOCATION", "in-tree")
            .env("TOOLR_NO_CACHE_HINT", "1");
        cmd
    }

    fn stderr(&self, args: &[&str]) -> String {
        let out = self.toolr(args).output().unwrap();
        assert!(
            out.status.success(),
            "toolr {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stderr).unwrap()
    }

    fn stdout(&self, args: &[&str]) -> String {
        let out = self.toolr(args).output().unwrap();
        assert!(out.status.success(), "toolr {args:?} failed");
        String::from_utf8(out.stdout).unwrap()
    }

    fn manifest(&self) -> String {
        std::fs::read_to_string(self.tools().join(".toolr-manifest.json")).unwrap()
    }
}

fn fragment(schema: u32, package: &str, group: &str, parent: Option<&str>, cmd: &str) -> String {
    let parent = parent
        .map(|p| format!(r#","parent":"{p}""#))
        .unwrap_or_default();
    let group_name = group.rsplit('.').next().unwrap();
    format!(
        r#"{{"toolr_schema_version":{schema},"package":"{package}",
            "groups":[{{"name":"{group_name}","title":"T","description":"D",
                "origin":"third_party"{parent}}}],
            "commands":[{{"name":"{cmd}","group":"{group}","module":"{package}.commands",
                "function":"{cmd}_fn","summary":"S","description":"",
                "arguments":[],"origin":"third_party"}}]}}"#
    )
}

/// A v1 `demo_plugin` next to a good v2 `good_plugin`.
fn project_with_skipped_plugin() -> Project {
    let p = Project::new();
    p.add_plugin(
        "demo_plugin",
        &fragment(1, "demo_plugin", "oldplugin", None, "run"),
    );
    p.add_plugin(
        "good_plugin",
        &fragment(2, "good_plugin", "good", None, "run"),
    );
    p
}

const SKIP_WARNING: &str = "toolr: warning: skipping plugin demo_plugin";

#[test]
fn skipped_plugin_warns_and_the_rest_still_works() {
    let p = project_with_skipped_plugin();
    let out = p.toolr(&["--help"]).output().unwrap();
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stderr.contains(SKIP_WARNING), "stderr:\n{stderr}");
    assert!(stdout.contains("good"), "stdout:\n{stdout}");
    assert!(!stdout.contains("oldplugin"), "stdout:\n{stdout}");
    p.toolr(&["greet", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("hi"));
}

#[test]
fn warning_repeats_on_every_run() {
    let p = project_with_skipped_plugin();
    let stderr = p.stderr(&["greet", "--help"]);
    assert!(stderr.contains(SKIP_WARNING), "stderr:\n{stderr}");
    let after_first = std::fs::read(p.tools().join(".toolr-manifest.json")).unwrap();
    let stderr = p.stderr(&["greet", "--help"]);
    assert!(stderr.contains(SKIP_WARNING), "stderr:\n{stderr}");
    let after_second = std::fs::read(p.tools().join(".toolr-manifest.json")).unwrap();
    assert!(
        after_first == after_second,
        "second run rewrote the manifest instead of reading the cache"
    );
}

#[test]
fn quiet_suppresses_the_warning() {
    let p = project_with_skipped_plugin();
    let stderr = p.stderr(&["--quiet", "--help"]);
    assert!(!stderr.contains("warning: skipping"), "stderr:\n{stderr}");
}

#[test]
fn completion_never_warns() {
    let p = project_with_skipped_plugin();
    let cwd = p.tmp.path().to_string_lossy().to_string();
    let stderr = p.stderr(&["__complete", &cwd, ""]);
    assert!(!stderr.contains("warning: skipping"), "stderr:\n{stderr}");
}

#[test]
fn self_commands_never_warn() {
    let p = project_with_skipped_plugin();
    let stderr = p.stderr(&["self", "--help"]);
    assert!(!stderr.contains("warning: skipping"), "stderr:\n{stderr}");
}

#[test]
fn project_manifest_rebuild_warns_exactly_once() {
    let p = project_with_skipped_plugin();
    let stderr = p.stderr(&["project", "manifest", "rebuild"]);
    assert_eq!(
        stderr
            .matches("warning: skipping plugin demo_plugin")
            .count(),
        1,
        "stderr:\n{stderr}"
    );
}

#[test]
fn shadowed_plugin_command_warns_then_recovers() {
    let p = Project::new();
    p.add_plugin("ci_plugin", &fragment(2, "ci_plugin", "ci", None, "lint"));

    // No local `ci lint`: the plugin command is present, no warning.
    let stderr = p.stderr(&["--help"]);
    assert!(!stderr.contains("hiding the one from"), "stderr:\n{stderr}");
    assert!(p.manifest().contains("ci_plugin.commands"));

    // A local `ci lint` hides it, and the next run says so.
    p.write_tool("ci.py", LOCAL_CI_LINT);
    let stderr = p.stderr(&["ci", "--help"]);
    assert!(
        stderr.contains("hiding the one from ci_plugin"),
        "stderr:\n{stderr}"
    );
    let manifest = p.manifest();
    assert!(!manifest.contains("ci_plugin.commands"), "{manifest}");
    let parsed: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    let lint = parsed["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["group"] == "ci" && c["name"] == "lint")
        .unwrap_or_else(|| panic!("no local ci lint: {manifest}"));
    assert!(
        lint["module"].as_str().unwrap().starts_with("tools."),
        "{lint}"
    );

    // Removing it brings the plugin command back and clears the warning.
    std::fs::remove_file(p.tools().join("ci.py")).unwrap();
    let stderr = p.stderr(&["--help"]);
    assert!(!stderr.contains("hiding the one from"), "stderr:\n{stderr}");
    assert!(p.manifest().contains("ci_plugin.commands"));
    p.toolr(&["ci", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("lint"));
}

#[test]
fn nested_groups_with_the_same_leaf_name_coexist() {
    let p = Project::new();
    // Plugin: `docker` plus its child `image`, with a `build` command.
    let plugin = r#"{"toolr_schema_version":2,"package":"dock_plugin",
        "groups":[
          {"name":"docker","title":"Docker","description":"D","origin":"third_party"},
          {"name":"image","title":"Image","description":"I","origin":"third_party","parent":"docker"}],
        "commands":[{"name":"build","group":"docker.image","module":"dock_plugin.commands",
            "function":"build_fn","summary":"Plugin build.","description":"",
            "arguments":[],"origin":"third_party"}]}"#;
    p.add_plugin("dock_plugin", plugin);
    p.write_tool("ci.py", LOCAL_CI_IMAGE);

    let stderr = p.stderr(&["--help"]);
    assert!(!stderr.contains("warning"), "stderr:\n{stderr}");

    let docker = p.stdout(&["docker", "image", "build", "--help"]);
    assert!(docker.contains("Plugin build."), "{docker}");
    assert!(!docker.contains("Local build."), "{docker}");
    let ci = p.stdout(&["ci", "image", "build", "--help"]);
    assert!(ci.contains("Local build."), "{ci}");
    assert!(!ci.contains("Plugin build."), "{ci}");

    // Each `build` resolves to its own function, checked via the manifest
    // because the fixture venv can't import the plugin package.
    let manifest: serde_json::Value = serde_json::from_str(&p.manifest()).unwrap();
    let module_of = |group: &str| {
        manifest["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["group"] == group && c["name"] == "build")
            .unwrap_or_else(|| panic!("no `build` in {group}: {manifest}"))["module"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(module_of("docker.image"), "dock_plugin.commands");
    assert_eq!(module_of("ci.image"), "tools.ci");
}

#[test]
fn host_group_title_wins_over_a_plugin_group() {
    let p = Project::new();
    p.write_tool("ci.py", LOCAL_CI_LINT);
    let plugin = r#"{"toolr_schema_version":2,"package":"ci_plugin",
        "groups":[{"name":"ci","title":"Plugin CI title","description":"D","origin":"third_party"}],
        "commands":[{"name":"deploy","group":"ci","module":"ci_plugin.commands",
            "function":"deploy_fn","summary":"S","description":"",
            "arguments":[],"origin":"third_party"}]}"#;
    p.add_plugin("ci_plugin", plugin);

    let help = p.stdout(&["--help"]);
    assert!(help.contains("CI"), "{help}");
    assert!(!help.contains("Plugin CI title"), "{help}");
    let manifest: serde_json::Value = serde_json::from_str(&p.manifest()).unwrap();
    let ci: Vec<_> = manifest["groups"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|g| g["name"] == "ci")
        .collect();
    assert_eq!(ci.len(), 1, "{manifest}");
    assert_eq!(ci[0]["title"], "CI");
}

#[test]
fn static_drift_keeps_plugin_entries_when_the_venv_cannot_be_resolved() {
    let p = Project::new();
    p.add_plugin(
        "dock_plugin",
        &fragment(2, "dock_plugin", "docker", None, "up"),
    );
    p.add_plugin(
        "demo_plugin",
        &fragment(1, "demo_plugin", "oldplugin", None, "run"),
    );
    p.stderr(&["--help"]);
    let before = p.manifest();
    assert!(before.contains("dock_plugin.commands"), "{before}");
    assert!(before.contains("plugin_warnings"), "{before}");

    // An invalid venv-location makes `resolve_venv_path` fail, so the
    // freshness check sees no venv at all while the static tree drifts.
    p.write_tool("ci.py", LOCAL_CI_LINT);
    let out = p
        .toolr(&["--help"])
        .env("TOOLR_VENV_LOCATION", "bogus")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    let after = p.manifest();
    assert!(
        after.contains("tools.ci"),
        "no rebuild happened:\n{stderr}\n{after}"
    );
    assert!(after.contains("dock_plugin.commands"), "{after}");
    assert!(after.contains("skipping plugin demo_plugin"), "{after}");
}

const CONFLICT_WARNING: &str =
    "toolr: warning: deploy rollout is defined by more than one plugin (a_plugin, b_plugin), so it is disabled.";

/// A fresh project (no cached manifest) where `a_plugin` and `b_plugin` both ship `deploy rollout`,
/// and `c_plugin` ships its own `extra run`.
fn project_with_conflict() -> Project {
    let p = Project::new();
    p.add_plugin(
        "a_plugin",
        &fragment(2, "a_plugin", "deploy", None, "rollout"),
    );
    p.add_plugin(
        "b_plugin",
        &fragment(2, "b_plugin", "deploy", None, "rollout"),
    );
    p.add_plugin("c_plugin", &fragment(2, "c_plugin", "extra", None, "run"));
    p
}

fn has_command(manifest: &serde_json::Value, group: &str, name: &str) -> bool {
    manifest["commands"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["group"] == group && c["name"] == name)
}

#[test]
fn conflicting_plugins_disable_only_that_command() {
    let p = project_with_conflict();
    assert!(
        !p.tools().join(".toolr-manifest.json").exists(),
        "the test needs a fresh project with no cache"
    );

    // Before the fix, the bootstrap build failed here and toolr exited 2.
    let out = p.toolr(&["--help"]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "stderr:\n{stderr}");
    assert_eq!(
        stderr.matches(CONFLICT_WARNING).count(),
        1,
        "stderr:\n{stderr}"
    );
    assert!(stdout.contains("greet"), "stdout:\n{stdout}");
    assert!(stdout.contains("extra"), "stdout:\n{stdout}");

    // The second run reads the cache and still warns exactly once.
    let stderr = p.stderr(&["--help"]);
    assert_eq!(
        stderr.matches(CONFLICT_WARNING).count(),
        1,
        "stderr:\n{stderr}"
    );

    // The local group and the unrelated plugin command still work. The fixture venv
    // can't import plugins, so plugin commands are checked through help and the manifest.
    p.toolr(&["greet", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("hi"));
    let manifest: serde_json::Value = serde_json::from_str(&p.manifest()).unwrap();
    assert!(has_command(&manifest, "extra", "run"), "{manifest}");
    assert!(!has_command(&manifest, "deploy", "rollout"), "{manifest}");

    // Gone from the group's help and from completion.
    let deploy_help = p.toolr(&["deploy", "--help"]).output().unwrap();
    assert!(
        !String::from_utf8_lossy(&deploy_help.stdout).contains("rollout"),
        "{deploy_help:?}"
    );
    let cwd = p.tmp.path().to_string_lossy().to_string();
    let completions = p.stdout(&["__complete", &cwd, "deploy", ""]);
    assert!(
        !completions.contains("rollout"),
        "completions:\n{completions}"
    );
    let top = p.stdout(&["__complete", &cwd, ""]);
    assert!(top.contains("extra"), "completions:\n{top}");
    assert!(top.contains("deploy"), "completions:\n{top}");
}

#[test]
fn uninstalling_one_conflicting_plugin_restores_the_command() {
    let p = project_with_conflict();
    let stderr = p.stderr(&["--help"]);
    assert!(stderr.contains(CONFLICT_WARNING), "stderr:\n{stderr}");

    std::fs::remove_dir_all(
        p.tools()
            .join(".venv/lib/python3.13/site-packages/a_plugin"),
    )
    .unwrap();

    let stderr = p.stderr(&["--help"]);
    assert!(
        !stderr.contains("is defined by more than one plugin"),
        "stderr:\n{stderr}"
    );
    let manifest: serde_json::Value = serde_json::from_str(&p.manifest()).unwrap();
    assert!(has_command(&manifest, "deploy", "rollout"), "{manifest}");
    assert!(p.manifest().contains("b_plugin.commands"));
    p.toolr(&["deploy", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("rollout"));
}
