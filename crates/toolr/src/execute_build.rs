//! Translate a parsed [`clap::ArgMatches`] into an [`ExecutionSpec`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clap::ArgMatches;
use clap::parser::ValueSource;
use serde_json::Value;

use toolr_core::execute::{
    ArgSchemaSpec, CommandSchemaSpec, ContextSpec, DispatchSpec, ExecutionSpec,
    RUNNER_SCHEMA_VERSION, enum_modules_for,
};
use toolr_core::manifest::{Argument, ArgumentKind, Command, Nargs};
use toolr_core::parser::SupportedType;

use crate::value_parsers::{DeferredPath, deferred_path, passes_path_as_typed};

/// Build the spec to write to disk, given:
///
/// - `cmd`: the matched manifest command (already located by `dispatch`).
/// - `matches`: clap's parsed matches *for this command* (not the root).
/// - `bases`: the invocation cwd and the project root previously resolved
///   by `discover_project_root`.
/// - `output_opts`: the values of toolr's "ToolR Options"
///   flags (`--debug` / `--quiet` / `--timestamps` /
///   `--timeout-secs` / `--no-output-timeout-secs`).
pub fn build_spec(
    cmd: &Command,
    matches: &ArgMatches,
    bases: &PathBases,
    output_opts: &OutputOptions,
) -> anyhow::Result<ExecutionSpec> {
    let repo_root = bases.repo_root;
    // SEC-02: the runner chdirs to repo_root, so a relative bare `Path` typed
    // from a subdirectory resolves from the repo root, not the user's cwd
    // (every other path type is resolved here, against the cwd). Warn here
    // (the Rust layer knows the cwd, the arg types, and which values the user
    // actually typed) rather than after coercion in Python.
    warn_relative_paths(cmd, matches, bases);
    let args = extract_args(cmd, matches, bases)?;
    Ok(ExecutionSpec {
        schema_version: RUNNER_SCHEMA_VERSION,
        group: cmd.group.clone(),
        command: cmd.name.clone(),
        module: cmd.module.clone(),
        function: cmd.function.clone(),
        args,
        context: ContextSpec {
            repo_root: repo_root.to_string_lossy().into_owned(),
            verbosity: output_opts.verbosity.as_str().to_owned(),
            timestamps: output_opts.timestamps,
            log_level: output_opts.log_level.clone(),
            default_timeout_secs: output_opts.default_timeout_secs,
            default_no_output_timeout_secs: output_opts.default_no_output_timeout_secs,
        },
        enum_modules: enum_modules_for(&cmd.arguments),
        dispatch: None,
    })
}

/// Where relative path values resolve from: a typed value from the
/// invocation cwd, a deferred default from the repo root.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PathBases<'a> {
    pub cwd: &'a Path,
    pub repo_root: &'a Path,
}

