use super::meta::{Meta, MetaError, SCHEMA_VERSION};
use chrono::{TimeZone, Utc};
use std::path::PathBuf;
use tempfile::TempDir;

fn sample_meta() -> Meta {
    Meta {
        schema_version: SCHEMA_VERSION,
        repo_path: PathBuf::from("/home/u/repo"),
        toolr_version: "1.0.0".into(),
        python_version: "3.13.1".into(),
        created_at: Utc.with_ymd_and_hms(2026, 5, 11, 12, 0, 0).unwrap(),
        last_used_at: Utc.with_ymd_and_hms(2026, 5, 11, 12, 34, 56).unwrap(),
    }
}

#[test]
fn meta_round_trips_through_json() {
    let m = sample_meta();
    let s = serde_json::to_string_pretty(&m).expect("serialize");
    let back: Meta = serde_json::from_str(&s).expect("deserialize");
    assert_eq!(m, back);
}

#[test]
fn meta_write_then_load_round_trips() {
    let tmp = TempDir::new().unwrap();
    let m = sample_meta();
    m.write(tmp.path()).expect("write");
    let loaded = Meta::load(tmp.path()).expect("load");
    assert_eq!(m, loaded);
}

#[test]
fn meta_load_rejects_unknown_schema_version() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("meta.json"),
        r#"{
          "schema_version": 999,
          "repo_path": "/x",
          "toolr_version": "1.0.0",
          "python_version": "3.13.1",
          "created_at": "2026-05-11T12:00:00Z",
          "last_used_at": "2026-05-11T12:00:00Z"
        }"#,
    )
    .unwrap();
    let err = Meta::load(tmp.path()).expect_err("should reject");
    assert!(matches!(err, MetaError::UnknownSchemaVersion(999)));
}

#[test]
fn meta_load_missing_returns_io_error() {
    let tmp = TempDir::new().unwrap();
    let err = Meta::load(tmp.path()).expect_err("should be missing");
    assert!(matches!(err, MetaError::Io(_)));
}

#[test]
fn meta_new_sets_created_and_last_used_equal() {
    let m = Meta::new("/x", "1.0.0", "3.13.1");
    assert_eq!(m.created_at, m.last_used_at);
    assert_eq!(m.schema_version, SCHEMA_VERSION);
}
