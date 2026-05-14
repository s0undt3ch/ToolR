use assert_cmd::Command;
use tempfile::TempDir;

fn write_pyproject(tools: &std::path::Path, body: &str) {
    std::fs::create_dir_all(tools).unwrap();
    std::fs::write(tools.join("pyproject.toml"), body).unwrap();
}

#[test]
fn project_venv_path_prints_cache_path_by_default() {
    let tmp = TempDir::new().unwrap();
    write_pyproject(
        &tmp.path().join("tools"),
        "[project]\nname=\"x\"\nversion=\"0\"\n",
    );
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["project", "venv", "path"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(
        stdout.contains("venv"),
        "expected a path containing `venv`, got: {stdout}"
    );
    assert!(
        !stdout.contains(tmp.path().join("tools").join(".venv").to_string_lossy().as_ref()),
        "default config should not land in-tree, got: {stdout}"
    );
}

#[test]
fn project_venv_path_prints_in_tree_path_when_configured() {
    let tmp = TempDir::new().unwrap();
    write_pyproject(
        &tmp.path().join("tools"),
        "[project]\nname=\"x\"\nversion=\"0\"\n\n[tool.toolr]\nvenv-location = \"in-tree\"\n",
    );
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["project", "venv", "path"])
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Canonicalise the expected path so the comparison matches toolr's
    // output across platforms. On Windows `TempDir` paths return the
    // 8.3 short form (`RUNNER~1`), while toolr internally calls
    // `canonicalize()` which produces the verbatim long-form
    // (`\\?\C:\Users\runneradmin\...`). On macOS `/tmp` is a symlink
    // to `/private/tmp`. Canonicalising the parent (which exists)
    // then joining `.venv` (which doesn't yet) lines both sides up.
    let expected = std::fs::canonicalize(tmp.path().join("tools"))
        .unwrap()
        .join(".venv");
    assert!(
        stdout.contains(expected.to_string_lossy().as_ref()),
        "expected in-tree path {} in: {stdout}",
        expected.display(),
    );
}

#[test]
fn project_venv_path_requires_pyproject() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join("tools")).unwrap();
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["project", "venv", "path"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("pyproject.toml"));
}
