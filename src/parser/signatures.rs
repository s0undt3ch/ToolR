//! Extract function arguments from a `def` AST node.

use ruff_python_ast::{Expr, Number, StmtFunctionDef};

use super::symbols::EnumTable;
use crate::manifest::{Argument, ArgumentKind};

/// Build the argument list for a command from its function definition.
/// Skips the first parameter (assumed to be `ctx: Context`).
pub fn extract_arguments(func: &StmtFunctionDef, enums: &EnumTable) -> Vec<Argument> {
    let params = func.parameters.as_ref();
    let mut out = Vec::new();

    // Skip ctx (first positional-or-keyword param).
    for p in params.args.iter().skip(1) {
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
        ));
    }

    // Keyword-only parameters (after the `*` separator). With or without a
    // default, they're always exposed as a keyword on the CLI; the kind
    // depends on the annotation shape (bool / list / other).
    for p in &params.kwonlyargs {
        let annotation = p.parameter.annotation.as_deref();
        let kind = classify_keyword_kind(annotation);
        out.push(build_argument(
            p.parameter.name.to_string(),
            kind,
            annotation,
            p.default.as_deref(),
            enums,
        ));
    }

    out
}

fn build_argument(
    name: String,
    kind: ArgumentKind,
    annotation: Option<&Expr>,
    default: Option<&Expr>,
    enums: &EnumTable,
) -> Argument {
    let allowed_values = annotation
        .map(|a| collect_allowed_values(a, enums))
        .unwrap_or_default();
    Argument {
        name,
        kind,
        help: String::new(),
        default: default.map(|d| literal_default(d, enums)),
        type_annotation: annotation.map(annotation_to_string),
        resolved_type: None,
        path_constraints: None,
        allowed_values,
    }
}

/// Pick the right `ArgumentKind` for a parameter exposed as a CLI keyword
/// (anything not bare-positional). The annotation drives whether we
/// produce a no-value `Flag`, a repeating `Repeated`, or a plain `Optional`.
fn classify_keyword_kind(annotation: Option<&Expr>) -> ArgumentKind {
    let Some(ann) = annotation else {
        return ArgumentKind::Optional;
    };
    if is_bool_annotation(ann) {
        return ArgumentKind::Flag;
    }
    if is_list_like_annotation(ann) {
        return ArgumentKind::Repeated;
    }
    ArgumentKind::Optional
}

