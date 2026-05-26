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

/// Extract a string sequence from a list, tuple, or set literal of
/// string literals. Mirrors the Python `arg()` helper, which accepts
/// any non-string `Iterable[str]` — these three are the
/// statically-recognisable literal forms.
///
/// Returns `None` for any other expression shape (name references,
/// function calls, mixed-type literals). Callers should surface that
/// as a manifest-build warning rather than silently dropping the
/// kwarg.
pub(super) fn literal_str_list(expr: &Expr) -> Option<Vec<String>> {
    let elts: &[Expr] = match expr {
        Expr::List(list) => &list.elts,
        Expr::Tuple(tup) => &tup.elts,
        Expr::Set(set) => &set.elts,
        _ => return None,
    };
    let out: Vec<String> = elts.iter().filter_map(literal_str).collect();
    if out.len() == elts.len() { Some(out) } else { None }
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
