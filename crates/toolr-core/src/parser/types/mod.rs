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
mod supported;

pub use arg_metadata::extract_arg_metadata;
pub use imports::{SourcesImports, TypeImports};
pub use path_constraints::{PathConstraints, extract_path_constraints};
pub use supported::{SupportedType, TypeResolutionError, UnsupportedType};

use ruff_python_ast::{Expr, ExprCall, StmtFunctionDef};

use super::symbols::{ArgSectionTable, EnumTable, TypeAliasTable};
use crate::manifest::{ArgMetadata, Argument, ArgumentKind};



/// Walk a function's parameters and populate `resolved_type` on each
/// matching [`Argument`]. Unsupported annotations are pushed to `errors`
/// with `module` / function-name context, and the corresponding
/// `Argument.resolved_type` stays `None`.
#[allow(clippy::too_many_arguments)] // every parameter carries a distinct compile-time context; bundling would hide call-site intent.
pub fn resolve_arguments(
    func: &StmtFunctionDef,
    arguments: &mut [Argument],
    enums: &EnumTable,
    type_imports: &TypeImports,
    sources: &SourcesImports,
    aliases: &TypeAliasTable,
    sections: &ArgSectionTable,
    module: &str,
    errors: &mut Vec<TypeResolutionError>,
) {
    let params = func.parameters.as_ref();
    let function = func.name.as_str().to_string();
    let mut i = 0usize;
    // First positional in the signature is `ctx`; skip it.
    for p in params.args.iter().skip(1) {
        resolve_one(
            p.parameter.annotation.as_deref(),
            &mut arguments[i],
            enums,
            type_imports,
            aliases,
            sections,
            module,
            &function,
            errors,
        );
        i += 1;
    }
    if let Some(vararg) = params.vararg.as_deref() {
        resolve_one(
            vararg.annotation.as_deref(),
            &mut arguments[i],
            enums,
            type_imports,
            aliases,
            sections,
            module,
            &function,
            errors,
        );
        i += 1;
    }
    // Kwarg walk must stay in lockstep with `extract_arguments`, which
    // already dropped any `DispatchCommand`-annotated kwargs from the
    // arguments slice. Skip the same params here so we don't run the
    // type resolver against a runtime injection slot.
    for p in &params.kwonlyargs {
        if let Some(ann) = p.parameter.annotation.as_deref() {
            if sources.is_dispatch_command(ann) {
                continue;
            }
        }
        resolve_one(
            p.parameter.annotation.as_deref(),
            &mut arguments[i],
            enums,
            type_imports,
            aliases,
            sections,
            module,
            &function,
            errors,
        );
        i += 1;
    }
}

#[allow(clippy::too_many_arguments)] // contextual fields each have distinct meaning; bundling would obscure the call sites.
fn resolve_one(
    annotation: Option<&Expr>,
    arg: &mut Argument,
    enums: &EnumTable,
    type_imports: &TypeImports,
    aliases: &TypeAliasTable,
    sections: &ArgSectionTable,
    module: &str,
    function: &str,
    errors: &mut Vec<TypeResolutionError>,
) {
    let Some(expr) = annotation else {
        // Bare-typed argument with no annotation — leave resolved_type
        // empty so the CLI builder falls back to string semantics. The
        // python registry still imposes its own runtime checks.
        return;
    };
    // Path constraints come from `Annotated[T, arg(...)]` metadata —
    // either directly on the parameter, or by following a module-level
    // type alias (e.g. `Foo = Annotated[Path, arg(must_exist=True)]`).
    arg.path_constraints = extract_path_constraints(expr)
        .or_else(|| follow_alias_for_path_constraints(expr, aliases));
    // Same drill for the broader clap metadata (aliases, conflicts,
    // env, help_section, ...). One harvest pass through every
    // `Annotated[T, arg(...)]` call on the parameter, optionally via a
    // module-level type alias.
    if let Some(md) = extract_arg_metadata(expr, sections)
        .or_else(|| follow_alias_for_arg_metadata(expr, aliases, sections))
    {
        arg.metadata = md;
    }
    match resolve(expr, enums, type_imports, aliases) {
        Ok(ty) => {
            // Post-resolution kind override: `toolr.types.Count` is the
            // only type that flips the inferred kind, since the type
            // itself is semantically a counting flag rather than a
            // value-taking one. Done here (not in the syntactic
            // classifier) so it also fires through `Annotated[Count, ...]`
            // and module-level aliases.
            if is_count_type(&ty) {
                arg.kind = ArgumentKind::Count;
            }
            arg.resolved_type = Some(ty);
        }
        Err(reason) => errors.push(TypeResolutionError {
            module: module.to_string(),
            function: function.to_string(),
            argument: arg.name.clone(),
            annotation: arg.type_annotation.clone().unwrap_or_default(),
            reason,
        }),
    }
}

fn is_count_type(ty: &SupportedType) -> bool {
    matches!(ty, SupportedType::Count)
        || matches!(ty, SupportedType::Optional(inner) if matches!(inner.as_ref(), SupportedType::Count))
}

