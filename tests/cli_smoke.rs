use assert_cmd::Command;
use tempfile::TempDir;

fn fixture_with_manifest(json: &str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    std::fs::create_dir(&tools).unwrap();
    std::fs::write(tools.join(".toolr-manifest.json"), json).unwrap();
    tmp
}

#[test]
fn version_flag_works_with_no_project() {
    let tmp = TempDir::new().unwrap();
    Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .arg("--version")
        .assert()
        .success();
}

#[test]
fn help_lists_groups_from_manifest() {
    let json = r#"{
        "schema_version": 1,
        "static_hash": "h",
        "dynamic_hash": "",
        "groups": [
            {"name": "ci", "title": "CI utilities", "description": "", "origin": "static"}
        ],
        "commands": [
            {
                "name": "hello", "group": "ci", "module": "tools.ci",
                "function": "hello", "summary": "Say hello.",
                "description": "", "arguments": [], "imports": [],
                "origin": "static"
            }
        ]
    }"#;
    let tmp = fixture_with_manifest(json);
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .arg("--help")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ci"), "expected ci group in help, got:\n{stdout}");
}

#[test]
fn running_a_user_command_emits_not_implemented_stub() {
    let json = r#"{
        "schema_version": 1, "static_hash": "h", "dynamic_hash": "",
        "groups": [{"name": "ci", "title": "CI", "description": "", "origin": "static"}],
        "commands": [{
            "name": "hello", "group": "ci", "module": "tools.ci",
            "function": "hello", "summary": "", "description": "",
            "arguments": [], "imports": [], "origin": "static"
        }]
    }"#;
    let tmp = fixture_with_manifest(json);
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["ci", "hello"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(64));
    assert!(stderr.contains("execution backend not yet implemented"));
}