fn extract_args(
    cmd: &Command,
    matches: &ArgMatches,
    bases: &PathBases,
) -> anyhow::Result<BTreeMap<String, Value>> {
    let mut args = BTreeMap::new();
    for arg in &cmd.arguments {
        if let Some(value) = extract_value(arg, matches, bases)? {
            args.insert(arg.name.clone(), value);
        }
    }
    Ok(args)
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
pub(crate) fn pack_child_args(
    cmd: &Command,
    matches: &ArgMatches,
    bases: &PathBases,
) -> anyhow::Result<PackedChild> {
    Ok(PackedChild {
        name: cmd.name.clone(),
        args: extract_args(cmd, matches, bases)?,
        schema: cmd.clone(),
    })
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
    bases: &PathBases,
    output_opts: &OutputOptions,
) -> anyhow::Result<ExecutionSpec> {
    let repo_root = bases.repo_root;
    reject_leaked_dispatcher_flags(parent, &packed)?;
    // Covers the dispatcher's own (parent) args. A dispatched child's args are
    // packed separately and not checked here — same scope as before.
    warn_relative_paths(parent, parent_matches, bases);
    let parent_args = extract_args(parent, parent_matches, bases)?;
    Ok(ExecutionSpec {
        schema_version: RUNNER_SCHEMA_VERSION,
        group: parent.group.clone(),
        command: parent.name.clone(),
        module: parent.module.clone(),
        function: parent.function.clone(),
        args: parent_args,
        context: ContextSpec {
            repo_root: repo_root.to_string_lossy().into_owned(),
            verbosity: output_opts.verbosity.as_str().to_owned(),
            timestamps: output_opts.timestamps,
            log_level: output_opts.log_level.clone(),
            default_timeout_secs: output_opts.default_timeout_secs,
            default_no_output_timeout_secs: output_opts.default_no_output_timeout_secs,
        },
        enum_modules: enum_modules_for(&parent.arguments),
        dispatch: Some(DispatchSpec {
            command: packed.name,
            command_args: packed.args,
            schema: command_to_schema_spec(&packed.schema),
        }),
    })
}

/// Long-flag spellings clap would attach to `arg`, mirroring
/// `cli::build_user_command` (always `--` + the kebab-cased name, for
/// any kind that calls `.long(...)`).
fn arg_long_flag_spelling(arg: &Argument) -> Option<String> {
    let has_long_flag = match arg.kind {
        ArgumentKind::Optional
        | ArgumentKind::Flag
        | ArgumentKind::Repeated
        | ArgumentKind::Count => true,
        ArgumentKind::FixedArity => arg.long_flag.is_some(),
        ArgumentKind::Positional
        | ArgumentKind::OptionalPositional
        | ArgumentKind::VarPositional => false,
    };
    has_long_flag.then(|| long_flag(&arg.name))
}

/// The `--kebab-case` long flag clap gives the arg named `name`.
fn long_flag(name: &str) -> String {
    format!("--{}", name.replace('_', "-"))
}

/// Guard against dispatcher-level flags (e.g. `--dry-run`) typed *after*
/// the dispatched subcommand name (see toolr#445): clap only rejects an
/// unrecognized option in scope of the leaf's own args, and the leaf's
/// `VarPositional` args are built with `trailing_var_arg(true)` so any
/// unclaimed token — including one that spells a *parent* dispatcher
/// flag — is silently accepted as a positional value instead of erroring.
/// That's dangerous specifically for a safety flag like `--dry-run`: the
/// dispatcher never sees it and runs as though it had never been passed.
///
/// Cross-checks the leaf's packed `VarPositional` values against the
/// parent dispatcher's own flag spellings and errors before the
/// `ExecutionSpec` (and the side-effecting dispatcher call it drives) is
/// ever built.
fn reject_leaked_dispatcher_flags(parent: &Command, packed: &PackedChild) -> anyhow::Result<()> {
    let parent_flags: Vec<String> = parent
        .arguments
        .iter()
        .filter_map(arg_long_flag_spelling)
        .collect();
    if parent_flags.is_empty() {
        return Ok(());
    }
    for leaf_arg in &packed.schema.arguments {
        if leaf_arg.kind != ArgumentKind::VarPositional {
            continue;
        }
        let Some(Value::Array(values)) = packed.args.get(&leaf_arg.name) else {
            continue;
        };
        for value in values {
            let Value::String(token) = value else {
                continue;
            };
            if let Some(flag) = parent_flags.iter().find(|f| *f == token) {
                anyhow::bail!(
                    "'{flag}' is an option of `toolr {} {}`, not of `{}` — put it \
                     before the `{}` subcommand name.",
                    parent.group,
                    parent.name,
                    packed.name,
                    packed.name,
                );
            }
        }
    }
    Ok(())
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
///   `Count` falls back to `"flag"`. `OptionalPositional` and
///   `VarPositional` are both `"positional"` — every positional-style
///   arg (no CLI flag, ever) maps to `"positional"`, parameterised by
///   `nargs` for its exact arity; `"repeated"` is reserved for
///   genuinely keyword-style repeatable args, so `DispatchCommand.argv`
///   never has to guess whether a `"repeated"` arg has a flag. `FixedArity`
///   is `"optional"` when `long_flag` is `Some` (keyword-style), else
///   `"positional"` — mirroring the same split used everywhere else.
/// - `choices` ← `allowed_values` (empty → `None`).
/// - `metavar` ← `metadata.metavar`.
/// - `type_annotation` ← `type_annotation`.
/// - `nargs` ← `metadata.nargs`/`kind`: `"?"` for `OptionalPositional`,
///   the exact int for `FixedArity`, `"+"`/`"*"` for `Repeated`/
///   `VarPositional` when a single occurrence (or the single positional
///   slot) takes several values, `None` otherwise (e.g.
///   `action="append"`, native `*args`).
fn argument_to_arg_schema(arg: &Argument) -> ArgSchemaSpec {
    let kind = match arg.kind {
        ArgumentKind::Positional
        | ArgumentKind::OptionalPositional
        | ArgumentKind::VarPositional => "positional",
        ArgumentKind::Optional => "optional",
        ArgumentKind::Flag | ArgumentKind::Count => "flag",
        ArgumentKind::Repeated => "repeated",
        ArgumentKind::FixedArity => {
            if arg.long_flag.is_some() {
                "optional"
            } else {
                "positional"
            }
        }
    };
    let nargs = match (arg.kind, arg.metadata.nargs) {
        (ArgumentKind::OptionalPositional, _) => Some(serde_json::json!("?")),
        (_, Some(Nargs::Fixed(n))) => Some(serde_json::json!(n)),
        (_, Some(Nargs::Plus)) => Some(serde_json::json!("+")),
        (_, Some(Nargs::Star)) => Some(serde_json::json!("*")),
        (_, None) => None,
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
        nargs,
        long_flag: arg.long_flag.clone(),
    }
}

/// How much toolr and the runner print; a string on the runner spec's wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Verbosity {
    Quiet,
    #[default]
    Normal,
    Verbose,
}

impl Verbosity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Quiet => "quiet",
            Self::Normal => "normal",
            Self::Verbose => "verbose",
        }
    }
}

