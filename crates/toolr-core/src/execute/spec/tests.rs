use super::*;

#[test]
fn spec_round_trips_through_json() {
    let mut spec = ExecutionSpec::new("ci", "hello", "tools.ci", "hello", "/repo");
    spec.args
        .insert("name".into(), serde_json::Value::String("Alice".into()));
    let json = serde_json::to_string(&spec).expect("serialize");
    let back: ExecutionSpec = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(spec, back);
}

#[test]
fn spec_json_uses_python_field_names() {
    let spec = ExecutionSpec::new("ci", "hello", "tools.ci", "hello", "/repo");
    let json = serde_json::to_string(&spec).expect("serialize");
    // These exact strings are what `toolr._runner.RunnerSpec` decodes.
    assert!(json.contains("\"schema_version\":3"), "got: {json}");
    assert!(json.contains("\"group\":\"ci\""), "got: {json}");
    assert!(json.contains("\"command\":\"hello\""), "got: {json}");
    assert!(json.contains("\"repo_root\":\"/repo\""), "got: {json}");
    assert!(json.contains("\"verbosity\":\"normal\""), "got: {json}");
}

#[test]
fn schema_version_constant_is_3() {
    assert_eq!(RUNNER_SCHEMA_VERSION, 3);
}

fn arg_with_type(
    name: &str,
    ty: Option<crate::parser::SupportedType>,
) -> crate::manifest::Argument {
    crate::manifest::Argument {
        name: name.to_string(),
        kind: crate::manifest::ArgumentKind::Optional,
        help: String::new(),
        default: None,
        type_annotation: None,
        resolved_type: ty,
        allowed_values: vec![],
        metadata: crate::manifest::ArgMetadata::default(),
        long_flag: None,
    }
}

#[test]
fn enum_modules_for_collects_bare_and_optional_enum_args() {
    use crate::parser::SupportedType;
    let arguments = vec![
        arg_with_type(
            "env",
            Some(SupportedType::Enum {
                name: "Environment".into(),
                module: "tools.metrics._common".into(),
                values: vec!["production".into()],
            }),
        ),
        arg_with_type(
            "mode",
            Some(SupportedType::Optional(Box::new(SupportedType::Enum {
                name: "Mode".into(),
                module: "tools.metrics.modes".into(),
                values: vec!["fast".into()],
            }))),
        ),
        arg_with_type("count", Some(SupportedType::Int)),
        arg_with_type("untyped", None),
    ];
    let modules = enum_modules_for(&arguments);
    assert_eq!(
        modules.get("Environment").map(String::as_str),
        Some("tools.metrics._common")
    );
    assert_eq!(
        modules.get("Mode").map(String::as_str),
        Some("tools.metrics.modes")
    );
    assert_eq!(modules.len(), 2);
}
