use super::payload::{DynamicPayload, PAYLOAD_SCHEMA_VERSION};
use crate::manifest::{Command, Group, Origin};

fn sample_payload() -> DynamicPayload {
    DynamicPayload {
        payload_schema_version: PAYLOAD_SCHEMA_VERSION,
        groups: vec![Group {
            name: "legacy".into(),
            title: "Legacy entry-point group".into(),
            description: "".into(),
            parent: None,
            origin: Origin::Dynamic,
        }],
        commands: vec![Command {
            name: "frob".into(),
            group: "legacy".into(),
            module: "third_party_pkg.commands".into(),
            function: "frob".into(),
            summary: "Frob the thing.".into(),
            description: "".into(),
            arguments: vec![],
            imports: vec![],
            origin: Origin::Dynamic,
            dispatched_from: None,
        }],
        warnings: vec![],
    }
}

#[test]
fn payload_round_trips_through_json() {
    let p = sample_payload();
    let json = serde_json::to_string(&p).expect("serialize");
    let back: DynamicPayload = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(p, back);
}

#[test]
fn retag_overwrites_origin_to_dynamic() {
    let mut p = sample_payload();
    p.groups[0].origin = Origin::Static;
    p.commands[0].origin = Origin::Static;
    let retagged = p.retag_as_dynamic();
    assert_eq!(retagged.groups[0].origin, Origin::Dynamic);
    assert_eq!(retagged.commands[0].origin, Origin::Dynamic);
}

#[test]
fn missing_warnings_field_defaults_to_empty() {
    let json = r#"{
        "payload_schema_version": 1,
        "groups": [],
        "commands": []
    }"#;
    let p: DynamicPayload = serde_json::from_str(json).expect("deserialize minimal");
    assert!(p.warnings.is_empty());
}