/// Plumbing struct bundling the "ToolR Options" flag
/// values for `build_spec`. Adding a new flag is a one-field change
/// instead of growing the `build_spec` argument list again.
#[derive(Debug, Clone)]
pub struct OutputOptions {
    pub verbosity: Verbosity,
    pub timestamps: bool,
    /// Python `logging` level name.
    pub log_level: String,
    pub default_timeout_secs: Option<f64>,
    pub default_no_output_timeout_secs: Option<f64>,
}

impl Default for OutputOptions {
    fn default() -> Self {
        Self {
            verbosity: Verbosity::Normal,
            timestamps: false,
            log_level: "INFO".to_string(),
            default_timeout_secs: None,
            default_no_output_timeout_secs: None,
        }
    }
}

/// Read the "ToolR Options" flags from the parsed matches
/// into an `OutputOptions`. Centralised so every command path produces
/// the same shape; missing flags fall back to `Default`.
pub fn output_options_from_matches(matches: &ArgMatches) -> OutputOptions {
    let mut opts = OutputOptions::default();
    if matches.get_flag("quiet") {
        opts.verbosity = Verbosity::Quiet;
        opts.log_level = "INFO".into();
    } else if matches.get_flag("debug") {
        opts.verbosity = Verbosity::Verbose;
        opts.log_level = "DEBUG".into();
    }
    // clap's `conflicts_with` makes `--timestamps` and `--no-timestamps` mutually exclusive.
    if matches.get_flag("timestamps") {
        opts.timestamps = true;
    }
    if let Some(secs) = matches.get_one::<f64>("timeout-secs").copied() {
        opts.default_timeout_secs = Some(secs);
    }
    if let Some(secs) = matches.get_one::<f64>("no-output-timeout-secs").copied() {
        opts.default_no_output_timeout_secs = Some(secs);
    }
    opts
}

