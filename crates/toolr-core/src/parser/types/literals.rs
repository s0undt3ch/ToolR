//! Small AST-literal extractors used across the type-resolution
//! pipeline. Each function returns `None` when the expression isn't
//! the expected literal shape.

use ruff_python_ast::Expr;

pub(super) fn literal_str(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
        _ => None,
    }
}

pub(super) fn literal_str_list(expr: &Expr) -> Option<Vec<String>> {
    let Expr::List(list) = expr else { return None };
    let out: Vec<String> = list.elts.iter().filter_map(literal_str).collect();
    if out.len() == list.elts.len() {
        Some(out)
    } else {
        None
    }
}

pub(super) fn literal_u32(expr: &Expr) -> Option<u32> {
    match expr {
        Expr::NumberLiteral(n) => match &n.value {
            ruff_python_ast::Number::Int(i) => i.as_u64().and_then(|v| u32::try_from(v).ok()),
            _ => None,
        },
        _ => None,
    }
}
