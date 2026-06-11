use super::model::*;

fn sample_manifest() -> Manifest {
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: "abc123".into(),
        third_party_hash: "".into(),
        groups: vec![Group {
            name: "ci".into(),
            title: "CI utilities".into(),
            description: "CI related utilities.".into(),
            parent: None,
            origin: Origin::Static,
        }],
        commands: vec![Command {
            name: "generate-build-matrix".into(),
            group: "ci".into(),
            module: "tools.ci".into(),
            function: "generate_build_matrix".into(),
            summary: "Generate a build matrix.".into(),
            description: "".into(),
            arguments: vec![],
            origin: Origin::Static,
            dispatched_from: None,
            is_dispatcher: false,
        }],
    }
}

#[test]
fn manifest_round_trips_through_json() {
    let m = sample_manifest();
    let json = serde_json::to_string_pretty(&m).expect("serialize");
    let back: Manifest = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(m, back);
}

#[test]
fn missing_optional_fields_default_to_empty() {
    // Minimal JSON should still deserialize.
    let json = r#"{
        "schema_version": 1,
        "static_hash": "h",
        "groups": [],
        "commands": []
    }"#;
    let m: Manifest = serde_json::from_str(json).expect("deserialize minimal");
    assert_eq!(m.schema_version, 1);
    assert!(m.third_party_hash.is_empty());
}

#[test]
fn legacy_imports_key_is_tolerated() {
    // Manifests written by an older toolr carry a now-removed `"imports"`
    // key on each command. The `Command` struct no longer declares the
    // field, but it must still deserialize: there is no
    // `deny_unknown_fields`, so the stale key is ignored rather than
    // erroring. This guards the forward/backward-compat we rely on to
    // avoid a schema-version bump for the field's removal.
    let json = r#"{
        "schema_version": 1,
        "static_hash": "h",
        "third_party_hash": "",
        "groups": [{"name": "ci", "title": "CI", "description": "", "origin": "static"}],
        "commands": [{
            "name": "hello", "group": "ci", "module": "tools.ci",
            "function": "hello", "summary": "", "description": "",
            "arguments": [], "imports": ["packaging"], "origin": "static"
        }]
    }"#;
    let m: Manifest = serde_json::from_str(json).expect("legacy imports key must be tolerated");
    assert_eq!(m.commands.len(), 1);
    assert_eq!(m.commands[0].name, "hello");
}

use super::io::{ManifestError, load_manifest, write_manifest};
use tempfile::TempDir;

#[test]
fn write_then_load_round_trips() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join(".toolr-manifest.json");
    let m = sample_manifest();
    write_manifest(&path, &m).expect("write");
    let loaded = load_manifest(&path).expect("load");
    assert_eq!(m, loaded);
}

#[test]
fn load_rejects_unknown_schema_version() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join(".toolr-manifest.json");
    std::fs::write(
        &path,
        r#"{"schema_version": 999, "static_hash": "h", "groups": [], "commands": []}"#,
    )
    .unwrap();
    let err = load_manifest(&path).expect_err("should reject");
    assert!(matches!(err, ManifestError::UnknownSchemaVersion(999)));
}

#[test]
fn load_returns_io_error_when_missing() {
    let tmp = TempDir::new().unwrap();
    let err = load_manifest(&tmp.path().join("absent.json")).expect_err("should be missing");
    assert!(matches!(err, ManifestError::Io(_)));
}

#[test]
fn legacy_dynamic_origin_is_not_loadable_and_triggers_rebuild() {
    // A manifest written by an older toolr with an entry origin "dynamic"
    // must not deserialize into the new enum; callers treat that as absent
    // and rebuild from source.
    let json = r#"{"schema_version":1,"static_hash":"x","third_party_hash":"",
        "groups":[],"commands":[{"name":"c","group":"g","module":"m","function":"f",
        "summary":"","description":"","arguments":[],"imports":[],"origin":"dynamic",
        "dispatched_from":null,"is_dispatcher":false}]}"#;
    let parsed: Result<Manifest, _> = serde_json::from_str(json);
    assert!(parsed.is_err());
}

mod dispatched_from_tests {
    use super::*;

    fn cmd_with(dispatched_from: Option<String>) -> Command {
        Command {
            name: "migrate".into(),
            group: "django".into(),
            module: "tools.django_dispatcher".into(),
            function: "django".into(),
            summary: String::new(),
            description: String::new(),
            arguments: vec![],
            origin: Origin::Static,
            dispatched_from,
            is_dispatcher: false,
        }
    }

    #[test]
    fn command_serializes_dispatched_from_when_present() {
        let json = serde_json::to_string(&cmd_with(Some("argparse:django".into()))).unwrap();
        assert!(json.contains(r#""dispatched_from":"argparse:django""#));
    }

    #[test]
    fn command_omits_dispatched_from_when_none() {
        let json = serde_json::to_string(&cmd_with(None)).unwrap();
        assert!(!json.contains("dispatched_from"));
    }
}

#[cfg(test)]
mod is_dispatcher_tests {
    use super::*;

    fn cmd_with(is_dispatcher: bool) -> Command {
        Command {
            name: "job".into(),
            group: "jenkins".into(),
            module: "tools.jenkins".into(),
            function: "job".into(),
            summary: String::new(),
            description: String::new(),
            arguments: vec![],
            origin: Origin::Static,
            dispatched_from: None,
            is_dispatcher,
        }
    }

    #[test]
    fn command_serializes_is_dispatcher_when_true() {
        let json = serde_json::to_string(&cmd_with(true)).unwrap();
        assert!(json.contains(r#""is_dispatcher":true"#));
    }

    #[test]
    fn command_omits_is_dispatcher_when_false() {
        let json = serde_json::to_string(&cmd_with(false)).unwrap();
        assert!(!json.contains("is_dispatcher"));
    }
}
