//! Translate a parsed [`clap::ArgMatches`] into an [`ExecutionSpec`].

use std::collections::BTreeMap;
use std::path::Path;

use clap::ArgMatches;
use serde_json::Value;

use toolr_core::execute::{
    ArgSchemaSpec, CommandSchemaSpec, ContextSpec, DispatchSpec, ExecutionSpec,
    RUNNER_SCHEMA_VERSION,
};
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
        dispatch: None,
    }
}

/// Packed payload extracted from clap matches for a dispatched leaf.
///
/// Built when the matched command has `dispatched_from` set on its
/// manifest entry. The dispatch seam in [`build_dispatch_spec`] turns
/// this into a `DispatchSpec` (the wire-shape of
/// `toolr.sources.DispatchCommand`) so the Python runner can call
/// `toolr._runner.invoke_dispatcher` instead of running the leaf as a
/// regular command call.
#[derive(Debug, Clone)]
pub struct PackedChild {
    /// The leaf command's name (e.g. `"migrate"`).
    pub name: String,
    /// Per-argument values extracted from clap matches, keyed by the
    /// argument's manifest name. Values are JSON-shaped so the existing
    /// `extract_value` machinery (used by `build_spec`) drives the
    /// encoding — keeping flags as bools, counts as numbers, etc. —
    /// rather than coercing everything to `String`.
    pub args: BTreeMap<String, Value>,
    /// The full manifest entry for the leaf, so the runtime side has
    /// access to schema metadata (argument list, dispatched_from, …)
    /// without re-looking-up the command.
    pub schema: Command,
}

/// Pack a matched dispatched-leaf's clap arguments into a `PackedChild`.
///
/// Reuses the same per-argument extraction logic as `build_spec` so the
/// on-the-wire shape of dispatched-leaf args is identical to a normal
/// command invocation. Consumed by [`build_dispatch_spec`] on the
/// Rust side and `toolr._runner.invoke_dispatcher` on the Python side.
pub(crate) fn pack_child_args(cmd: &Command, matches: &ArgMatches) -> PackedChild {
    let mut args = BTreeMap::new();
    for arg in &cmd.arguments {
        if let Some(value) = extract_value(arg, matches) {
            args.insert(arg.name.clone(), value);
        }
    }
    PackedChild {
        name: cmd.name.clone(),
        args,
        schema: cmd.clone(),
    }
}

/// Build the execution spec for a dispatched leaf invocation.
///
/// `parent` is the dispatcher Command whose `module`/`function` get
/// invoked; `parent_matches` are clap matches at the level *above* the
/// leaf (i.e. the group-level matches owning the dispatcher's own
/// args, if any). `packed` carries the leaf's name + args + schema —
/// shipped to the runner under [`ExecutionSpec::dispatch`].
///
/// The runner sees `module`/`function` pointing at the parent, `args`
/// carrying the parent's own kwargs, and `dispatch` carrying everything
/// `invoke_dispatcher` needs to build a `DispatchCommand`.
pub fn build_dispatch_spec(
    parent: &Command,
    parent_matches: &ArgMatches,
    packed: PackedChild,
    repo_root: &Path,
    output_opts: &OutputOptions,
) -> ExecutionSpec {
    let mut parent_args = BTreeMap::new();
    for arg in &parent.arguments {
        if let Some(value) = extract_value(arg, parent_matches) {
            parent_args.insert(arg.name.clone(), value);
        }
    }
    ExecutionSpec {
        schema_version: RUNNER_SCHEMA_VERSION,
        group: parent.group.clone(),
        command: parent.name.clone(),
        module: parent.module.clone(),
        function: parent.function.clone(),
        args: parent_args,
        context: ContextSpec {
            repo_root: repo_root.to_string_lossy().into_owned(),
            verbosity: output_opts.verbosity.clone(),
            timestamps: output_opts.timestamps,
            log_level: output_opts.log_level.clone(),
            default_timeout_secs: output_opts.default_timeout_secs,
            default_no_output_timeout_secs: output_opts.default_no_output_timeout_secs,
        },
        dispatch: Some(DispatchSpec {
            command: packed.name,
            command_args: packed.args,
            schema: command_to_schema_spec(&packed.schema),
        }),
    }
}

/// Convert a manifest [`Command`] into the wire-shape consumed by
/// `msgspec.convert` on the Python side to reconstruct a
/// `toolr.sources.CommandSchema`.
fn command_to_schema_spec(cmd: &Command) -> CommandSchemaSpec {
    CommandSchemaSpec {
        name: cmd.name.clone(),
        summary: cmd.summary.clone(),
        description: cmd.description.clone(),
        arguments: cmd.arguments.iter().map(argument_to_arg_schema).collect(),
    }
}