fn is_bool_annotation(expr: &Expr) -> bool {
    matches!(expr, Expr::Name(n) if n.id.as_str() == "bool")
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

fn collect_allowed_values(annotation: &Expr, enums: &EnumTable) -> Vec<String> {
    let mut allowed = literal_values(annotation);
    if !allowed.is_empty() {
        return allowed;
    }
    if let Some(name) = referenced_name(annotation) {
        if let Some(vals) = enums.lookup(name) {
            allowed = vals.to_vec();
        }
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
fn literal_default(expr: &Expr, enums: &EnumTable) -> String {
    match expr {
        Expr::StringLiteral(s) => s.value.to_str().to_string(),
        Expr::NumberLiteral(n) => match &n.value {
            Number::Int(i) => i.to_string(),
            Number::Float(f) => f.to_string(),
            Number::Complex { real, imag } => format!("({real}+{imag}j)"),
        },
        Expr::BooleanLiteral(b) => if b.value { "true" } else { "false" }.to_string(),
        Expr::NoneLiteral(_) => "None".to_string(),
        Expr::List(l) if l.elts.is_empty() => String::new(),
        Expr::Attribute(attr) => resolve_enum_attribute_default(attr, enums)
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
) -> Option<String> {
    let class = match attr.value.as_ref() {
        Expr::Name(n) => n.id.as_str(),
        _ => return None,
    };
    enums
        .lookup_member(class, attr.attr.as_str())
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
mod tests {
    use super::*;
    use crate::parser::parse_python_file;
    use ruff_python_ast::Stmt;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn first_func(src: &str) -> StmtFunctionDef {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(src.as_bytes()).unwrap();
        let m = parse_python_file(f.path()).unwrap();
        for stmt in m.body {
            if let Stmt::FunctionDef(f) = stmt {
                return f;
            }
        }
        panic!("no function found");
    }

    #[test]
    fn skips_ctx_first_argument() {
        let func = first_func("def f(ctx, name): pass\n");
        let args = extract_arguments(&func, &EnumTable::default());
        assert_eq!(args.len(), 1);
        assert_eq!(args[0].name, "name");
    }

    #[test]
    fn marks_arguments_with_defaults_as_optional() {
        let func = first_func("def f(ctx, name=\"x\"): pass\n");
        let args = extract_arguments(&func, &EnumTable::default());
        assert_eq!(args[0].kind, ArgumentKind::Optional);
        assert_eq!(args[0].default.as_deref(), Some("x"));
    }

    #[test]
    fn bool_with_false_default_classified_as_flag() {
        let func = first_func("def f(ctx, verbose: bool = False): pass\n");
        let args = extract_arguments(&func, &EnumTable::default());
        assert_eq!(args[0].kind, ArgumentKind::Flag);
        assert_eq!(args[0].default.as_deref(), Some("false"));
    }

    #[test]
    fn list_keyword_classified_as_repeated() {
        let func = first_func("def f(ctx, files: list[str] = []): pass\n");
        let args = extract_arguments(&func, &EnumTable::default());
        assert_eq!(args[0].kind, ArgumentKind::Repeated);
    }

    #[test]
    fn star_args_emits_var_positional() {
        let func = first_func("def f(ctx, *files: str): pass\n");
        let args = extract_arguments(&func, &EnumTable::default());
        assert_eq!(args.len(), 1);
        assert_eq!(args[0].name, "files");
        assert_eq!(args[0].kind, ArgumentKind::VarPositional);
        assert_eq!(args[0].type_annotation.as_deref(), Some("str"));
    }

    #[test]
    fn integer_default_serialized_without_format_noise() {
        let func = first_func("def f(ctx, n: int = 5): pass\n");
        let args = extract_arguments(&func, &EnumTable::default());
        assert_eq!(args[0].default.as_deref(), Some("5"));
    }

    #[test]
    fn string_default_has_no_embedded_quotes() {
        let func = first_func("def f(ctx, name: str = \"world\"): pass\n");
        let args = extract_arguments(&func, &EnumTable::default());
        assert_eq!(args[0].default.as_deref(), Some("world"));
    }

    #[test]
    fn enum_attribute_default_resolves_to_serialised_value() {
        let src = r#"
from enum import StrEnum

class Operation(StrEnum):
    ADD = "add"
    SUBTRACT = "subtract"

def f(ctx, op: Operation = Operation.ADD): pass
"#;
        let m = module(src);
        let mut enums = EnumTable::default();
        enums.merge(EnumTable::from_module(&m));
        let func = m
            .body
            .iter()
            .find_map(|s| match s {
                ruff_python_ast::Stmt::FunctionDef(f) => Some(f.clone()),
                _ => None,
            })
            .unwrap();
        let args = extract_arguments(&func, &enums);
        assert_eq!(args[0].default.as_deref(), Some("add"));
    }

    #[test]
    fn unknown_enum_attribute_falls_back_to_expr_placeholder() {
        let func = first_func("def f(ctx, x = Unknown.MEMBER): pass\n");
        let args = extract_arguments(&func, &EnumTable::default());
        assert_eq!(args[0].default.as_deref(), Some("<expr>"));
    }

    fn module(src: &str) -> ruff_python_ast::ModModule {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(src.as_bytes()).unwrap();
        crate::parser::parse_python_file(f.path()).unwrap()
    }

    #[test]
    fn captures_type_annotations_as_strings() {
        let func = first_func("def f(ctx, name: str = \"x\"): pass\n");
        let args = extract_arguments(&func, &EnumTable::default());
        assert_eq!(args[0].type_annotation.as_deref(), Some("str"));
    }

    #[test]
    fn extracts_literal_values() {
        let func = first_func(
            r#"
from typing import Literal
def f(ctx, mode: Literal["a", "b"]): pass
"#,
        );
        let args = extract_arguments(&func, &EnumTable::default());
        assert_eq!(
            args[0].allowed_values,
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn leaves_allowed_values_empty_for_non_literal_types() {
        let func = first_func("def f(ctx, name: str): pass\n");
        let args = extract_arguments(&func, &EnumTable::default());
        assert!(args[0].allowed_values.is_empty());
    }

    #[test]
    fn resolves_local_enum_for_allowed_values() {
        use super::super::symbols::EnumTable;

        let src = r#"
from enum import StrEnum

class Mode(StrEnum):
    FAST = "fast"
    SLOW = "slow"

def f(ctx, mode: Mode): pass
"#;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        use std::io::Write;
        tmp.write_all(src.as_bytes()).unwrap();
        let m = crate::parser::parse_python_file(tmp.path()).unwrap();
        let mut enums = EnumTable::default();
        enums.merge(EnumTable::from_module(&m));
        // Pull out the function manually (skip the enum class above).
        let func = m
            .body
            .iter()
            .find_map(|s| match s {
                ruff_python_ast::Stmt::FunctionDef(f) => Some(f.clone()),
                _ => None,
            })
            .unwrap();
        let args = extract_arguments(&func, &enums);
        assert_eq!(
            args[0].allowed_values,
            vec!["fast".to_string(), "slow".to_string()]
        );
    }
}
