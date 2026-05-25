//! Resolve a Python parameter annotation to a [`SupportedType`].
//!
//! Toolr supports an explicit, opinionated set of parameter types. Any
//! annotation outside that set is rejected at manifest build time with
//! a clear pointer to `toolr.types` as the extension namespace.
//!
//! The resolver is name-based — it inspects the textual annotation as
//! parsed by `ruff_python_parser` plus a small [`TypeImports`] table
//! that tracks which symbols in this module were imported from
//! `toolr.types`. That lets us resolve `from toolr.types import
//! ResolvedPath as RP` style aliases without doing a full symbol-table
//! pass over the file.

mod arg_metadata;
mod imports;
mod literals;
mod path_constraints;
mod resolve;
mod supported;

pub use arg_metadata::extract_arg_metadata;
pub use imports::{SourcesImports, TypeImports};
pub use path_constraints::{PathConstraints, extract_path_constraints};
pub use resolve::{resolve, resolve_arguments};
pub use supported::{SupportedType, TypeResolutionError, UnsupportedType};

use ruff_python_ast::{Expr, ExprCall};

/// Predicate shared between the path-constraints and arg-metadata
/// extractors: is this call expression a `toolr.arg(...)` call (or
/// `<alias>.arg(...)` after aliasing)? The two extractors live in
/// sibling modules but both need to filter `Annotated[...]` elements
/// down to just the toolr-flavoured ones.
pub(super) fn is_toolr_arg_call(call: &ExprCall) -> bool {
    match call.func.as_ref() {
        Expr::Name(n) => n.id.as_str() == "arg",
        Expr::Attribute(a) => a.attr.as_str() == "arg",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::resolve::resolve_toolr_types_name;
    use crate::parser::parse_python_file;
    use crate::parser::symbols::{ArgSectionTable, EnumTable, TypeAliasTable};
    use ruff_python_ast::{ModModule, Stmt};
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn module(src: &str) -> ModModule {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(src.as_bytes()).unwrap();
        parse_python_file(f.path()).unwrap()
    }

    fn first_annotation(src: &str) -> (ModModule, Expr) {
        let m = module(src);
        for stmt in &m.body {
            if let Stmt::FunctionDef(func) = stmt {
                let p = &func.parameters.args[0];
                if let Some(ann) = p.parameter.annotation.as_deref() {
                    return (m.clone(), ann.clone());
                }
            }
        }
        panic!("no annotated function");
    }

    #[test]
    fn primitives_resolve() {
        let (_, ann) = first_annotation("def f(x: int): pass\n");
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &TypeImports::default(), &TypeAliasTable::default()).unwrap(),
            SupportedType::Int
        );
        let (_, ann) = first_annotation("def f(x: float): pass\n");
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &TypeImports::default(), &TypeAliasTable::default()).unwrap(),
            SupportedType::Float
        );
        let (_, ann) = first_annotation("def f(x: bool): pass\n");
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &TypeImports::default(), &TypeAliasTable::default()).unwrap(),
            SupportedType::Bool
        );
        let (_, ann) = first_annotation("def f(x: str): pass\n");
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &TypeImports::default(), &TypeAliasTable::default()).unwrap(),
            SupportedType::Str
        );
    }

    #[test]
    fn bare_path_name_is_supported() {
        let (_, ann) = first_annotation("def f(x: Path): pass\n");
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &TypeImports::default(), &TypeAliasTable::default()).unwrap(),
            SupportedType::Path
        );
    }

    #[test]
    fn pathlib_path_attribute_is_supported() {
        let (_, ann) = first_annotation("def f(x: pathlib.Path): pass\n");
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &TypeImports::default(), &TypeAliasTable::default()).unwrap(),
            SupportedType::Path
        );
    }

    #[test]
    fn toolr_types_resolved_path_via_from_import() {
        let src = "from toolr.types import ResolvedPath\ndef f(x: ResolvedPath): pass\n";
        let m = module(src);
        let (_, ann) = first_annotation(src);
        let imports = TypeImports::from_module(&m);
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &imports, &TypeAliasTable::default()).unwrap(),
            SupportedType::ResolvedPath
        );
    }

    #[test]
    fn toolr_types_via_alias() {
        let src = "from toolr.types import ResolvedPath as RP\ndef f(x: RP): pass\n";
        let m = module(src);
        let (_, ann) = first_annotation(src);
        let imports = TypeImports::from_module(&m);
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &imports, &TypeAliasTable::default()).unwrap(),
            SupportedType::ResolvedPath
        );
    }

    #[test]
    fn toolr_types_via_module_import() {
        let src = "import toolr.types\ndef f(x: toolr.types.AbsolutePath): pass\n";
        let m = module(src);
        let (_, ann) = first_annotation(src);
        let imports = TypeImports::from_module(&m);
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &imports, &TypeAliasTable::default()).unwrap(),
            SupportedType::AbsolutePath
        );
    }

    #[test]
    fn unknown_dotted_name_errors_with_pointer_to_toolr_types() {
        let (_, ann) = first_annotation("def f(x: datetime.datetime): pass\n");
        let err =
            resolve(&ann, &EnumTable::default(), &TypeImports::default(), &TypeAliasTable::default()).expect_err("should fail");
        let msg = err.to_string();
        assert!(msg.contains("datetime.datetime"), "msg was: {msg}");
        assert!(msg.contains("toolr.types"), "msg was: {msg}");
    }

    #[test]
    fn list_of_int_resolves() {
        let (_, ann) = first_annotation("def f(x: list[int]): pass\n");
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &TypeImports::default(), &TypeAliasTable::default()).unwrap(),
            SupportedType::List(Box::new(SupportedType::Int))
        );
    }

    #[test]
    fn tuple_str_int_resolves_heterogeneous() {
        let (_, ann) = first_annotation("def f(x: tuple[str, int]): pass\n");
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &TypeImports::default(), &TypeAliasTable::default()).unwrap(),
            SupportedType::Tuple(vec![SupportedType::Str, SupportedType::Int])
        );
    }

    #[test]
    fn literal_resolves_string_values() {
        let (_, ann) = first_annotation(
            "from typing import Literal\ndef f(x: Literal[\"a\", \"b\"]): pass\n",
        );
        let SupportedType::Literal(values) =
            resolve(&ann, &EnumTable::default(), &TypeImports::default(), &TypeAliasTable::default()).unwrap()
        else {
            panic!("expected Literal");
        };
        assert_eq!(values, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn optional_via_bin_or_with_none() {
        let (_, ann) = first_annotation("def f(x: int | None): pass\n");
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &TypeImports::default(), &TypeAliasTable::default()).unwrap(),
            SupportedType::Optional(Box::new(SupportedType::Int))
        );
    }

    /// Pin every `toolr.types.X` name the rust side knows about.
    ///
    /// The python-side companion lives at `tests/test_types_module.py`
    /// (`EXPECTED_TOOLR_TYPES_NAMES`). Anything added in one place
    /// without the other will break this test or its python twin,
    /// so the public surface can't silently drift.
    #[test]
    fn toolr_types_names_match_python_surface() {
        let names = [
            "AbsolutePath",
            "Count",
            "Date",
            "DateTime",
            "Email",
            "IPv4",
            "IPv6",
            "ResolvedPath",
            "Time",
            "UUID",
            "Version",
        ];
        for name in names {
            assert!(
                resolve_toolr_types_name(name).is_ok(),
                "rust resolver doesn't know about `toolr.types.{name}` — \
                 add it to `resolve_toolr_types_name` or remove it from \
                 the EXPECTED_TOOLR_TYPES_NAMES list in \
                 tests/test_types_module.py"
            );
        }
        // Anything else should be rejected.
        for spurious in ["NotARealType", "Foo", "AbsolutePath2"] {
            assert!(
                resolve_toolr_types_name(spurious).is_err(),
                "rust resolver unexpectedly accepted `toolr.types.{spurious}`"
            );
        }
    }

    /// `CommitHash = Annotated[str | None, arg(aliases=["--sha"])]` —
    /// a module-level alias should resolve to its underlying base
    /// type (`Optional[Str]` here) when used as a parameter annotation.
    #[test]
    fn module_level_alias_to_annotated_optional_str_resolves() {
        let src = r#"
from typing import Annotated
from toolr import arg

CommitHash = Annotated[str | None, arg(aliases=["--sha", "--commit-sha"])]

def f(commit_sha: CommitHash): pass
"#;
        let m = module(src);
        let (_, ann) = first_annotation(src);
        let aliases = TypeAliasTable::from_module(&m);
        let resolved =
            resolve(&ann, &EnumTable::default(), &TypeImports::default(), &aliases).unwrap();
        assert_eq!(
            resolved,
            SupportedType::Optional(Box::new(SupportedType::Str))
        );
    }

    #[test]
    fn module_level_alias_to_list_of_primitive_resolves() {
        let src = r#"
HostList = list[str]

def f(hosts: HostList): pass
"#;
        let m = module(src);
        let (_, ann) = first_annotation(src);
        let aliases = TypeAliasTable::from_module(&m);
        let resolved =
            resolve(&ann, &EnumTable::default(), &TypeImports::default(), &aliases).unwrap();
        assert_eq!(resolved, SupportedType::List(Box::new(SupportedType::Str)));
    }

    #[test]
    fn cyclic_aliases_are_rejected_not_hung() {
        let src = r#"
A = B
B = A

def f(x: A): pass
"#;
        let m = module(src);
        let (_, ann) = first_annotation(src);
        let aliases = TypeAliasTable::from_module(&m);
        let err =
            resolve(&ann, &EnumTable::default(), &TypeImports::default(), &aliases)
                .expect_err("cycle must error");
        let msg = err.to_string();
        assert!(msg.contains("cyclic"), "got: {msg}");
    }

    #[test]
    fn enum_subclass_resolves_via_table() {
        let src = "from enum import StrEnum\n\nclass Mode(StrEnum):\n    FAST = \"fast\"\n    SLOW = \"slow\"\n\ndef f(x: Mode): pass\n";
        let m = module(src);
        let (_, ann) = first_annotation(src);
        let mut enums = EnumTable::default();
        enums.merge(EnumTable::from_module(&m));
        let resolved = resolve(&ann, &enums, &TypeImports::default(), &TypeAliasTable::default()).unwrap();
        assert_eq!(
            resolved,
            SupportedType::Enum {
                name: "Mode".into(),
                values: vec!["fast".into(), "slow".into()],
            }
        );
    }

    #[test]
    fn extract_arg_metadata_harvests_aliases_and_metavar() {
        let (_, ann) = first_annotation(
            "def f(x: Annotated[str, arg(aliases=[\"-n\", \"--also\"], metavar=\"NAME\")]): pass\n",
        );
        let md = extract_arg_metadata(&ann, &ArgSectionTable::default()).unwrap();
        assert_eq!(md.aliases, vec!["-n", "--also"]);
        assert_eq!(md.metavar.as_deref(), Some("NAME"));
    }

    #[test]
    fn extract_arg_metadata_harvests_env_and_hide_and_order() {
        let (_, ann) = first_annotation(
            "def f(x: Annotated[str, arg(env=\"X\", hide=True, display_order=5)]): pass\n",
        );
        let md = extract_arg_metadata(&ann, &ArgSectionTable::default()).unwrap();
        assert_eq!(md.env.as_deref(), Some("X"));
        assert!(md.hide);
        assert_eq!(md.display_order, Some(5));
    }

    #[test]
    fn extract_arg_metadata_harvests_conflicts_and_requires() {
        let (_, ann) = first_annotation(
            "def f(x: Annotated[bool, arg(conflicts_with=[\"verbose\"], requires=[\"flag\"])]): pass\n",
        );
        let md = extract_arg_metadata(&ann, &ArgSectionTable::default()).unwrap();
        assert_eq!(md.conflicts_with, vec!["verbose"]);
        assert_eq!(md.requires, vec!["flag"]);
    }

    #[test]
    fn extract_arg_metadata_resolves_help_section_from_table() {
        let src = r#"
LOGGING = arg_section("Logging Options", description="Control verbosity.")
def f(x: Annotated[bool, arg(help_section=LOGGING)]): pass
"#;
        let m = module(src);
        let sections = ArgSectionTable::from_module(&m);
        let (_, ann) = first_annotation(src);
        let md = extract_arg_metadata(&ann, &sections).unwrap();
        let section = md.help_section.unwrap();
        assert_eq!(section.title, "Logging Options");
        assert_eq!(section.description.as_deref(), Some("Control verbosity."));
    }

    #[test]
    fn extract_arg_metadata_inline_help_section_call() {
        let (_, ann) = first_annotation(
            "def f(x: Annotated[bool, arg(help_section=arg_section(\"Net\", description=\"...\"))]): pass\n",
        );
        let md = extract_arg_metadata(&ann, &ArgSectionTable::default()).unwrap();
        let section = md.help_section.unwrap();
        assert_eq!(section.title, "Net");
        assert_eq!(section.description.as_deref(), Some("..."));
    }

    #[test]
    fn extract_arg_metadata_bare_string_help_section() {
        let (_, ann) = first_annotation(
            "def f(x: Annotated[bool, arg(help_section=\"Logging\")]): pass\n",
        );
        let md = extract_arg_metadata(&ann, &ArgSectionTable::default()).unwrap();
        let section = md.help_section.unwrap();
        assert_eq!(section.title, "Logging");
        assert!(section.description.is_none());
    }

    #[test]
    fn path_constraints_extract_from_must_kwargs() {
        let (_, ann) = first_annotation(
            "def f(x: Annotated[Path, arg(must_exist=True, must_be_file=True)]): pass\n",
        );
        let pc = extract_path_constraints(&ann).unwrap();
        assert!(pc.must_exist);
        assert!(pc.must_be_file);
        assert!(!pc.must_be_dir);
    }

    #[test]
    fn count_resolves_to_supported_type() {
        let (_, ann) = first_annotation(
            "from toolr.types import Count\n\ndef f(x: Count): pass\n",
        );
        let src = "from toolr.types import Count\n\ndef f(x: Count): pass\n";
        let m = module(src);
        let imports = TypeImports::from_module(&m);
        let resolved =
            resolve(&ann, &EnumTable::default(), &imports, &TypeAliasTable::default()).unwrap();
        assert_eq!(resolved, SupportedType::Count);
    }
}
