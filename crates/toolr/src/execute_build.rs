//! Translate a parsed [`clap::ArgMatches`] into an [`ExecutionSpec`].

use std::collections::BTreeMap;
use std::path::Path;

use clap::ArgMatches;
use serde_json::Value;

use toolr_core::execute::{ContextSpec, ExecutionSpec, RUNNER_SCHEMA_VERSION};
use toolr_core::manifest::{Argument, ArgumentKind, Command};
use toolr_core::parser::SupportedType;

/// Build the spec to write to disk, given:
///
/// - `cmd`: the matched manifest command (already located by `dispatch`).
/// - `matches`: clap's parsed matches *for this command* (not the root).
/// - `repo_root`: the project root previously resolved by
///   `discover_project_root`.
/// - `output_opts`: the values of toolr's root-level "Output Options"
///   flags (`--debug` / `--quiet` / `--timestamps` /
///   `--timeout-secs` / `--no-output-timeout-secs`).
pub fn build_spec(
    cmd: &Command,
    matches: &ArgMatches,
    repo_root: &Path,
    output_opts: &OutputOptions,
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
            verbosity: output_opts.verbosity.clone(),
            timestamps: output_opts.timestamps,
            log_level: output_opts.log_level.clone(),
            default_timeout_secs: output_opts.default_timeout_secs,
            default_no_output_timeout_secs: output_opts.default_no_output_timeout_secs,
        },
    }
}

/// Plumbing struct bundling the root-level "Output Options" flag
/// values for `build_spec`. Adding a new flag is a one-field change
/// instead of growing the `build_spec` argument list again.
#[derive(Debug, Clone)]
pub struct OutputOptions {
    /// One of `"quiet"` / `"normal"` / `"verbose"`.
    pub verbosity: String,
    pub timestamps: bool,
    /// Python `logging` level name.
    pub log_level: String,
    pub default_timeout_secs: Option<f64>,
    pub default_no_output_timeout_secs: Option<f64>,
}

