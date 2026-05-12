//! Translate a parsed [`clap::ArgMatches`] into an [`ExecutionSpec`].

use std::collections::BTreeMap;
use std::path::Path;

use clap::ArgMatches;
use serde_json::Value;

use crate::manifest::{Argument, ArgumentKind, Command};

use super::spec::{ContextSpec, ExecutionSpec, RUNNER_SCHEMA_VERSION};

/// Build the spec to write to disk, given:
///
/// - `cmd`: the matched manifest command (already located by `dispatch`).
/// - `matches`: clap's parsed matches *for this command* (not the root).
/// - `repo_root`: the project root previously resolved by
///   `discover_project_root`.
/// - `verbosity` / `timestamps` / `log_level`: pulled from the global CLI
///   args by the caller.
pub fn build_spec(
    cmd: &Command,
    matches: &ArgMatches,
    repo_root: &Path,
    verbosity: &str,
    timestamps: bool,
    log_level: &str,
) -> ExecutionSpec {
    let mut args = BTreeMap::new();
    for arg in &cmd.arguments {
        if let Some(value) = extract_value(arg, matches) {
            args.insert(arg.name.clone(), value);
        }
    }
    ExecutionSpec {
        schema_version: RUNNER_SCHEMA_VERSION,
        group: cmd.group.clone(),
        command: cmd.name.clone(),
        module: cmd.module.clone(),
        function: cmd.function.clone(),
        args,
        context: ContextSpec {
            repo_root: repo_root.to_string_lossy().into_owned(),
            verbosity: verbosity.to_string(),
            timestamps,
            log_level: log_level.to_string(),
        },
    }
}

fn extract_value(arg: &Argument, matches: &ArgMatches) -> Option<Value> {
    match arg.kind {
        ArgumentKind::Flag => {
            // clap stored as bool via ArgAction::SetTrue.
            let v = matches.get_flag(arg.name.as_str());
            Some(Value::Bool(v))
        }
        ArgumentKind::Positional | ArgumentKind::Optional => matches
            .get_one::<String>(arg.name.as_str())
            .map(|s| Value::String(s.clone())),
        ArgumentKind::Repeated | ArgumentKind::VarPositional => {
            // Both kinds may capture zero, one, or many values via clap's
            // `get_many`; we always emit a JSON array so the Python runner
            // can hand it to msgspec for element-wise coercion.
            let values: Vec<Value> = matches
                .get_many::<String>(arg.name.as_str())
                .map(|iter| iter.map(|s| Value::String(s.clone())).collect())
                .unwrap_or_default();
            Some(Value::Array(values))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Argument, ArgumentKind, Command, Origin};
    use clap::{Arg, ArgAction};

    fn cmd_hello_with_name_arg() -> Command {
        Command {
            name: "hello".into(),
            group: "ci".into(),
            module: "tools.ci".into(),
            function: "hello".into(),
            summary: "".into(),
            description: "".into(),
            arguments: vec![Argument {
                name: "name".into(),
                kind: ArgumentKind::Optional,
                help: "".into(),
                default: Some("world".into()),
                type_annotation: None,
                allowed_values: vec![],
            }],
            imports: vec![],
            origin: Origin::Static,
        }
    }

    fn parse(value: &str) -> ArgMatches {
        clap::Command::new("hello")
            .arg(
                Arg::new("name")
                    .long("name")
                    .default_value("world"),
            )
            .get_matches_from(["hello", "--name", value])
    }

    #[test]
    fn build_spec_copies_static_fields() {
        let cmd = cmd_hello_with_name_arg();
        let matches = parse("Alice");
        let spec = build_spec(
            &cmd,
            &matches,
            Path::new("/repo"),
            "normal",
            false,
            "INFO",
        );
        assert_eq!(spec.group, "ci");
        assert_eq!(spec.command, "hello");
        assert_eq!(spec.module, "tools.ci");
        assert_eq!(spec.function, "hello");
        assert_eq!(spec.context.repo_root, "/repo");
    }

    #[test]
    fn build_spec_extracts_optional_arg_value() {
        let cmd = cmd_hello_with_name_arg();
        let matches = parse("Alice");
        let spec = build_spec(
            &cmd,
            &matches,
            Path::new("/repo"),
            "normal",
            false,
            "INFO",
        );
        assert_eq!(spec.args.get("name"), Some(&Value::String("Alice".into())));
    }

    #[test]
    fn build_spec_extracts_flag_as_bool() {
        let cmd = Command {
            name: "switch".into(),
            group: "ci".into(),
            module: "tools.ci".into(),
            function: "switch".into(),
            summary: "".into(),
            description: "".into(),
            arguments: vec![Argument {
                name: "force".into(),
                kind: ArgumentKind::Flag,
                help: "".into(),
                default: None,
                type_annotation: None,
                allowed_values: vec![],
            }],
            imports: vec![],
            origin: Origin::Static,
        };
        let matches = clap::Command::new("switch")
            .arg(Arg::new("force").long("force").action(ArgAction::SetTrue))
            .get_matches_from(["switch", "--force"]);
        let spec = build_spec(
            &cmd,
            &matches,
            Path::new("/repo"),
            "normal",
            false,
            "INFO",
        );
        assert_eq!(spec.args.get("force"), Some(&Value::Bool(true)));
    }
}
