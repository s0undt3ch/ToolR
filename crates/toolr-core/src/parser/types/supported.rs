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
    /// Enum subclass resolved via [`EnumTable`]. `module` is the
    /// dotted path of the module that actually declares the class
    /// (which may differ from the annotation's own module when the
    /// class was imported) — the Python runtime needs it to lazily
    /// import the real class at coercion time, independent of whether
    /// the annotation's own module has it bound in its globals (see
    /// `TYPE_CHECKING`-guarded imports).
    Enum {
        name: String,
        /// `#[serde(default)]` so a manifest built by an older toolr
        /// (no `module` field on this variant) still deserialises —
        /// non-breaking addition, no `SCHEMA_VERSION` bump needed per
        /// `manifest/model.rs`'s "bump on breaking format changes" rule.
        #[serde(default)]
        module: String,
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

    /// Doc-table row for this variant, matching the "Supported types"
    /// table in `docs/writing-commands/arguments.md`. Exhaustive with
    /// no `_` arm so a new variant fails to compile until documented.
    pub fn doc(&self) -> TypeDoc {
        match self {
            SupportedType::Int => TypeDoc {
                annotation: "int",
                validated_by: "clap",
                wire_format: "JSON number",
                python_receives: "`int`",
                note: "",
            },
            SupportedType::Float => TypeDoc {
                annotation: "float",
                validated_by: "clap",
                wire_format: "JSON number",
                python_receives: "`float`",
                note: "",
            },
            SupportedType::Bool => TypeDoc {
                annotation: "bool",
                validated_by: "clap",
                wire_format: "JSON bool",
                python_receives: "`bool`",
                note: "",
            },
            SupportedType::Str => TypeDoc {
                annotation: "str",
                validated_by: "none (passthrough)",
                wire_format: "JSON string",
                python_receives: "`str`",
                note: "",
            },
            SupportedType::Path => TypeDoc {
                annotation: "pathlib.Path",
                validated_by: "clap (custom parser)",
                wire_format: "string",
                python_receives: "`pathlib.Path`",
                note: "",
            },
            SupportedType::AbsolutePath => TypeDoc {
                annotation: "toolr.types.AbsolutePath",
                validated_by: "clap (absolutise vs cwd)",
                wire_format: "absolute string",
                python_receives: "`pathlib.Path`",
                note: "",
            },
            SupportedType::ResolvedPath => TypeDoc {
                annotation: "toolr.types.ResolvedPath",
                validated_by: "clap (`canonicalize()`)",
                wire_format: "resolved string",
                python_receives: "`pathlib.Path`",
                note: "",
            },
            SupportedType::DateTime => TypeDoc {
                annotation: "toolr.types.DateTime",
                validated_by: "clap (chrono RFC 3339)",
                wire_format: "string",
                python_receives: "`datetime.datetime`",
                note: "",
            },
            SupportedType::Date => TypeDoc {
                annotation: "toolr.types.Date",
                validated_by: "clap (chrono ISO date)",
                wire_format: "string",
                python_receives: "`datetime.date`",
                note: "",
            },
            SupportedType::Time => TypeDoc {
                annotation: "toolr.types.Time",
                validated_by: "clap (chrono ISO time)",
                wire_format: "string",
                python_receives: "`datetime.time`",
                note: "",
            },
            SupportedType::Uuid => TypeDoc {
                annotation: "toolr.types.UUID",
                validated_by: "clap (`uuid` crate)",
                wire_format: "string",
                python_receives: "`uuid.UUID`",
                note: "",
            },
            SupportedType::Ipv4 => TypeDoc {
                annotation: "toolr.types.IPv4",
                validated_by: "clap (`std::net::Ipv4Addr`)",
                wire_format: "string",
                python_receives: "`ipaddress.IPv4Address`",
                note: "",
            },
            SupportedType::Ipv6 => TypeDoc {
                annotation: "toolr.types.IPv6",
                validated_by: "clap (`std::net::Ipv6Addr`)",
                wire_format: "string",
                python_receives: "`ipaddress.IPv6Address`",
                note: "",
            },
            SupportedType::Email => TypeDoc {
                annotation: "toolr.types.Email",
                validated_by: "clap (`email_address` crate)",
                wire_format: "string",
                python_receives: "`str` (pre-validated)",
                note: "",
            },
            SupportedType::Version => TypeDoc {
                annotation: "toolr.types.Version",
                validated_by: "clap (`pep440_rs` crate)",
                wire_format: "string",
                python_receives: "`packaging.version.Version`",
                note: "",
            },
            SupportedType::Count => TypeDoc {
                annotation: "toolr.types.Count",
                validated_by: "clap (`ArgAction::Count`)",
                wire_format: "integer",
                python_receives: "`int`",
                note: "",
            },
            SupportedType::Literal(_) => TypeDoc {
                annotation: "Literal[\"a\", \"b\"]",
                validated_by: "clap (allowed-values)",
                wire_format: "string",
                python_receives: "`Literal` value",
                note: "",
            },
            SupportedType::Enum { .. } => TypeDoc {
                annotation: "Enum",
                validated_by: "clap (member values)",
                wire_format: "string",
                python_receives: "enum member",
                note: "subclass",
            },
            SupportedType::List(_) => TypeDoc {
                annotation: "list[T]",
                validated_by: "clap per-element",
                wire_format: "JSON array",
                python_receives: "`list[T]`",
                note: "(T above)",
            },
            SupportedType::Tuple(_) => TypeDoc {
                annotation: "tuple[T1, T2, …]",
                validated_by: "clap arity, msgspec per-slot",
                wire_format: "JSON array",
                python_receives: "`tuple[T1, T2]`",
                note: "",
            },
            SupportedType::Optional(_) => TypeDoc {
                annotation: "T | None",
                validated_by: "clap (`required=false`)",
                wire_format: "typed or absent",
                python_receives: "`T` or `None`",
                note: "",
            },
        }
    }

    /// The fieldless [`SupportedTypeKind`] of this variant. Exhaustive, so
    /// a new variant doesn't compile until it has a kind, and a new kind
    /// doesn't compile until [`SupportedTypeKind::representative`] covers it.
    /// For every step of adding a type, see "Adding a supported type" in
    /// `CONTRIBUTING.md`.
    pub fn kind(&self) -> SupportedTypeKind {
        match self {
            SupportedType::Str => SupportedTypeKind::Str,
            SupportedType::Int => SupportedTypeKind::Int,
            SupportedType::Float => SupportedTypeKind::Float,
            SupportedType::Bool => SupportedTypeKind::Bool,
            SupportedType::Path => SupportedTypeKind::Path,
            SupportedType::AbsolutePath => SupportedTypeKind::AbsolutePath,
            SupportedType::ResolvedPath => SupportedTypeKind::ResolvedPath,
            SupportedType::DateTime => SupportedTypeKind::DateTime,
            SupportedType::Date => SupportedTypeKind::Date,
            SupportedType::Time => SupportedTypeKind::Time,
            SupportedType::Uuid => SupportedTypeKind::Uuid,
            SupportedType::Ipv4 => SupportedTypeKind::Ipv4,
            SupportedType::Ipv6 => SupportedTypeKind::Ipv6,
            SupportedType::Email => SupportedTypeKind::Email,
            SupportedType::Version => SupportedTypeKind::Version,
            SupportedType::Count => SupportedTypeKind::Count,
            SupportedType::Literal(_) => SupportedTypeKind::Literal,
            SupportedType::Enum { .. } => SupportedTypeKind::Enum,
            SupportedType::List(_) => SupportedTypeKind::List,
            SupportedType::Tuple(_) => SupportedTypeKind::Tuple,
            SupportedType::Optional(_) => SupportedTypeKind::Optional,
        }
    }

    /// Every variant's doc row, in table order, for generating the
    /// "Supported types" doc table.
    pub fn catalogue() -> Vec<TypeDoc> {
        SupportedTypeKind::ALL
            .iter()
            .map(|kind| kind.representative().doc())
            .collect()
    }
}

