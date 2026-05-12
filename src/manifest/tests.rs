use super::model::*;

fn sample_manifest() -> Manifest {
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: "abc123".into(),
        dynamic_hash: "".into(),
        groups: vec![Group {
            name: "ci".into(),
            title: "CI utilities".into(),
            description: "CI related utilities.".into(),
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
            imports: vec!["packaging".into()],
            origin: Origin::Static,
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
    assert!(m.dynamic_hash.is_empty());
}
