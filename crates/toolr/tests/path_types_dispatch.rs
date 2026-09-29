//! The path types reject bad input at clap-parse time, before any Python
//! is spawned. The fixture manifest has no `tools/pyproject.toml`, so the
//! binary uses it as written without a freshness rebuild.

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

fn fixture(kind: &str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir(&tools).unwrap();
    let manifest = format!(
        r#"{{
    "schema_version": 2,
    "static_hash": "h",
    "third_party_hash": "",
    "groups": [{{"name": "probe", "title": "Probe", "description": "", "origin": "static"}}],
    "commands": [{{
        "name": "read", "group": "probe", "module": "tools.probe", "function": "read",
        "summary": "Path probe.", "description": "",
        "arguments": [{{
            "name": "target", "kind": "optional", "help": "a path.", "default": null,
            "type_annotation": "toolr.types.X", "resolved_type": {{"kind": "{kind}"}},
            "allowed_values": []
        }}],
        "origin": "static"
    }}]
}}"#
    );
    fs::write(tools.join(".toolr-manifest.json"), manifest).unwrap();
    tmp
}

fn assert_rejected(kind: &str, value: &str, message: &str) {
    let tmp = fixture(kind);
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["probe", "read", "--target", value])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{kind}: stderr:\n{stderr}");
    assert!(stderr.contains(message), "{kind}: stderr:\n{stderr}");
}

#[test]
fn each_path_type_rejects_its_bad_input() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("f.txt");
    fs::write(&file, "x").unwrap();
    let dir = tmp.path().to_str().unwrap();
    let file = file.to_str().unwrap();
    let missing = tmp.path().join("missing");
    let missing = missing.to_str().unwrap();
    let orphan = tmp.path().join("missing").join("out.txt");
    let orphan = orphan.to_str().unwrap();

    assert_rejected("resolved_path", missing, &format!("path does not exist: {missing}"));
    assert_rejected("file_path", dir, &format!("path is not a regular file: {dir}"));
    assert_rejected("directory_path", file, &format!("path is not a directory: {file}"));
    assert_rejected("new_path", file, &format!("path already exists: {file}"));
    assert_rejected("new_path", orphan, "parent directory does not exist:");
    assert_rejected("writable_directory_path", file, &format!("path is not a directory: {file}"));
    assert_rejected("executable_path", dir, &format!("path is not a regular file: {dir}"));
}

#[cfg(unix)]
#[test]
fn executable_path_rejects_a_non_executable_file() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let tool = tmp.path().join("tool");
    fs::write(&tool, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o644)).unwrap();
    let tool = tool.to_str().unwrap();
    assert_rejected("executable_path", tool, &format!("path is not executable: {tool}"));
}