/// Declares [`SupportedTypeKind`] and its `ALL` list from one list of
/// names, so the two can't drift apart.
macro_rules! supported_type_kinds {
    ($($kind:ident),+ $(,)?) => {
        /// Fieldless mirror of [`SupportedType`].
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum SupportedTypeKind {
            $($kind),+
        }

        impl SupportedTypeKind {
            /// Every kind, in "Supported types" table order.
            pub const ALL: &[SupportedTypeKind] = &[$(SupportedTypeKind::$kind),+];
        }
    };
}

supported_type_kinds!(
    Int,
    Float,
    Bool,
    Str,
    Path,
    AbsolutePath,
    ResolvedPath,
    DateTime,
    Date,
    Time,
    Uuid,
    Ipv4,
    Ipv6,
    Email,
    Version,
    Count,
    Literal,
    Enum,
    List,
    Tuple,
    Optional,
);

impl SupportedTypeKind {
    /// A value of this kind, for its doc row. Exhaustive, so a new kind
    /// doesn't compile until it has one.
    pub fn representative(self) -> SupportedType {
        match self {
            SupportedTypeKind::Int => SupportedType::Int,
            SupportedTypeKind::Float => SupportedType::Float,
            SupportedTypeKind::Bool => SupportedType::Bool,
            SupportedTypeKind::Str => SupportedType::Str,
            SupportedTypeKind::Path => SupportedType::Path,
            SupportedTypeKind::AbsolutePath => SupportedType::AbsolutePath,
            SupportedTypeKind::ResolvedPath => SupportedType::ResolvedPath,
            SupportedTypeKind::DateTime => SupportedType::DateTime,
            SupportedTypeKind::Date => SupportedType::Date,
            SupportedTypeKind::Time => SupportedType::Time,
            SupportedTypeKind::Uuid => SupportedType::Uuid,
            SupportedTypeKind::Ipv4 => SupportedType::Ipv4,
            SupportedTypeKind::Ipv6 => SupportedType::Ipv6,
            SupportedTypeKind::Email => SupportedType::Email,
            SupportedTypeKind::Version => SupportedType::Version,
            SupportedTypeKind::Count => SupportedType::Count,
            SupportedTypeKind::Literal => SupportedType::Literal(vec![]),
            SupportedTypeKind::Enum => SupportedType::Enum {
                name: String::new(),
                module: String::new(),
                values: vec![],
            },
            SupportedTypeKind::List => SupportedType::List(Box::new(SupportedType::Str)),
            SupportedTypeKind::Tuple => SupportedType::Tuple(vec![]),
            SupportedTypeKind::Optional => SupportedType::Optional(Box::new(SupportedType::Str)),
        }
    }
}

