use super::*;
use crate::execute::spec::{ContextSpec, ExecutionSpec, RUNNER_SCHEMA_VERSION};
use std::collections::BTreeMap;
use std::fs;

fn sample_spec() -> ExecutionSpec {
    ExecutionSpec {
        schema_version: RUNNER_SCHEMA_VERSION,
        group: "ci".into(),
        command: "hello".into(),
        module: "tools.ci".into(),
        function: "hello".into(),
        args: BTreeMap::new(),
        context: ContextSpec {
            repo_root: "/repo".into(),
            verbosity: "normal".into(),
            timestamps: false,
            log_level: "INFO".into(),
            default_timeout_secs: None,
            default_no_output_timeout_secs: None,
        },
        enum_modules: BTreeMap::new(),
        dispatch: None,
    }
}

#[test]
fn writes_valid_json_to_disk() {
    let spec = sample_spec();
    let file = write_spec_to_tempfile(&spec).expect("write");
    let read_back: ExecutionSpec =
        serde_json::from_slice(&fs::read(file.path()).unwrap()).expect("parse");
    assert_eq!(spec, read_back);
}

#[test]
fn tempfile_path_has_expected_prefix_and_suffix() {
    let spec = sample_spec();
    let file = write_spec_to_tempfile(&spec).expect("write");
    let name = file
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(name.starts_with("toolr-spec-"), "name was {name}");
    assert!(name.ends_with(".json"), "name was {name}");
}

#[test]
fn tempfile_is_deleted_on_drop() {
    let spec = sample_spec();
    let file = write_spec_to_tempfile(&spec).expect("write");
    let path = file.path().to_path_buf();
    assert!(path.exists());
    drop(file);
    assert!(!path.exists(), "tempfile should be gone after drop");
}