fn extract_value(
    arg: &Argument,
    matches: &ArgMatches,
    bases: &PathBases,
) -> anyhow::Result<Option<Value>> {
    // Heterogeneous tuples are configured with `num_args(N)` and need
    // multi-value retrieval even when the manifest `kind` says
    // `Positional` or `Optional`. Route them through `extract_many`
    // before the kind-based switch.
    let is_tuple = matches!(
        arg.resolved_type.as_ref().map(unwrap_optional),
        Some(SupportedType::Tuple(_))
    );
    if is_tuple {
        return Ok(extract_optional_many(arg, matches));
    }
    Ok(match arg.kind {
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
        ArgumentKind::Positional | ArgumentKind::Optional | ArgumentKind::OptionalPositional => {
            return extract_scalar(arg, matches, bases);
        }
        ArgumentKind::Repeated => extract_optional_many(arg, matches),
        ArgumentKind::VarPositional => Some(Value::Array(extract_many(arg, matches))),
        ArgumentKind::FixedArity => {
            // Positional-style is always `required(true)`, so it's
            // always present when parsing succeeds — but keyword-style
            // is `required(false)` with no default, so an omitted
            // `--pair` must come through as `None` (skipped from the
            // args map entirely), not `Some([])`. An empty array would
            // make `DispatchCommand.argv` emit a bare `--pair` with
            // zero of its required N values.
            if matches.contains_id(arg.name.as_str()) {
                Some(Value::Array(extract_many(arg, matches)))
            } else {
                None
            }
        }
    })
}

/// Multi-value extraction for `list[T] | None` / `tuple[...] | None`
/// (kind `Repeated`, or `Tuple`'s early-return above). An omitted flag
/// has no clap value to collect, but the wire value must stay absent
/// too — so the Python function's own `= None` default runs — rather
/// than sending `[]`, which would silently turn `None` into an empty
/// collection (and, for a fixed-arity tuple, fail its own schema).
fn extract_optional_many(arg: &Argument, matches: &ArgMatches) -> Option<Value> {
    if !matches.contains_id(arg.name.as_str())
        && matches!(arg.resolved_type.as_ref(), Some(SupportedType::Optional(_)))
    {
        None
    } else {
        Some(Value::Array(extract_many(arg, matches)))
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

fn extract_scalar(
    arg: &Argument,
    matches: &ArgMatches,
    bases: &PathBases,
) -> anyhow::Result<Option<Value>> {
    let name = arg.name.as_str();
    if let Some(deferred) = deferred_path(arg) {
        return matches
            .get_one::<PathBuf>(name)
            .map(|raw| resolve_deferred(arg, deferred, matches, raw, bases))
            .transpose();
    }
    Ok(match scalar_element_type(arg) {
        Some(SupportedType::Int) => matches
            .get_one::<i64>(name)
            .map(|n| Value::Number((*n).into())),
        Some(SupportedType::Float) => matches
            .get_one::<f64>(name)
            .and_then(|f| serde_json::Number::from_f64(*f).map(Value::Number)),
        Some(SupportedType::Bool) => matches.get_one::<bool>(name).map(|b| Value::Bool(*b)),
        Some(ty) if ty.is_path() => matches
            .get_one::<std::path::PathBuf>(name)
            .map(|p| Value::String(p.to_string_lossy().into_owned())),
        // Everything else — strings (incl. enum / literal / email /
        // datetime / uuid / ip / unannotated) flows through as-is.
        _ => matches
            .get_one::<String>(name)
            .map(|s| Value::String(s.clone())),
    })
}

/// Where a deferred path value that failed its check came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PathValueOrigin {
    Typed,
    Env(String),
    Default { repo_root: PathBuf },
}

/// A deferred path value that failed its check. `dispatch` renders it as a
/// clap error against the command that owns `arg`; `Display` spells the arg
/// as its plain long flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PathValueError {
    /// The clap arg id (the manifest argument name).
    pub arg: String,
    pub value: String,
    pub origin: PathValueOrigin,
    pub reason: String,
}

impl PathValueError {
    /// The message with `shown` as the arg's spelling (clap's `--cfg <cfg>`).
    pub(crate) fn message(&self, shown: &str) -> String {
        let Self { value, reason, .. } = self;
        match &self.origin {
            PathValueOrigin::Typed => format!("invalid value '{value}' for '{shown}': {reason}"),
            PathValueOrigin::Env(var) => {
                format!("invalid value '{value}' for '{shown}' (from {var}): {reason}")
            }
            PathValueOrigin::Default { repo_root } => format!(
                "default '{value}' for '{shown}' (resolved from the repo root {}): {reason}",
                repo_root.display()
            ),
        }
    }
}