/// One row of the "Supported types" doc table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeDoc {
    pub annotation: &'static str,
    pub validated_by: &'static str,
    pub wire_format: &'static str,
    pub python_receives: &'static str,
    pub note: &'static str,
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
    /// `arg(<keyword>=...)` where `arg()` has no such parameter.
    UnknownArgKeyword {
        keyword: String,
        suggestion: Option<String>,
    },
    /// `arg(...)` called with a positional argument; it is keyword-only.
    PositionalArgArgument,
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
            Self::UnknownArgKeyword { keyword, suggestion } => {
                write!(f, "unknown `arg()` keyword `{keyword}`")?;
                match (super::arg_keywords::argparse_hint(keyword), suggestion) {
                    (Some(hint), _) => write!(f, "; {hint}"),
                    (None, Some(s)) => write!(f, " (did you mean `{s}`?)"),
                    (None, None) => Ok(()),
                }
            }
            Self::PositionalArgArgument => write!(f, "`arg()` takes keyword arguments only"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_covers_every_toolr_types_name() {
        // Same list as `toolr_types_names_match_python_surface` in mod.rs,
        // which is itself pinned to tests/test_types_module.py.
        let names = [
            "AbsolutePath", "Count", "Date", "DateTime", "Email", "IPv4",
            "IPv6", "ResolvedPath", "Time", "UUID", "Version",
        ];
        let annotations: Vec<&str> =
            SupportedType::catalogue().iter().map(|d| d.annotation).collect();
        for name in names {
            let want = format!("toolr.types.{name}");
            assert!(
                annotations.iter().any(|a| *a == want),
                "catalogue has no row for `{want}`",
            );
        }
    }

    #[test]
    fn every_kind_has_a_representative_of_that_kind() {
        for kind in SupportedTypeKind::ALL {
            assert_eq!(kind.representative().kind(), *kind);
        }
        assert_eq!(SupportedType::catalogue().len(), SupportedTypeKind::ALL.len());
    }

    #[test]
    fn catalogue_rows_are_unique_and_complete() {
        let rows = SupportedType::catalogue();
        let mut seen = std::collections::BTreeSet::new();
        for row in &rows {
            assert!(seen.insert(row.annotation), "duplicate row {}", row.annotation);
            for field in [row.validated_by, row.wire_format, row.python_receives] {
                assert!(!field.is_empty(), "empty column in {}", row.annotation);
            }
        }
        for primitive in ["int", "float", "bool", "str", "pathlib.Path"] {
            assert!(seen.contains(primitive), "missing {primitive}");
        }
    }

    #[test]
    fn enum_and_list_rows_carry_the_expected_note() {
        let rows = SupportedType::catalogue();
        let enum_row = rows.iter().find(|d| d.annotation == "Enum").unwrap();
        assert_eq!(enum_row.note, "subclass");
        let list_row = rows.iter().find(|d| d.annotation == "list[T]").unwrap();
        assert_eq!(list_row.note, "(T above)");
    }

}