fn follow_alias_for_path_constraints(
    expr: &Expr,
    aliases: &TypeAliasTable,
) -> Option<PathConstraints> {
    let Expr::Name(name) = expr else { return None };
    let aliased = aliases.lookup(name.id.as_str())?;
    extract_path_constraints(aliased)
}

fn follow_alias_for_arg_metadata(
    expr: &Expr,
    aliases: &TypeAliasTable,
    sections: &ArgSectionTable,
) -> Option<ArgMetadata> {
    let Expr::Name(name) = expr else { return None };
    let aliased = aliases.lookup(name.id.as_str())?;
    extract_arg_metadata(aliased, sections)
}


pub(super) fn is_toolr_arg_call(call: &ExprCall) -> bool {
    match call.func.as_ref() {
        Expr::Name(n) => n.id.as_str() == "arg",
        Expr::Attribute(a) => a.attr.as_str() == "arg",
        _ => false,
    }
}

/// Resolve a parameter annotation to a [`SupportedType`].
pub fn resolve(
    annotation: &Expr,
    enums: &EnumTable,
    imports: &TypeImports,
    aliases: &TypeAliasTable,
) -> Result<SupportedType, UnsupportedType> {
    resolve_inner(annotation, enums, imports, aliases, &mut Vec::new())
}

/// `seen` tracks names currently being expanded to break alias cycles
/// (`A = B; B = A`). Capped depth gives a second line of defence.
const MAX_ALIAS_DEPTH: usize = 16;

fn resolve_inner(
    annotation: &Expr,
    enums: &EnumTable,
    imports: &TypeImports,
    aliases: &TypeAliasTable,
    seen: &mut Vec<String>,
) -> Result<SupportedType, UnsupportedType> {
    if seen.len() >= MAX_ALIAS_DEPTH {
        return Err(UnsupportedType::UnsupportedShape(
            "type alias chain too deep".to_string(),
        ));
    }
    match annotation {
        Expr::Name(n) => resolve_name(n.id.as_str(), enums, imports, aliases, seen),
        Expr::Attribute(_) => {
            if let Some(toolr_name) = imports.resolve_attribute(annotation) {
                return resolve_toolr_types_name(toolr_name);
            }
            // `pathlib.Path` (or its aliases) — the only stdlib
            // dotted-name we natively recognise. Anything else is a
            // diagnostic.
            let rendered = render_attribute(annotation);
            if rendered == "pathlib.Path" {
                return Ok(SupportedType::Path);
            }
            Err(UnsupportedType::UnknownName(rendered))
        }
        Expr::Subscript(sub) => resolve_subscript(sub, enums, imports, aliases, seen),
        Expr::BinOp(op) => resolve_bin_op(op, enums, imports, aliases, seen),
        _ => Err(UnsupportedType::UnknownName(annotation_to_label(annotation))),
    }
}

fn resolve_name(
    name: &str,
    enums: &EnumTable,
    imports: &TypeImports,
    aliases: &TypeAliasTable,
    seen: &mut Vec<String>,
) -> Result<SupportedType, UnsupportedType> {
    if let Some(canonical) = imports.resolve_direct(name) {
        return resolve_toolr_types_name(canonical);
    }
    match name {
        "str" => Ok(SupportedType::Str),
        "int" => Ok(SupportedType::Int),
        "float" => Ok(SupportedType::Float),
        "bool" => Ok(SupportedType::Bool),
        "Path" => Ok(SupportedType::Path),
        _ => {
            if let Some(values) = enums.lookup(name) {
                return Ok(SupportedType::Enum {
                    name: name.to_string(),
                    values: values.to_vec(),
                });
            }
            // Module-level type alias fallback: `Foo = Annotated[T, ...]`
            // / `Bar = str | None` / `HostList = list[str]`. Recurse
            // with a guard against cyclic chains.
            if let Some(aliased) = aliases.lookup(name) {
                if seen.iter().any(|n| n == name) {
                    return Err(UnsupportedType::UnsupportedShape(format!(
                        "cyclic type alias `{name}`"
                    )));
                }
                seen.push(name.to_string());
                let result = resolve_inner(aliased, enums, imports, aliases, seen);
                seen.pop();
                return result;
            }
            Err(UnsupportedType::UnknownName(name.to_string()))
        }
    }
}

fn resolve_toolr_types_name(name: &str) -> Result<SupportedType, UnsupportedType> {
    match name {
        "DateTime" => Ok(SupportedType::DateTime),
        "Date" => Ok(SupportedType::Date),
        "Time" => Ok(SupportedType::Time),
        "UUID" => Ok(SupportedType::Uuid),
        "IPv4" => Ok(SupportedType::Ipv4),
        "IPv6" => Ok(SupportedType::Ipv6),
        "AbsolutePath" => Ok(SupportedType::AbsolutePath),
        "ResolvedPath" => Ok(SupportedType::ResolvedPath),
        "Email" => Ok(SupportedType::Email),
        "Version" => Ok(SupportedType::Version),
        "Count" => Ok(SupportedType::Count),
        other => Err(UnsupportedType::UnknownName(format!("toolr.types.{other}"))),
    }
}

