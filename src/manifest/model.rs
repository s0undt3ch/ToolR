//! Serde-derived types representing a loaded manifest.

use serde::{Deserialize, Serialize};

use crate::parser::{PathConstraints, SupportedType};

/// Current manifest schema version. Bump on breaking format changes.
pub const SCHEMA_VERSION: u32 = 1;

/// Top-level manifest document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    /// Hash over `tools/**/*.py` contents — used for fast freshness checks.
    pub static_hash: String,
    /// Hash over the installed package set (versions). Empty until Plan 6
    /// adds dynamic-layer support.
    #[serde(default)]
    pub dynamic_hash: String,
    pub groups: Vec<Group>,
    pub commands: Vec<Command>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    /// Lowercase group name (e.g. "ci"). Local-only; for nested groups
    /// this is the leaf name (`image`), not the full path
    /// (`docker.image`). Use `full_path()` to reconstruct the
    /// hierarchy.
    pub name: String,
    /// Short title shown in `--help`.
    pub title: String,
    /// Optional longer description.
    #[serde(default)]
    pub description: String,
    /// Parent group name (the parent's `full_path`). `None` for top-level
    /// groups. `Some("docker")` for `docker image`,
    /// `Some("docker.image")` for `docker image build`, etc.
    #[serde(default)]
    pub parent: Option<String>,
    /// Where this group entry came from.
    pub origin: Origin,
}

impl Group {
    /// Dotted full path including ancestor groups
    /// (e.g. `docker.image` for a group named `image` whose parent is
    /// `docker`). For top-level groups this equals `self.name`.
    pub fn full_path(&self) -> String {
        match &self.parent {
            Some(parent) => format!("{parent}.{}", self.name),
            None => self.name.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Command {
    /// Lowercase command name (e.g. "generate-build-matrix").
    pub name: String,
    /// Parent group name.
    pub group: String,
    /// Module path used by the runner to import.
    pub module: String,
    /// Python function name within that module.
    pub function: String,
    /// First line of the docstring; used in `--help` summaries.
    #[serde(default)]
    pub summary: String,
    /// Full description (rest of the docstring after the first line).
    #[serde(default)]
    pub description: String,
    /// Ordered list of arguments.
    pub arguments: Vec<Argument>,
    /// Top-level imports recorded by the static parser (used by Plan 7).
    #[serde(default)]
    pub imports: Vec<String>,
    /// Where this command entry came from.
    pub origin: Origin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Argument {
    pub name: String,
    pub kind: ArgumentKind,
    #[serde(default)]
    pub help: String,
    /// String-encoded default. `None` means required.
    #[serde(default)]
    pub default: Option<String>,
    /// Argument's type annotation as written in source (best-effort).
    /// Kept for `--help` rendering and diagnostics; the structured
    /// equivalent the CLI builder consumes lives on `resolved_type`.
    #[serde(default)]
    pub type_annotation: Option<String>,
    /// Resolved supported type — drives the per-type clap value_parser
    /// and how `extract_value` shapes the wire payload. `None` means
    /// "no type info available" (legacy / third-party fragments).
    #[serde(default)]
    pub resolved_type: Option<SupportedType>,
    /// For Literal[...] / Enum-backed args, the allowed value strings.
    #[serde(default)]
    pub allowed_values: Vec<String>,
    /// Path-constraint metadata harvested from
    /// `Annotated[Path, arg(must_exist=True, ...)]`. Applied by the
    /// path value-parsers in `src/bin/toolr/value_parsers.rs`. `None`
    /// when no constraint flags were set; ignored for non-path types.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_constraints: Option<PathConstraints>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArgumentKind {
    /// Single required positional value (`def f(ctx, name: str)`).
    Positional,
    /// Single optional keyword (`--name VALUE`, with a default).
    Optional,
    /// No-value boolean keyword (`--verbose`, `bool = False`).
    Flag,
    /// Repeatable keyword that appends each occurrence
    /// (`def f(ctx, items: list[str] = [])` → `--items a --items b`).
    Repeated,
    /// Variadic trailing positional (`def f(ctx, *files: str)` → `toolr ... a.py b.py`).
    VarPositional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Static,
    Dynamic,
}
