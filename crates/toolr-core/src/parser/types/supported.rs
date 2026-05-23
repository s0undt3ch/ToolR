//! The set of annotation shapes toolr recognises end-to-end, plus the
//! error types the resolver produces when an annotation falls outside
//! that set.

use serde::{Deserialize, Serialize};

/// Every annotation shape toolr recognises end-to-end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum SupportedType {
    Str,
    Int,
    Float,
    Bool,
    /// `pathlib.Path` — string passes through unchanged.
    Path,
    /// `toolr.types.AbsolutePath` — absolutised against cwd, no fs check.
    AbsolutePath,
    /// `toolr.types.ResolvedPath` — canonicalised, must exist.
    ResolvedPath,
    DateTime,
    Date,
    Time,
    Uuid,
    Ipv4,
    Ipv6,
    /// `toolr.types.Email` — RFC-5321-ish address (single `local@domain`
    /// pair, no comments / display name). Runtime value is `str`.
    Email,
    /// `toolr.types.Version` — PEP 440 version string. Validated by
    /// the `pep440_rs` crate (the same parser uv uses). Runtime value
    /// is `packaging.version.Version`.
    Version,
    /// `toolr.types.Count` — int counter accumulating repeated flags
    /// (`-vvv` → 3). Wired via clap `ArgAction::Count`. Runtime value
    /// is :class:`int`.
    Count,
    /// `Literal["a", "b"]` — string validated against the allowed set.
    Literal(Vec<String>),
    /// Enum subclass resolved via [`EnumTable`].
    Enum {
        name: String,
        values: Vec<String>,
    },
    /// `list[T]` / `List[T]` — repeated keyword that appends.
    List(Box<SupportedType>),
    /// Heterogeneous `tuple[T1, T2, ...]`.
    Tuple(Vec<SupportedType>),
    /// `T | None` / `Optional[T]` — same as T at the CLI surface, but
    /// the parameter is not required.
    Optional(Box<SupportedType>),
}

impl SupportedType {
    /// Strip an `Optional(T)` wrapper to `T`, returning whether the
    /// original was wrapped. Helps the CLI-build path treat
    /// `T | None` as "T with required=false".
    pub fn unwrap_optional(self) -> (Self, bool) {
        match self {
            SupportedType::Optional(inner) => (*inner, true),
            other => (other, false),
        }
    }
}

/// Reasons annotation resolution can fail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnsupportedType {
    /// A bare name we don't recognise (e.g. `datetime.datetime` without
    /// going through `toolr.types`).
    UnknownName(String),
    /// `Annotated[T, ...]` wrapper — supported, but the inner T was
    /// unsupported (we surface the inner error).
    Inner(Box<UnsupportedType>),
    /// `T | None` with both sides not-None (we only support
    /// `T | None`, not arbitrary unions).
    UnsupportedUnion(String),
    /// A subscript shape we don't handle (e.g. `dict[K, V]`).
    UnsupportedShape(String),
}

/// A typed-annotation rejection with full context for diagnostic output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeResolutionError {
    /// Dotted module path the offending command lives in (`tools.foo.bar`).
    pub module: String,
    /// Python function name of the command.
    pub function: String,
    /// Parameter name on that function.
    pub argument: String,
    /// Textual rendering of the unsupported annotation as it appeared
    /// in source — for the user-facing message.
    pub annotation: String,
    /// The underlying [`UnsupportedType`] reason.
    pub reason: UnsupportedType,
}

impl std::fmt::Display for TypeResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{module}::{function} argument `{arg}` (annotated `{annotation}`): {reason}",
            module = self.module,
            function = self.function,
            arg = self.argument,
            annotation = self.annotation,
            reason = self.reason,
        )
    }
}

impl std::fmt::Display for UnsupportedType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownName(n) => write!(
                f,
                "type `{n}` is not supported. Use a primitive (int, float, bool, str, pathlib.Path), \
                 a Literal[...] or Enum, or one of the aliases under `toolr.types`."
            ),
            Self::Inner(inner) => inner.fmt(f),
            Self::UnsupportedUnion(s) => write!(f, "unsupported union `{s}`; only `T | None` is recognised."),
            Self::UnsupportedShape(s) => write!(f, "unsupported generic shape `{s}`."),
        }
    }
}
