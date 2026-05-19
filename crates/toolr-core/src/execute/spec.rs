//! Serde-derived types matching the Python runner's spec schema.
//!
//! Wire format: JSON. The Python side decodes with
//! `msgspec.json.decode(data, type=RunnerSpec)`. Field names and types
//! here must stay in lock-step with `crates/toolr-py/python/toolr/_runner.py`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Schema version. Must match `toolr._runner.SCHEMA_VERSION` exactly.
pub const RUNNER_SCHEMA_VERSION: u32 = 1;

/// Reduced view of `toolr.Context` reconstructable from JSON.
///
/// `Eq` is intentionally not derived: the `default_*_timeout_secs`
/// fields are `Option<f64>`, and floats are only `PartialEq`. Tests
/// compare with `assert_eq!` via `PartialEq`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextSpec {
    pub repo_root: String,
    /// One of "quiet", "normal", "verbose".
    pub verbosity: String,
    pub timestamps: bool,
    /// Python `logging` level name, e.g. "INFO".
    pub log_level: String,
    /// Default for `ctx.run(timeout_secs=...)` when the per-call value
    /// is `None`. `None` means no default — `ctx.run` doesn't time out
    /// unless the caller asks for it. Plumbed from
    /// `toolr --timeout-secs N` / `--timeout N`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_timeout_secs: Option<f64>,
    /// Default for `ctx.run(no_output_timeout_secs=...)` when the
    /// per-call value is `None`. Plumbed from
    /// `toolr --no-output-timeout-secs N` / `--nots N`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_no_output_timeout_secs: Option<f64>,
}

/// Top-level execution spec written to `$TOOLR_SPEC_FILE`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionSpec {
    pub schema_version: u32,
    pub group: String,
    pub command: String,
    pub module: String,
    pub function: String,
    /// Argument map: name → JSON value (string / number / bool / null).
    /// We use `serde_json::Value` (via `BTreeMap` for deterministic
    /// ordering in tests) so callers can pass parsed clap matches through
    /// without per-arg type juggling on the Rust side.
    pub args: BTreeMap<String, serde_json::Value>,
    pub context: ContextSpec,
    /// Set when the matched command is a dispatched leaf — the runner
    /// must construct a `DispatchCommand` from this payload and call
    /// `toolr._runner.invoke_dispatcher` instead of running `args` as
    /// a regular command call. `module` / `function` point at the
    /// parent dispatcher function; `args` carries the parent's own
    /// kwargs (typically empty); this struct carries the leaf's name,
    /// its packed args, and the schema needed by `DispatchCommand`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatch: Option<DispatchSpec>,
}

/// Dispatch payload conveyed to the Python runner.
///
/// Mirrors the keyword arguments of `toolr._runner.invoke_dispatcher`:
/// `command` → `child_name`, `command_args` → `child_args`,
/// `schema` → `child_schema`. The serialised `schema` shape mirrors
/// `toolr.sources.CommandSchema` so `msgspec.convert` reconstructs the
/// Python frozen struct one-shot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DispatchSpec {
    /// Leaf command name (e.g. `"migrate"`).
    pub command: String,
    /// Leaf's argument values, keyed by name. Same shape and encoding
    /// as `ExecutionSpec::args` for a normal command call — flags are
    /// JSON bools, counts are numbers, missing optionals are absent.
    pub command_args: BTreeMap<String, serde_json::Value>,
    /// Wire-shape of `toolr.sources.CommandSchema` for the leaf.
    pub schema: CommandSchemaSpec,
}

/// Wire-shape for `toolr.sources.CommandSchema`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandSchemaSpec {
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub description: String,
    pub arguments: Vec<ArgSchemaSpec>,
}

/// Wire-shape for `toolr.sources.ArgSchema`.
///
/// Field order and default-skipping rules match the Python frozen
/// struct so `msgspec.convert` reconstructs it field-for-field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArgSchemaSpec {
    pub name: String,
    /// One of `"positional" | "optional" | "flag" | "repeated"`.
    pub kind: String,
    #[serde(default)]
    pub help: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metavar: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_annotation: Option<String>,
    /// `"*" | "+" | "?"` or an integer; serde encodes whichever variant.
    /// Always `None` from the current Rust scanner — argparse-equivalent
    /// nargs information isn't tracked yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nargs: Option<serde_json::Value>,
}

impl ExecutionSpec {
    /// Construct a default-shaped spec with empty args and a quiet/normal
    /// context. Most callers use the builder pattern in the `toolr` binary
    /// crate's `execute_build::build_spec`; this is for tests.
    #[must_use]
    pub fn new(
        group: impl Into<String>,
        command: impl Into<String>,
        module: impl Into<String>,
        function: impl Into<String>,
        repo_root: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: RUNNER_SCHEMA_VERSION,
            group: group.into(),
            command: command.into(),
            module: module.into(),
            function: function.into(),
            args: BTreeMap::new(),
            context: ContextSpec {
                repo_root: repo_root.into(),
                verbosity: "normal".into(),
                timestamps: false,
                log_level: "INFO".into(),
                default_timeout_secs: None,
                default_no_output_timeout_secs: None,
            },
            dispatch: None,
        }
    }
}

#[cfg(test)]
mod tests {
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
        assert!(json.contains("\"schema_version\":1"), "got: {json}");
        assert!(json.contains("\"group\":\"ci\""), "got: {json}");
        assert!(json.contains("\"command\":\"hello\""), "got: {json}");
        assert!(json.contains("\"repo_root\":\"/repo\""), "got: {json}");
        assert!(json.contains("\"verbosity\":\"normal\""), "got: {json}");
    }

    #[test]
    fn schema_version_constant_is_1() {
        assert_eq!(RUNNER_SCHEMA_VERSION, 1);
    }
}