fn resolve_subscript(
    sub: &ruff_python_ast::ExprSubscript,
    enums: &EnumTable,
    imports: &TypeImports,
    aliases: &TypeAliasTable,
    seen: &mut Vec<String>,
) -> Result<SupportedType, UnsupportedType> {
    let head = match sub.value.as_ref() {
        Expr::Name(n) => n.id.as_str(),
        Expr::Attribute(a) => a.attr.as_str(),
        _ => return Err(UnsupportedType::UnsupportedShape(annotation_to_label(sub.value.as_ref()))),
    };
    match head {
        "Literal" => {
            let values = literal_string_values(sub.slice.as_ref());
            if values.is_empty() {
                Err(UnsupportedType::UnsupportedShape("Literal[...]".into()))
            } else {
                Ok(SupportedType::Literal(values))
            }
        }
        "list" | "List" => {
            let inner = resolve_inner(sub.slice.as_ref(), enums, imports, aliases, seen)
                .map_err(|e| UnsupportedType::Inner(Box::new(e)))?;
            Ok(SupportedType::List(Box::new(inner)))
        }
        "tuple" | "Tuple" => {
            let parts = tuple_element_exprs(sub.slice.as_ref());
            let resolved: Result<Vec<_>, _> = parts
                .into_iter()
                .map(|elt| resolve_inner(elt, enums, imports, aliases, seen))
                .collect();
            Ok(SupportedType::Tuple(
                resolved.map_err(|e| UnsupportedType::Inner(Box::new(e)))?,
            ))
        }
        "Optional" => {
            let inner = resolve_inner(sub.slice.as_ref(), enums, imports, aliases, seen)
                .map_err(|e| UnsupportedType::Inner(Box::new(e)))?;
            Ok(SupportedType::Optional(Box::new(inner)))
        }
        "Annotated" => {
            // The first element is the underlying type; the rest are
            // metadata we ignore here. Validation of `arg(...)` lives
            // in a separate pass.
            let exprs = tuple_element_exprs(sub.slice.as_ref());
            let first = exprs
                .first()
                .ok_or_else(|| UnsupportedType::UnsupportedShape("Annotated[]".into()))?;
            resolve_inner(first, enums, imports, aliases, seen)
        }
        other => Err(UnsupportedType::UnsupportedShape(other.to_string())),
    }
}

fn resolve_bin_op(
    op: &ruff_python_ast::ExprBinOp,
    enums: &EnumTable,
    imports: &TypeImports,
    aliases: &TypeAliasTable,
    seen: &mut Vec<String>,
) -> Result<SupportedType, UnsupportedType> {
    if !matches!(op.op, ruff_python_ast::Operator::BitOr) {
        return Err(UnsupportedType::UnsupportedShape(format!("{:?}", op.op)));
    }
    // Recognise `T | None` (in either order) as Optional[T]. Anything
    // else is a union we can't handle.
    let (lhs, rhs) = (op.left.as_ref(), op.right.as_ref());
    let none_lhs = matches!(lhs, Expr::NoneLiteral(_));
    let none_rhs = matches!(rhs, Expr::NoneLiteral(_));
    match (none_lhs, none_rhs) {
        (false, true) => {
            let inner = resolve_inner(lhs, enums, imports, aliases, seen)
                .map_err(|e| UnsupportedType::Inner(Box::new(e)))?;
            Ok(SupportedType::Optional(Box::new(inner)))
        }
        (true, false) => {
            let inner = resolve_inner(rhs, enums, imports, aliases, seen)
                .map_err(|e| UnsupportedType::Inner(Box::new(e)))?;
            Ok(SupportedType::Optional(Box::new(inner)))
        }
        _ => Err(UnsupportedType::UnsupportedUnion(annotation_to_label(
            &Expr::BinOp(op.clone()),
        ))),
    }
}

fn literal_string_values(expr: &Expr) -> Vec<String> {
    let elts: Vec<&Expr> = match expr {
        Expr::Tuple(t) => t.elts.iter().collect(),
        single => vec![single],
    };
    elts.into_iter()
        .filter_map(|e| match e {
            Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
            _ => None,
        })
        .collect()
}

fn tuple_element_exprs(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::Tuple(t) => t.elts.iter().collect(),
        single => vec![single],
    }
}

fn render_attribute(expr: &Expr) -> String {
    match expr {
        Expr::Name(n) => n.id.as_str().to_string(),
        Expr::Attribute(a) => format!("{}.{}", render_attribute(a.value.as_ref()), a.attr),
        _ => annotation_to_label(expr),
    }
}

fn annotation_to_label(expr: &Expr) -> String {
    match expr {
        Expr::Name(n) => n.id.as_str().to_string(),
        Expr::Attribute(_) => render_attribute(expr),
        Expr::Subscript(s) => format!(
            "{}[...]",
            match s.value.as_ref() {
                Expr::Name(n) => n.id.as_str(),
                _ => "...",
            }
        ),
        Expr::BinOp(_) => "<union>".to_string(),
        _ => "<expr>".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_python_file;
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
