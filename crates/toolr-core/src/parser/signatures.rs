//! Extract function arguments from a `def` AST node.

use std::collections::HashMap;

use ruff_python_ast::{Expr, Number, StmtFunctionDef};

use super::symbols::{ConstTable, EnumTable, ImportTable};
use super::types::SourcesImports;
use crate::manifest::{Argument, ArgumentKind};

/// Build the argument list for a command from its function definition.
/// Skips the first parameter (assumed to be `ctx: Context`).
///
/// Keyword-only parameters annotated with `toolr.sources.DispatchCommand`
/// are dropped — they're a runtime-injection slot, not a CLI argument.
/// `sources` carries the import table needed to resolve the annotation
/// to that type (handles `from toolr.sources import DispatchCommand`
/// with or without an alias, plus `import toolr.sources` qualified use).
pub fn extract_arguments(
    func: &StmtFunctionDef,
    enums: &EnumTable,
    all_imports: &HashMap<String, ImportTable>,
    consts: &ConstTable,
    sources: &SourcesImports,
    module: &str,
) -> Vec<Argument> {
    let params = func.parameters.as_ref();
    let mut out = Vec::new();

    // `ctx` is the first positional, which is positional-only when the
    // signature has a `/`.
    for p in params.posonlyargs.iter().chain(&params.args).skip(1) {
        let annotation = p.parameter.annotation.as_deref();
        let has_default = p.default.is_some();
        let kind = if has_default {
            classify_keyword_kind(annotation)
        } else {
            ArgumentKind::Positional
        };
        out.push(build_argument(
            p.parameter.name.to_string(),
            kind,
            annotation,
            p.default.as_deref(),
            enums,
            all_imports,
            consts,
            module,
        ));
    }

    // Variadic positional (`*args: T`). ruff exposes this as `params.vararg`.
    if let Some(vararg) = params.vararg.as_deref() {
        let annotation = vararg.annotation.as_deref();
        out.push(build_argument(
            vararg.name.to_string(),
            ArgumentKind::VarPositional,
            annotation,
            None,
            enums,
            all_imports,
            consts,
            module,
        ));
    }

    // Keyword-only parameters (after the `*` separator). With or without a
    // default, they're always exposed as a keyword on the CLI; the kind
    // depends on the annotation shape (bool / list / other).
    //
    // Exception: a kwarg annotated with `toolr.sources.DispatchCommand`
    // is a runtime injection slot used by the dispatcher implementation
    // and never lands on the CLI — skip it entirely so the type
    // resolver doesn't see it as an unsupported name.
    for p in &params.kwonlyargs {
        let annotation = p.parameter.annotation.as_deref();
        if let Some(ann) = annotation
            && sources.is_dispatch_command(ann)
        {
            continue;
        }
        let kind = classify_keyword_kind(annotation);
        out.push(build_argument(
            p.parameter.name.to_string(),
            kind,
            annotation,
            p.default.as_deref(),
            enums,
            all_imports,
            consts,
            module,
        ));
    }

    out
}

#[allow(clippy::too_many_arguments)]
fn build_argument(
    name: String,
    kind: ArgumentKind,
    annotation: Option<&Expr>,
    default: Option<&Expr>,
    enums: &EnumTable,
    all_imports: &HashMap<String, ImportTable>,
    consts: &ConstTable,
    module: &str,
) -> Argument {
    let allowed_values = annotation
        .map(|a| collect_allowed_values(a, enums, all_imports, module))
        .unwrap_or_default();
    Argument {
        name,
        kind,
        help: String::new(),
        default: default.map(|d| literal_default(d, enums, all_imports, consts, module)),
        type_annotation: annotation.map(annotation_to_string),
        resolved_type: None,
        allowed_values,
        metadata: crate::manifest::ArgMetadata::default(),
        // Native toolr commands built from Python function signatures
        // don't have a source-literal flag spelling — the param name
        // *is* the canonical form; the CLI hyphenates for display and
        // there is no upstream argparse to dispatch to.
        long_flag: None,
    }
}

/// Pick the right `ArgumentKind` for a parameter exposed as a CLI keyword
/// (anything not bare-positional). The annotation drives whether we
/// produce a no-value `Flag`, a repeating `Repeated`, or a plain `Optional`.
fn classify_keyword_kind(annotation: Option<&Expr>) -> ArgumentKind {
    let Some(ann) = annotation else {
        return ArgumentKind::Optional;
    };
    // `Annotated[T, arg(...)]` wraps the real type — peel it off
    // before pattern-matching, otherwise `Annotated[bool, arg(env=...)]`
    // falls through to `Optional` and renders as a value-taking flag
    // instead of a no-value one.
    let inner = peel_annotated(ann).unwrap_or(ann);
    if is_bool_annotation(inner) {
        return ArgumentKind::Flag;
    }
    if is_list_like_annotation(inner) {
        return ArgumentKind::Repeated;
    }
    ArgumentKind::Optional
}

fn is_bool_annotation(expr: &Expr) -> bool {
    matches!(expr, Expr::Name(n) if n.id.as_str() == "bool")
}

/// If `expr` is `Annotated[T, ...]`, return `T`; otherwise `None`.
/// Handles both `typing.Annotated[...]` and bare `Annotated[...]` (the
/// caller doesn't have to know which import shape is in use).
fn peel_annotated(expr: &Expr) -> Option<&Expr> {
    let Expr::Subscript(sub) = expr else {
        return None;
    };
    let head = match sub.value.as_ref() {
        Expr::Name(n) => n.id.as_str(),
        Expr::Attribute(a) => a.attr.as_str(),
        _ => return None,
    };
    if head != "Annotated" {
        return None;
    }
    match sub.slice.as_ref() {
        Expr::Tuple(t) => t.elts.first(),
        single => Some(single),
    }
}