impl Default for OutputOptions {
    fn default() -> Self {
        Self {
            verbosity: "normal".to_string(),
            timestamps: false,
            log_level: "INFO".to_string(),
            default_timeout_secs: None,
            default_no_output_timeout_secs: None,
        }
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
    use toolr_core::manifest::{Argument, ArgumentKind, Command, Origin};
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
                metadata: toolr_core::manifest::ArgMetadata::default(),
            }],
            imports: vec![],
            origin: Origin::Static,
            dispatched_from: None,
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
            &OutputOptions::default(),
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
            &OutputOptions::default(),
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
                metadata: toolr_core::manifest::ArgMetadata::default(),
            }],
            imports: vec![],
            origin: Origin::Static,
            dispatched_from: None,
        };
        let matches = clap::Command::new("switch")
            .arg(Arg::new("force").long("force").action(ArgAction::SetTrue))
            .get_matches_from(["switch", "--force"]);
        let spec = build_spec(
            &cmd,
            &matches,
            Path::new("/repo"),
            &OutputOptions::default(),
        );
        assert_eq!(spec.args.get("force"), Some(&Value::Bool(true)));
    }

    /// Helper that returns an `Argument` with the given name + kind +
    /// resolved type, defaults everywhere else. Used by the type-router
    /// tests so each test reads as "given this resolved type, do we
    /// route to the right `get_one`/`get_many` arm?".
    fn arg_of(name: &str, kind: ArgumentKind, ty: SupportedType) -> Argument {
        Argument {
            name: name.into(),
            kind,
            help: String::new(),
            default: None,
            type_annotation: None,
            resolved_type: Some(ty),
            path_constraints: None,
            allowed_values: vec![],
            metadata: toolr_core::manifest::ArgMetadata::default(),
        }
    }

    fn cmd_with(args: Vec<Argument>) -> Command {
        Command {
            name: "test".into(),
            group: "g".into(),
            module: "tools.g".into(),
            function: "test".into(),
            summary: String::new(),
            description: String::new(),
            arguments: args,
            imports: vec![],
            origin: Origin::Static,
            dispatched_from: None,
        }
    }

    #[test]
    fn build_spec_extracts_count_arg() {
        let cmd = cmd_with(vec![arg_of("verbose", ArgumentKind::Count, SupportedType::Int)]);
        let matches = clap::Command::new("test")
            .arg(
                Arg::new("verbose")
                    .short('v')
                    .action(ArgAction::Count),
            )
            .get_matches_from(["test", "-vvv"]);
        let spec = build_spec(&cmd, &matches, Path::new("/repo"), &OutputOptions::default());
        assert_eq!(spec.args.get("verbose"), Some(&Value::Number(3.into())));
    }

    #[test]
    fn build_spec_extracts_int_scalar() {
        let cmd = cmd_with(vec![arg_of("n", ArgumentKind::Optional, SupportedType::Int)]);
        let matches = clap::Command::new("test")
            .arg(
                Arg::new("n")
                    .long("n")
                    .value_parser(clap::value_parser!(i64))
                    .default_value("42"),
            )
            .get_matches_from(["test"]);
        let spec = build_spec(&cmd, &matches, Path::new("/repo"), &OutputOptions::default());
        assert_eq!(spec.args.get("n"), Some(&Value::Number(42.into())));
    }

    #[test]
    fn build_spec_extracts_float_scalar() {
        let cmd = cmd_with(vec![arg_of("ratio", ArgumentKind::Optional, SupportedType::Float)]);
        let matches = clap::Command::new("test")
            .arg(
                Arg::new("ratio")
                    .long("ratio")
                    .value_parser(clap::value_parser!(f64))
                    .default_value("1.5"),
            )
            .get_matches_from(["test"]);
        let spec = build_spec(&cmd, &matches, Path::new("/repo"), &OutputOptions::default());
        match spec.args.get("ratio") {
            Some(Value::Number(n)) => assert_eq!(n.as_f64(), Some(1.5)),
            other => panic!("expected Float number, got {other:?}"),
        }
    }

    #[test]
    fn build_spec_extracts_bool_scalar_via_optional() {
        let cmd = cmd_with(vec![arg_of("dry", ArgumentKind::Optional, SupportedType::Bool)]);
        let matches = clap::Command::new("test")
            .arg(
                Arg::new("dry")
                    .long("dry")
                    .value_parser(clap::value_parser!(bool))
                    .default_value("false"),
            )
            .get_matches_from(["test", "--dry", "true"]);
        let spec = build_spec(&cmd, &matches, Path::new("/repo"), &OutputOptions::default());
        assert_eq!(spec.args.get("dry"), Some(&Value::Bool(true)));
    }

    #[test]
    fn build_spec_extracts_path_scalar_as_string() {
        let cmd = cmd_with(vec![arg_of("file", ArgumentKind::Optional, SupportedType::Path)]);
        let matches = clap::Command::new("test")
            .arg(
                Arg::new("file")
                    .long("file")
                    .value_parser(clap::value_parser!(std::path::PathBuf))
                    .default_value("/tmp/x"),
            )
            .get_matches_from(["test"]);
        let spec = build_spec(&cmd, &matches, Path::new("/repo"), &OutputOptions::default());
        assert_eq!(spec.args.get("file"), Some(&Value::String("/tmp/x".into())));
    }

    #[test]
    fn build_spec_extracts_optional_inner_type_via_unwrap_optional() {
        // `Optional<Int>` should resolve to the Int arm of `extract_scalar`.
        let cmd = cmd_with(vec![arg_of(
            "maybe_n",
            ArgumentKind::Optional,
            SupportedType::Optional(Box::new(SupportedType::Int)),
        )]);
        let matches = clap::Command::new("test")
            .arg(
                Arg::new("maybe_n")
                    .long("maybe-n")
                    .value_parser(clap::value_parser!(i64))
                    .default_value("7"),
            )
            .get_matches_from(["test"]);
        let spec = build_spec(&cmd, &matches, Path::new("/repo"), &OutputOptions::default());
        assert_eq!(spec.args.get("maybe_n"), Some(&Value::Number(7.into())));
    }

    #[test]
    fn build_spec_extracts_repeated_int_as_json_array() {
        let cmd = cmd_with(vec![arg_of(
            "ports",
            ArgumentKind::Repeated,
            SupportedType::List(Box::new(SupportedType::Int)),
        )]);
        let matches = clap::Command::new("test")
            .arg(
                Arg::new("ports")
                    .long("port")
                    .action(ArgAction::Append)
                    .value_parser(clap::value_parser!(i64)),
            )
            .get_matches_from(["test", "--port", "80", "--port", "443"]);
        let spec = build_spec(&cmd, &matches, Path::new("/repo"), &OutputOptions::default());
        assert_eq!(
            spec.args.get("ports"),
            Some(&Value::Array(vec![Value::Number(80.into()), Value::Number(443.into())])),
        );
    }

    #[test]
    fn build_spec_extracts_repeated_path_as_json_strings() {
        let cmd = cmd_with(vec![arg_of(
            "files",
            ArgumentKind::Repeated,
            SupportedType::List(Box::new(SupportedType::Path)),
        )]);
        let matches = clap::Command::new("test")
            .arg(
                Arg::new("files")
                    .long("file")
                    .action(ArgAction::Append)
                    .value_parser(clap::value_parser!(std::path::PathBuf)),
            )
            .get_matches_from(["test", "--file", "/a", "--file", "/b"]);
        let spec = build_spec(&cmd, &matches, Path::new("/repo"), &OutputOptions::default());
        assert_eq!(
            spec.args.get("files"),
            Some(&Value::Array(vec![
                Value::String("/a".into()),
                Value::String("/b".into()),
            ])),
        );
    }

    #[test]
    fn build_spec_extracts_tuple_arg_with_num_args() {
        // Heterogeneous Tuple routes through `extract_many` even though
        // the kind is Positional — verify the `is_tuple` early branch.
        let cmd = cmd_with(vec![arg_of(
            "pair",
            ArgumentKind::Positional,
            SupportedType::Tuple(vec![SupportedType::Str, SupportedType::Int]),
        )]);
        let matches = clap::Command::new("test")
            .arg(
                Arg::new("pair")
                    .num_args(2)
                    .value_parser(clap::value_parser!(String)),
            )
            .get_matches_from(["test", "name", "42"]);
        let spec = build_spec(&cmd, &matches, Path::new("/repo"), &OutputOptions::default());
        assert_eq!(
            spec.args.get("pair"),
            Some(&Value::Array(vec![
                Value::String("name".into()),
                Value::String("42".into()),
            ])),
        );
    }

    #[test]
    fn build_spec_missing_optional_value_does_not_appear_in_args_map() {
        // No clap value present → extract_scalar returns None → key is
        // absent from the args map. Pinning this protects the Python
        // side, which uses `args.get(name)` and relies on absence
        // (rather than null) for "user didn't pass this".
        let cmd = cmd_with(vec![arg_of(
            "absent",
            ArgumentKind::Optional,
            SupportedType::Str,
        )]);
        let matches = clap::Command::new("test")
            .arg(Arg::new("absent").long("absent"))
            .get_matches_from(["test"]);
        let spec = build_spec(&cmd, &matches, Path::new("/repo"), &OutputOptions::default());
        assert!(!spec.args.contains_key("absent"));
    }

    #[test]
    fn output_options_default_values_match_python_runner_expectations() {
        // The runner side defaults `verbosity` to "normal" and
        // `log_level` to "INFO" when not overridden. Document those
        // strings here so a Rust-side rename gets caught by a unit
        // test instead of a CLI smoke run.
        let opts = OutputOptions::default();
        assert_eq!(opts.verbosity, "normal");
        assert!(!opts.timestamps);
        assert_eq!(opts.log_level, "INFO");
        assert!(opts.default_timeout_secs.is_none());
        assert!(opts.default_no_output_timeout_secs.is_none());
    }
}