/// Convert a manifest [`Argument`] into the wire-shape of
/// `toolr.sources.ArgSchema`.
///
/// Maps:
/// - `kind` → one of `"positional" | "optional" | "flag" | "repeated"`.
///   `VarPositional` is encoded as `"repeated"` (closest argparse-shaped
///   equivalent for argv reconstruction); `Count` falls back to
///   `"flag"`. The argparse scanner only emits the first four kinds, so
///   those two arms only fire for hand-authored manifests today.
/// - `choices` ← `allowed_values` (empty → `None`).
/// - `metavar` ← `metadata.metavar`.
/// - `type_annotation` ← `type_annotation`.
/// - `nargs` is always `None` — the manifest doesn't carry argparse-
///   compatible nargs information.
fn argument_to_arg_schema(arg: &Argument) -> ArgSchemaSpec {
    let kind = match arg.kind {
        ArgumentKind::Positional => "positional",
        ArgumentKind::Optional => "optional",
        ArgumentKind::Flag | ArgumentKind::Count => "flag",
        ArgumentKind::Repeated | ArgumentKind::VarPositional => "repeated",
    };
    ArgSchemaSpec {
        name: arg.name.clone(),
        kind: kind.to_string(),
        help: arg.help.clone(),
        default: arg.default.clone(),
        choices: if arg.allowed_values.is_empty() {
            None
        } else {
            Some(arg.allowed_values.clone())
        },
        metavar: arg.metadata.metavar.clone(),
        type_annotation: arg.type_annotation.clone(),
        nargs: None,
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
            is_dispatcher: false,
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
            is_dispatcher: false,
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
            is_dispatcher: false,
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

#[cfg(test)]
mod dispatched_pack_tests {
    //! `pack_child_args` packs a matched dispatched-leaf into a
    //! `PackedChild`. These tests pin the on-the-wire encoding (which
    //! reuses `extract_value`, so flags stay bools and missing
    //! arguments stay absent rather than null) — `build_dispatch_spec`
    //! and the Python runner read the resulting `args` map and must keep
    //! getting the same JSON shape `build_spec` would have produced.
    use super::*;
    use clap::{Arg, ArgAction};
    use toolr_core::manifest::{Argument, ArgumentKind, Command, Origin};

    fn migrate_cmd() -> Command {
        Command {
            name: "migrate".into(),
            group: "django".into(),
            module: "tools.dispatcher".into(),
            function: "django".into(),
            summary: String::new(),
            description: String::new(),
            arguments: vec![Argument {
                name: "check".into(),
                kind: ArgumentKind::Flag,
                help: String::new(),
                default: None,
                type_annotation: None,
                resolved_type: None,
                allowed_values: vec![],
                path_constraints: None,
                metadata: Default::default(),
            }],
            imports: vec![],
            origin: Origin::Static,
            dispatched_from: Some("argparse:django".into()),
            is_dispatcher: false,
        }
    }

    #[test]
    fn pack_child_args_extracts_flag_as_bool() {
        let clap_cmd = clap::Command::new("migrate")
            .arg(Arg::new("check").long("check").action(ArgAction::SetTrue));
        let matches = clap_cmd
            .try_get_matches_from(vec!["migrate", "--check"])
            .unwrap();
        let cmd = migrate_cmd();
        let packed = pack_child_args(&cmd, &matches);
        assert_eq!(packed.name, "migrate");
        assert_eq!(packed.args.get("check"), Some(&Value::Bool(true)));
        // Schema round-trips intact (Task 17 reads it on the Python side).
        assert_eq!(packed.schema.dispatched_from.as_deref(), Some("argparse:django"));
        assert_eq!(packed.schema.function, "django");
    }

    #[test]
    fn pack_child_args_unset_flag_records_false_value() {
        // Flags route through `get_flag`, which always yields a bool,
        // so an unset flag still appears in the map as `false`. This
        // matches `build_spec`'s encoding — `extract_value` always
        // returns Some for `ArgumentKind::Flag`.
        let clap_cmd = clap::Command::new("migrate")
            .arg(Arg::new("check").long("check").action(ArgAction::SetTrue));
        let matches = clap_cmd
            .try_get_matches_from(vec!["migrate"])
            .unwrap();
        let packed = pack_child_args(&migrate_cmd(), &matches);
        assert_eq!(packed.args.get("check"), Some(&Value::Bool(false)));
    }

    #[test]
    fn build_dispatch_spec_routes_to_parent_function() {
        // The grafted leaf carries the parent dispatcher's module +
        // function (set by `graft_children`). `build_dispatch_spec`
        // copies those into the spec so the runner imports the parent.
        let leaf = migrate_cmd();
        let packed = pack_child_args(
            &leaf,
            &clap::Command::new("migrate")
                .arg(Arg::new("check").long("check").action(ArgAction::SetTrue))
                .try_get_matches_from(vec!["migrate", "--check"])
                .unwrap(),
        );
        // Parent dispatcher has no own args in this scenario — use a
        // copy of the leaf shape but with an empty arguments list so
        // `build_dispatch_spec` won't try to extract a `check` arg
        // from the parent matches.
        let parent = Command {
            arguments: vec![],
            ..migrate_cmd()
        };
        let parent_matches = clap::Command::new("django")
            .try_get_matches_from(vec!["django"])
            .unwrap();
        let spec = build_dispatch_spec(
            &parent,
            &parent_matches,
            packed,
            Path::new("/repo"),
            &OutputOptions::default(),
        );
        assert_eq!(spec.module, "tools.dispatcher");
        assert_eq!(spec.function, "django");
        let dispatch = spec.dispatch.expect("dispatch present");
        assert_eq!(dispatch.command, "migrate");
        assert_eq!(
            dispatch.command_args.get("check"),
            Some(&Value::Bool(true))
        );
        assert_eq!(dispatch.schema.name, "migrate");
        assert_eq!(dispatch.schema.arguments.len(), 1);
        assert_eq!(dispatch.schema.arguments[0].name, "check");
        assert_eq!(dispatch.schema.arguments[0].kind, "flag");
    }

    #[test]
    fn build_dispatch_spec_extracts_parent_kwargs() {
        // When the parent dispatcher has its own args (e.g. `--cpu`),
        // those are extracted from `parent_matches` into `spec.args` —
        // distinct from the leaf args carried under `dispatch`.
        let parent = Command {
            arguments: vec![Argument {
                name: "cpu".into(),
                kind: ArgumentKind::Optional,
                help: String::new(),
                default: None,
                type_annotation: None,
                resolved_type: None,
                allowed_values: vec![],
                path_constraints: None,
                metadata: Default::default(),
            }],
            ..migrate_cmd()
        };
        let parent_matches = clap::Command::new("django")
            .arg(Arg::new("cpu").long("cpu"))
            .try_get_matches_from(vec!["django", "--cpu", "5000m"])
            .unwrap();
        let leaf_matches = clap::Command::new("migrate")
            .arg(Arg::new("check").long("check").action(ArgAction::SetTrue))
            .try_get_matches_from(vec!["migrate"])
            .unwrap();
        let packed = pack_child_args(&migrate_cmd(), &leaf_matches);
        let spec = build_dispatch_spec(
            &parent,
            &parent_matches,
            packed,
            Path::new("/repo"),
            &OutputOptions::default(),
        );
        assert_eq!(
            spec.args.get("cpu"),
            Some(&Value::String("5000m".into()))
        );
        // Dispatch payload still carries the leaf's args, independent of parent kwargs.
        let dispatch = spec.dispatch.expect("dispatch present");
        assert_eq!(dispatch.command_args.get("check"), Some(&Value::Bool(false)));
    }

    #[test]
    fn argument_to_arg_schema_maps_all_kinds() {
        fn argument_named(name: &str, kind: ArgumentKind) -> Argument {
            Argument {
                name: name.into(),
                kind,
                help: String::new(),
                default: None,
                type_annotation: None,
                resolved_type: None,
                allowed_values: vec![],
                path_constraints: None,
                metadata: Default::default(),
            }
        }
        let cases = [
            (ArgumentKind::Positional, "positional"),
            (ArgumentKind::Optional, "optional"),
            (ArgumentKind::Flag, "flag"),
            (ArgumentKind::Count, "flag"),
            (ArgumentKind::Repeated, "repeated"),
            (ArgumentKind::VarPositional, "repeated"),
        ];
        for (kind, expected) in cases {
            let schema = argument_to_arg_schema(&argument_named("a", kind));
            assert_eq!(schema.kind, expected, "kind {kind:?}");
        }
    }

    #[test]
    fn argument_to_arg_schema_carries_choices_metavar_and_type_annotation() {
        let metadata = toolr_core::manifest::ArgMetadata {
            metavar: Some("PATH".into()),
            ..Default::default()
        };
        let arg = Argument {
            name: "out".into(),
            kind: ArgumentKind::Optional,
            help: "where to write".into(),
            default: Some("/tmp".into()),
            type_annotation: Some("str".into()),
            resolved_type: None,
            allowed_values: vec!["a".into(), "b".into()],
            path_constraints: None,
            metadata,
        };
        let schema = argument_to_arg_schema(&arg);
        assert_eq!(schema.name, "out");
        assert_eq!(schema.help, "where to write");
        assert_eq!(schema.default.as_deref(), Some("/tmp"));
        assert_eq!(schema.metavar.as_deref(), Some("PATH"));
        assert_eq!(schema.type_annotation.as_deref(), Some("str"));
        assert_eq!(schema.choices.as_deref(), Some(&["a".to_string(), "b".to_string()][..]));
    }

    #[test]
    fn pack_child_args_omits_unset_optional() {
        // Optional args with no clap value present should be absent
        // from the packed map (rather than `null`) — matches the
        // `build_spec_missing_optional_value_does_not_appear_in_args_map`
        // contract.
        let cmd = Command {
            arguments: vec![Argument {
                name: "label".into(),
                kind: ArgumentKind::Optional,
                help: String::new(),
                default: None,
                type_annotation: None,
                resolved_type: None,
                allowed_values: vec![],
                path_constraints: None,
                metadata: Default::default(),
            }],
            ..migrate_cmd()
        };
        let clap_cmd = clap::Command::new("migrate").arg(Arg::new("label").long("label"));
        let matches = clap_cmd
            .try_get_matches_from(vec!["migrate"])
            .unwrap();
        let packed = pack_child_args(&cmd, &matches);
        assert!(!packed.args.contains_key("label"));
    }
}
