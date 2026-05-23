//! The annotation-to-`SupportedType` resolver.
//!
//! The public entry points are `resolve_arguments` (walks every
//! parameter on a `StmtFunctionDef`, fills in `Argument.resolved_type`
//! and `Argument.metadata`, appends `TypeResolutionError`s for
//! anything unsupported) and `resolve` (just the type-resolution
//! lookup for a single annotation expression).
//!
//! All annotation shapes toolr recognises live in
//! [`crate::parser::types::SupportedType`]. The resolver is name-based:
//! it inspects the textual annotation as parsed by `ruff_python_parser`
//! plus the [`TypeImports`] / [`SourcesImports`] / [`TypeAliasTable`]
//! / [`EnumTable`] / [`ArgSectionTable`] symbol tables this module
//! receives as parameters.

use ruff_python_ast::{Expr, StmtFunctionDef};

use super::arg_metadata::extract_arg_metadata;
use super::path_constraints::extract_path_constraints;
use super::supported::{SupportedType, TypeResolutionError, UnsupportedType};
use super::{PathConstraints, SourcesImports, TypeImports};
use crate::manifest::{ArgMetadata, Argument, ArgumentKind};
use crate::parser::symbols::{ArgSectionTable, EnumTable, TypeAliasTable};

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

pub(super) fn resolve_toolr_types_name(name: &str) -> Result<SupportedType, UnsupportedType> {
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
