//! Translate a parsed [`clap::ArgMatches`] into an [`ExecutionSpec`].

use std::collections::BTreeMap;
use std::path::Path;

use clap::ArgMatches;
use serde_json::Value;

use crate::manifest::{Argument, ArgumentKind, Command};
use crate::parser::SupportedType;

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
    // Heterogeneous tuples are configured with `num_args(N)` and need
    // multi-value retrieval even when the manifest `kind` says
    // `Positional` or `Optional`. Route them through `extract_many`
    // before the kind-based switch.
    let is_tuple = matches!(
        arg.resolved_type.as_ref().map(unwrap_optional),
        Some(SupportedType::Tuple(_))
    );
    if is_tuple {
        return Some(Value::Array(extract_many(arg, matches)));
    }
    match arg.kind {
        ArgumentKind::Flag => {
            // clap stored as bool via ArgAction::SetTrue.
            let v = matches.get_flag(arg.name.as_str());
            Some(Value::Bool(v))
        }
        ArgumentKind::Count => {
            // clap stores a u8 via ArgAction::Count. Forward to Python
            // as a JSON number so msgspec can coerce into the target
            // `int` (or `toolr.types.Count`, which is int at runtime).
            let n = matches.get_count(arg.name.as_str());
            Some(Value::Number(u64::from(n).into()))
        }
        ArgumentKind::Positional | ArgumentKind::Optional => {
            extract_scalar(arg, matches)
        }
        ArgumentKind::Repeated | ArgumentKind::VarPositional => {
            Some(Value::Array(extract_many(arg, matches)))
        }
    }
}

fn unwrap_optional(ty: &SupportedType) -> &SupportedType {
    match ty {
        SupportedType::Optional(inner) => inner.as_ref(),
        other => other,
    }
}

fn scalar_element_type(arg: &Argument) -> Option<&SupportedType> {
    let ty = arg.resolved_type.as_ref()?;
    let unwrapped = match ty {
        SupportedType::Optional(inner) => inner.as_ref(),
        other => other,
    };
    Some(match unwrapped {
        SupportedType::List(elem) => elem.as_ref(),
        other => other,
    })
}

fn extract_scalar(arg: &Argument, matches: &ArgMatches) -> Option<Value> {
    let name = arg.name.as_str();
    match scalar_element_type(arg) {
        Some(SupportedType::Int) => matches
            .get_one::<i64>(name)
            .map(|n| Value::Number((*n).into())),
        Some(SupportedType::Float) => matches.get_one::<f64>(name).and_then(|f| {
            serde_json::Number::from_f64(*f).map(Value::Number)
        }),
        Some(SupportedType::Bool) => matches.get_one::<bool>(name).map(|b| Value::Bool(*b)),
        Some(
            SupportedType::Path
            | SupportedType::AbsolutePath
            | SupportedType::ResolvedPath,
        ) => matches
            .get_one::<std::path::PathBuf>(name)
            .map(|p| Value::String(p.to_string_lossy().into_owned())),
        // Everything else — strings (incl. enum / literal / email /
        // datetime / uuid / ip / unannotated) flows through as-is.
        _ => matches
            .get_one::<String>(name)
            .map(|s| Value::String(s.clone())),
    }
}

fn extract_many(arg: &Argument, matches: &ArgMatches) -> Vec<Value> {
    let name = arg.name.as_str();
    match scalar_element_type(arg) {
        Some(SupportedType::Int) => matches
            .get_many::<i64>(name)
            .map(|iter| iter.map(|n| Value::Number((*n).into())).collect())
            .unwrap_or_default(),
        Some(SupportedType::Float) => matches
            .get_many::<f64>(name)
            .map(|iter| {
                iter.filter_map(|f| serde_json::Number::from_f64(*f).map(Value::Number))
                    .collect()
            })
            .unwrap_or_default(),
        Some(SupportedType::Bool) => matches
            .get_many::<bool>(name)
            .map(|iter| iter.map(|b| Value::Bool(*b)).collect())
            .unwrap_or_default(),
        Some(
            SupportedType::Path
            | SupportedType::AbsolutePath
            | SupportedType::ResolvedPath,
        ) => matches
            .get_many::<std::path::PathBuf>(name)
            .map(|iter| {
                iter.map(|p| Value::String(p.to_string_lossy().into_owned()))
                    .collect()
            })
            .unwrap_or_default(),
        _ => matches
            .get_many::<String>(name)
            .map(|iter| iter.map(|s| Value::String(s.clone())).collect())
            .unwrap_or_default(),
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
                resolved_type: None,
                path_constraints: None,
                allowed_values: vec![],
                metadata: crate::manifest::ArgMetadata::default(),
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
                resolved_type: None,
                path_constraints: None,
                allowed_values: vec![],
                metadata: crate::manifest::ArgMetadata::default(),
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
