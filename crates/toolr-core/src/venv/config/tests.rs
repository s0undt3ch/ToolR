use super::*;
use tempfile::TempDir;

fn write_pyproject(tools: &Path, body: &str) {
    std::fs::create_dir_all(tools).unwrap();
    std::fs::write(tools.join("pyproject.toml"), body).unwrap();
}

#[test]
fn defaults_when_table_is_absent() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    write_pyproject(&tools, "[project]\nname=\"x\"\nversion=\"0\"\n");
    let cfg = load_toolr_config(&tools).unwrap();
    assert_eq!(cfg.venv_location, VenvLocation::Cache);
    assert!(cfg.python_version.is_none());
}

#[test]
fn legacy_editable_install_key_is_ignored_not_an_error() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    write_pyproject(
        &tools,
        "[project]\nname=\"x\"\nversion=\"0\"\n\n[tool.toolr]\neditable-install = [\".\", \"git+https://example/x\"]\n",
    );
    // Must not error — `ToolrConfig` has no deny_unknown_fields, so the
    // removed key is silently ignored.
    let cfg = load_toolr_config(&tools).unwrap();
    assert_eq!(cfg.venv_location, VenvLocation::Cache);
}

#[test]
fn parses_in_tree_venv_location() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    write_pyproject(
        &tools,
        r#"
[project]
name = "x"
version = "0"

[tool.toolr]
venv-location = "in-tree"
python-version = "3.13"
"#,
    );
    let cfg = load_toolr_config(&tools).unwrap();
    assert_eq!(cfg.venv_location, VenvLocation::InTree);
    assert_eq!(cfg.python_version.as_deref(), Some("3.13"));
}

#[test]
fn reports_missing_pyproject() {
    let tmp = TempDir::new().unwrap();
    let err = load_toolr_config(&tmp.path().join("tools")).unwrap_err();
    assert!(matches!(err, ConfigError::Missing(_)));
}

#[test]
fn reads_requires_python() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    write_pyproject(
        &tools,
        "[project]\nname=\"x\"\nversion=\"0\"\nrequires-python = \">=3.11\"\n",
    );
    let v = read_requires_python(&tools).unwrap();
    assert_eq!(v.as_deref(), Some(">=3.11"));
}