impl std::fmt::Display for PathValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message(&long_flag(&self.arg)))
    }
}

impl std::error::Error for PathValueError {}

/// Resolve a deferred path arg's raw value: from the repo root when it is
/// the default, from the cwd when typed or read from its env var.
fn resolve_deferred(
    arg: &Argument,
    deferred: DeferredPath,
    matches: &ArgMatches,
    raw: &Path,
    bases: &PathBases,
) -> anyhow::Result<Value> {
    let origin = match matches.value_source(arg.name.as_str()) {
        Some(ValueSource::DefaultValue) => PathValueOrigin::Default {
            repo_root: bases.repo_root.to_path_buf(),
        },
        Some(ValueSource::EnvVariable) => PathValueOrigin::Env(
            arg.metadata
                .env
                .clone()
                .expect("clap only reads an env var for an arg that names one"),
        ),
        _ => PathValueOrigin::Typed,
    };
    let base = match origin {
        PathValueOrigin::Default { .. } => bases.repo_root,
        _ => bases.cwd,
    };
    let path = deferred
        .resolve(raw, base)
        .map_err(|reason| PathValueError {
            arg: arg.name.clone(),
            value: raw.display().to_string(),
            origin,
            reason,
        })?;
    Ok(Value::String(path.to_string_lossy().into_owned()))
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
        Some(ty) if ty.is_path() => matches
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

/// True when `arg` is a bare `Path` argument whose value the user typed on
/// the command line (not a default) and that value is a relative path.
///
/// Type-driven: only a path type that reaches Python as typed counts. Every
/// other path type is resolved against the cwd here, so it never surprises,
/// and a `str` arg that merely looks path-like never counts. Source-precise:
/// `ValueSource::CommandLine` only, so a relative *default* never warns.
fn arg_has_relative_cli_path(arg: &Argument, matches: &ArgMatches) -> bool {
    if !scalar_element_type(arg).is_some_and(passes_path_as_typed) {
        return false;
    }
    let name = arg.name.as_str();
    if matches.value_source(name) != Some(ValueSource::CommandLine) {
        return false;
    }
    let is_many = matches!(
        arg.resolved_type.as_ref().map(unwrap_optional),
        Some(SupportedType::Tuple(_))
    ) || matches!(
        arg.kind,
        ArgumentKind::Repeated | ArgumentKind::VarPositional | ArgumentKind::FixedArity
    );
    if is_many {
        matches
            .get_many::<std::path::PathBuf>(name)
            .map(|it| it.into_iter().any(|p| !p.is_absolute()))
            .unwrap_or(false)
    } else {
        matches
            .get_one::<std::path::PathBuf>(name)
            .map(|p| !p.is_absolute())
            .unwrap_or(false)
    }
}

/// The one-line note to print iff the invocation cwd differs from `repo_root`
/// AND a path-typed argument was given a relative value on the command line.
/// Pure (no I/O) so it can be unit-tested against constructed matches.
pub(crate) fn relative_path_warning(
    cmd: &Command,
    matches: &ArgMatches,
    repo_root: &Path,
    cwd: &Path,
) -> Option<String> {
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    if canon(cwd) == canon(repo_root) {
        return None;
    }
    if !cmd
        .arguments
        .iter()
        .any(|arg| arg_has_relative_cli_path(arg, matches))
    {
        return None;
    }
    Some(format!(
        "toolr: note: commands run from the repo root ({}); relative path \
         arguments resolve from there, not {}",
        repo_root.display(),
        cwd.display(),
    ))
}

/// Side-effecting wrapper: print the note (if any) to stderr. Called by the
/// spec builders before the runner is spawned.
fn warn_relative_paths(cmd: &Command, matches: &ArgMatches, bases: &PathBases) {
    if let Some(msg) = relative_path_warning(cmd, matches, bases.repo_root, bases.cwd) {
        eprintln!("{msg}");
    }
}

#[cfg(test)]
mod tests;
