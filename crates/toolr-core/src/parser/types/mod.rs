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

mod arg_keywords;
mod arg_metadata;
mod bool_flag;
mod imports;
mod literals;
mod resolve;
mod supported;

pub use arg_keywords::ACTIVE_ARG_KEYWORDS;
pub use arg_metadata::extract_arg_metadata;
pub use bool_flag::BoolFlagError;
pub use imports::{SourcesImports, TypeImports};
pub use resolve::{resolve, resolve_arguments};
pub use supported::{
    PathCompletion, SupportedType, SupportedTypeKind, TypeDoc, TypeResolutionError, UnsupportedType,
};

use ruff_python_ast::{Expr, ExprCall};

/// Predicate shared between the arg-metadata extractor and the `arg()`
/// keyword checks: is this call expression a `toolr.arg(...)` call (or
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

/// The toolr `arg(...)` calls among an `Annotated[T, ...]` annotation's
/// metadata elements; empty for any other annotation shape.
pub(super) fn toolr_arg_calls(annotation: &Expr) -> Vec<&ExprCall> {
    let Expr::Subscript(sub) = annotation else {
        return Vec::new();
    };
    let head = match sub.value.as_ref() {
        Expr::Name(n) => n.id.as_str(),
        Expr::Attribute(a) => a.attr.as_str(),
        _ => return Vec::new(),
    };
    if head != "Annotated" {
        return Vec::new();
    }
    let elts: Vec<&Expr> = match sub.slice.as_ref() {
        Expr::Tuple(t) => t.elts.iter().collect(),
        single => vec![single],
    };
    elts.into_iter()
        .skip(1)
        .filter_map(|elt| match elt {
            Expr::Call(call) if is_toolr_arg_call(call) => Some(call),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests;
