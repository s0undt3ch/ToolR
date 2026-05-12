use crate::complete::serve_completions;
use crate::manifest::{
    Argument, ArgumentKind, Command, Group, Manifest, Origin, SCHEMA_VERSION,
};

fn fixture() -> Manifest {
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: "h".into(),
        dynamic_hash: String::new(),
        groups: vec![
            Group {
                name: "ci".into(),
                title: "CI utilities".into(),
                description: String::new(),
                origin: Origin::Static,
            },
            Group {
                name: "data".into(),
                title: "Data utilities".into(),
                description: String::new(),
                origin: Origin::Static,
            },
        ],
        commands: vec![
            Command {
                name: "hello".into(),
                group: "ci".into(),
                module: "tools.ci".into(),
                function: "hello".into(),
                summary: "Say hello.".into(),
                description: String::new(),
                arguments: vec![Argument {
                    name: "name".into(),
                    kind: ArgumentKind::Optional,
                    help: "Who to greet".into(),
                    default: Some("\"world\"".into()),
                    type_annotation: Some("str".into()),
                    allowed_values: vec![],
                }],
                imports: vec![],
                origin: Origin::Static,
            },
            Command {
                name: "deploy".into(),
                group: "ci".into(),
                module: "tools.ci".into(),
                function: "deploy".into(),
                summary: "Deploy something.".into(),
                description: String::new(),
                arguments: vec![Argument {
                    name: "env".into(),
                    kind: ArgumentKind::Optional,
                    help: "Target env".into(),
                    default: None,
                    type_annotation: Some("Literal".into()),
                    allowed_values: vec!["staging".into(), "production".into()],
                }],
                imports: vec![],
                origin: Origin::Static,
            },
            Command {
                name: "load".into(),
                group: "data".into(),
                module: "tools.data".into(),
                function: "load".into(),
                summary: "Load data.".into(),
                description: String::new(),
                arguments: vec![Argument {
                    name: "shape".into(),
                    kind: ArgumentKind::Positional,
                    help: "Shape".into(),
                    default: None,
                    type_annotation: Some("Literal".into()),
                    allowed_values: vec!["wide".into(), "tall".into()],
                }],
                imports: vec![],
                origin: Origin::Static,
            },
        ],
    }
}

fn tokens(words: &[&str]) -> Vec<String> {
    words.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn empty_tokens_lists_all_groups() {
    let out = serve_completions(&fixture(), &tokens(&[""]));
    assert_eq!(out, vec!["ci".to_string(), "data".to_string()]);
}

#[test]
fn group_prefix_filters_groups() {
    let out = serve_completions(&fixture(), &tokens(&["c"]));
    assert_eq!(out, vec!["ci".to_string()]);
}

#[test]
fn after_group_lists_its_commands() {
    let out = serve_completions(&fixture(), &tokens(&["ci", ""]));
    assert_eq!(out, vec!["deploy".to_string(), "hello".to_string()]);
}

#[test]
fn command_prefix_filters_commands() {
    let out = serve_completions(&fixture(), &tokens(&["ci", "h"]));
    assert_eq!(out, vec!["hello".to_string()]);
}

#[test]
fn flag_prefix_lists_argument_flags() {
    let out = serve_completions(&fixture(), &tokens(&["ci", "hello", "--"]));
    assert_eq!(out, vec!["--name".to_string()]);
}

#[test]
fn flag_value_completes_to_allowed_values() {
    let out = serve_completions(&fixture(), &tokens(&["ci", "deploy", "--env", ""]));
    assert_eq!(out, vec!["production".to_string(), "staging".to_string()]);
}

#[test]
fn flag_value_partial_filters_allowed_values() {
    let out = serve_completions(&fixture(), &tokens(&["ci", "deploy", "--env", "s"]));
    assert_eq!(out, vec!["staging".to_string()]);
}

#[test]
fn positional_value_completes_to_allowed_values() {
    let out = serve_completions(&fixture(), &tokens(&["data", "load", ""]));
    assert_eq!(out, vec!["tall".to_string(), "wide".to_string()]);
}

#[test]
fn unknown_group_returns_no_completions() {
    let out = serve_completions(&fixture(), &tokens(&["nope", ""]));
    assert!(out.is_empty());
}

#[test]
fn flag_without_allowed_values_returns_empty() {
    // `--name` has no allowed_values → shell falls back to filename completion.
    let out = serve_completions(&fixture(), &tokens(&["ci", "hello", "--name", ""]));
    assert!(out.is_empty());
}