/// Detects `list[...]`, `List[...]`, `tuple[..., ...]`, `Tuple[..., ...]`.
fn is_list_like_annotation(expr: &Expr) -> bool {
    let Expr::Subscript(sub) = expr else {
        return false;
    };
    let head = match sub.value.as_ref() {
        Expr::Name(n) => n.id.as_str(),
        Expr::Attribute(a) => a.attr.as_str(),
        _ => return false,
    };
    matches!(head, "list" | "List" | "tuple" | "Tuple")
}

fn collect_allowed_values(
    annotation: &Expr,
    enums: &EnumTable,
    all_imports: &HashMap<String, ImportTable>,
    module: &str,
) -> Vec<String> {
    let mut allowed = literal_values(annotation);
    if !allowed.is_empty() {
        return allowed;
    }
    if let Some(name) = referenced_name(annotation)
        && let Some((_module, vals)) = enums.lookup(name, module, all_imports)
    {
        allowed = vals;
    }
    allowed
}

fn referenced_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name(n) => Some(n.id.as_str()),
        _ => None,
    }
}

/// Render a default-value expression as a wire-format string.
///
/// The contract is that the value is what `--help` prints **and** what
/// the toolr binary feeds back through msgspec when the user omits the
/// flag. So strings are rendered unquoted (msgspec.convert "world" →
/// `"world"`), numbers as their literal form, bools lowercased, and
/// `Class.MEMBER` attribute defaults are resolved via `enums` to their
/// serialised value (so `Operation.ADD` becomes `"add"` for a
/// `StrEnum`).
fn literal_default(
    expr: &Expr,
    enums: &EnumTable,
    all_imports: &HashMap<String, ImportTable>,
    consts: &ConstTable,
    module: &str,
) -> String {
    match expr {
        Expr::StringLiteral(s) => s.value.to_str().to_string(),
        Expr::NumberLiteral(n) => match &n.value {
            Number::Int(i) => i.to_string(),
            Number::Float(f) => f.to_string(),
            Number::Complex { real, imag } => format!("({real}+{imag}j)"),
        },
        Expr::BooleanLiteral(b) => if b.value { "true" } else { "false" }.to_string(),
        // `None` default means "absent on the wire" — clap doesn't set
        // a default_value, `extract_value` doesn't emit anything, and
        // the python function's own `= None` default is what runs.
        // Render as empty so the manifest's `default` reads as "no
        // observable default" downstream (matched by `Option<String>::is_none()`
        // once we ship the schema flip; today the empty-string check
        // in cli.rs:build_user_command skips applying it).
        Expr::NoneLiteral(_) => String::new(),
        Expr::List(l) if l.elts.is_empty() => String::new(),
        Expr::Attribute(attr) => resolve_enum_attribute_default(attr, enums, all_imports, module)
            .unwrap_or_else(|| "<expr>".to_string()),
        // Bare name reference (e.g. `mode: str = DEFAULT_MODE`) — look up
        // the resolved literal in the same-module constant table. Falls
        // through to `<expr>` when the name isn't a tracked literal
        // constant (cross-module imports, function calls, expressions).
        Expr::Name(n) => consts
            .lookup(n.id.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| "<expr>".to_string()),
        _ => "<expr>".to_string(),
    }
}

/// Resolve `Class.MEMBER` attribute expressions against the enum
/// table. Returns the serialised value (e.g. `"add"`) when both
/// `Class` and `MEMBER` are known; `None` otherwise.
fn resolve_enum_attribute_default(
    attr: &ruff_python_ast::ExprAttribute,
    enums: &EnumTable,
    all_imports: &HashMap<String, ImportTable>,
    module: &str,
) -> Option<String> {
    let class = match attr.value.as_ref() {
        Expr::Name(n) => n.id.as_str(),
        _ => return None,
    };
    enums
        .lookup_member(class, attr.attr.as_str(), module, all_imports)
        .map(str::to_string)
}

fn annotation_to_string(expr: &Expr) -> String {
    // Best-effort textual rendering. Detailed resolution lands in
    // Tasks 12-14.
    match expr {
        Expr::Name(n) => n.id.as_str().to_string(),
        Expr::Attribute(a) => format!("{}.{}", annotation_to_string(&a.value), a.attr),
        Expr::Subscript(s) => format!(
            "{}[{}]",
            annotation_to_string(&s.value),
            annotation_to_string(&s.slice)
        ),
        _ => "<expr>".to_string(),
    }
}

fn literal_values(annotation: &Expr) -> Vec<String> {
    let Expr::Subscript(sub) = annotation else {
        return Vec::new();
    };
    // The subscripted expression must be named "Literal".
    let is_literal = match sub.value.as_ref() {
        Expr::Name(n) => n.id.as_str() == "Literal",
        Expr::Attribute(a) => a.attr.as_str() == "Literal",
        _ => false,
    };
    if !is_literal {
        return Vec::new();
    }
    match sub.slice.as_ref() {
        Expr::Tuple(t) => t.elts.iter().filter_map(literal_str_value).collect(),
        other => literal_str_value(other).into_iter().collect(),
    }
}

fn literal_str_value(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
