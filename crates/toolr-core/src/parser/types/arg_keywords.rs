//! Reject `arg(...)` calls the Python runtime would refuse, so a typo fails the
//! manifest build instead of raising `TypeError` the first time the command runs.

use std::collections::HashSet;

use ruff_python_ast::{Expr, ExprCall, Operator};

use super::supported::UnsupportedType;
use crate::parser::build::edit_distance;
use crate::parser::symbols::{ImportTable, TypeAliasTable};

/// `toolr.arg()`'s current keywords; the only ones typo hints suggest.
///
/// Together with [`DEPRECATED_ARG_KEYWORDS`] this must match the keyword-only
/// parameters of `arg` in `crates/toolr-py/python/toolr/utils/_signature.py`;
/// the `arg_keywords_match_python_signature` test enforces it.
pub const ACTIVE_ARG_KEYWORDS: &[&str] = &[
    "aliases",
    "metavar",
    "env",
    "hide",
    "help_section",
    "display_order",
    "conflicts_with",
    "requires",
    "must_exist",
    "must_be_file",
    "must_be_dir",
];

/// Deprecated `toolr.arg()` keywords: still accepted, never suggested.
pub const DEPRECATED_ARG_KEYWORDS: &[&str] = &["required", "action", "choices", "nargs", "group"];

fn is_arg_keyword(name: &str) -> bool {
    ACTIVE_ARG_KEYWORDS.contains(&name) || DEPRECATED_ARG_KEYWORDS.contains(&name)
}

/// One [`UnsupportedType`] per unknown keyword, plus one per `arg()` call
/// that passes positional arguments, for every toolr `arg()` call anywhere
/// in the annotation, including through module-level aliases.
pub(super) fn check_arg_calls(
    annotation: &Expr,
    aliases: &TypeAliasTable,
    imports: Option<&ImportTable>,
) -> Vec<UnsupportedType> {
    let mut problems = Vec::new();
    walk(annotation, aliases, imports, &mut HashSet::new(), &mut problems);
    problems
}

fn walk(
    expr: &Expr,
    aliases: &TypeAliasTable,
    imports: Option<&ImportTable>,
    expanded: &mut HashSet<String>,
    problems: &mut Vec<UnsupportedType>,
) {
    match expr {
        Expr::Call(call) if calls_toolr_arg(call, imports) => check_call(call, problems),
        Expr::Subscript(sub) => {
            walk(&sub.value, aliases, imports, expanded, problems);
            walk(&sub.slice, aliases, imports, expanded, problems);
        }
        Expr::Tuple(t) => {
            for elt in &t.elts {
                walk(elt, aliases, imports, expanded, problems);
            }
        }
        Expr::BinOp(b) if b.op == Operator::BitOr => {
            walk(&b.left, aliases, imports, expanded, problems);
            walk(&b.right, aliases, imports, expanded, problems);
        }
        // Each alias is expanded once, which also breaks `A = B; B = A` cycles.
        Expr::Name(n) if expanded.insert(n.id.to_string()) => {
            if let Some(target) = aliases.lookup(n.id.as_str()) {
                walk(target, aliases, imports, expanded, problems);
            }
        }
        _ => {}
    }
}

fn check_call(call: &ExprCall, problems: &mut Vec<UnsupportedType>) {
    // Splats can't be checked statically; Python validates them at call time.
    if call.arguments.args.iter().any(|a| !matches!(a, Expr::Starred(_))) {
        problems.push(UnsupportedType::PositionalArgArgument);
    }
    for kw in &call.arguments.keywords {
        let Some(name) = kw.arg.as_ref().map(|n| n.as_str()) else {
            continue;
        };
        if !is_arg_keyword(name) {
            let suggestion = match argparse_hint(name) {
                Some(_) => None,
                None => suggest_arg_keyword(name),
            };
            problems.push(UnsupportedType::UnknownArgKeyword {
                keyword: name.to_string(),
                suggestion,
            });
        }
    }
}

fn is_toolr_module(module: &str) -> bool {
    module == "toolr" || module.starts_with("toolr.")
}

/// Whether `call` resolves to `toolr.arg`, judged from this module's imports.
fn calls_toolr_arg(call: &ExprCall, imports: Option<&ImportTable>) -> bool {
    match call.func.as_ref() {
        Expr::Name(n) => {
            let candidates = imports.map_or(&[][..], |t| t.candidates(n.id.as_str()));
            if candidates.is_empty() {
                // Unresolvable binding (star import, alias from another module):
                // keep treating the literal name `arg` as toolr's.
                return n.id.as_str() == "arg";
            }
            candidates
                .iter()
                .any(|c| is_toolr_module(&c.module) && c.original_name == "arg")
        }
        Expr::Attribute(a) if a.attr.as_str() == "arg" => {
            let mut root = a.value.as_ref();
            while let Expr::Attribute(inner) = root {
                root = inner.value.as_ref();
            }
            let (Expr::Name(r), Some(table)) = (root, imports) else {
                return false;
            };
            table.resolve_module_binding(r.id.as_str()).is_some_and(is_toolr_module)
                || table
                    .candidates(r.id.as_str())
                    .iter()
                    .any(|c| is_toolr_module(&c.module))
        }
        _ => false,
    }
}

/// `path_<keyword>` maps to `<keyword>` (the dropped `path_must_*`
/// spellings); otherwise the nearest active keyword within the same
/// `(len / 3).max(2)` edit distance the unknown-group hint uses.
pub(super) fn suggest_arg_keyword(keyword: &str) -> Option<String> {
    if let Some(rest) = keyword.strip_prefix("path_") {
        if ACTIVE_ARG_KEYWORDS.contains(&rest) {
            return Some(rest.to_string());
        }
    }
    let max = (keyword.len() / 3).max(2);
    let mut best: Option<(usize, &str)> = None;
    for candidate in ACTIVE_ARG_KEYWORDS {
        let d = edit_distance(keyword, candidate);
        if d <= max && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, candidate));
        }
    }
    best.map(|(_, s)| s.to_string())
}

/// Where toolr takes what an argparse-style `arg()` keyword would have set.
pub(super) fn argparse_hint(keyword: &str) -> Option<&'static str> {
    match keyword {
        "help" => Some("help text comes from the docstring's `Args:` section"),
        "type" => Some("the type comes from the annotation"),
        "default" => Some("the default comes from the parameter's default value"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_python_file;
    use ruff_python_ast::Stmt;
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    fn signature_py() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("toolr-core lives under crates/")
            .join("toolr-py/python/toolr/utils/_signature.py")
    }

    #[test]
    fn arg_keywords_match_python_signature() {
        let path = signature_py();
        let module = parse_python_file(&path).unwrap();
        let func = module
            .body
            .iter()
            .find_map(|stmt| match stmt {
                Stmt::FunctionDef(f) if f.name.as_str() == "arg" => Some(f),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no `def arg` in {}", path.display()));
        let params = func.parameters.as_ref();
        // The positional-argument and unknown-keyword checks both rely on
        // `arg` being `def arg(*, ...)` with no `**kwargs`.
        assert!(
            params.posonlyargs.is_empty() && params.args.is_empty() && params.vararg.is_none(),
            "`arg()` in {} now accepts positional arguments; update \
             `check_arg_calls` in crates/toolr-core/src/parser/types/arg_keywords.rs",
            path.display()
        );
        assert!(
            params.kwarg.is_none(),
            "`arg()` in {} now accepts `**kwargs`; the unknown-keyword check in \
             crates/toolr-core/src/parser/types/arg_keywords.rs no longer holds",
            path.display()
        );
        let python: BTreeSet<&str> = params
            .kwonlyargs
            .iter()
            .map(|p| p.parameter.name.as_str())
            .collect();
        let rust_list: Vec<&str> = ACTIVE_ARG_KEYWORDS
            .iter()
            .chain(DEPRECATED_ARG_KEYWORDS)
            .copied()
            .collect();
        let rust: BTreeSet<&str> = rust_list.iter().copied().collect();
        assert_eq!(
            python,
            rust,
            "`arg()` keyword drift: update ACTIVE_ARG_KEYWORDS / DEPRECATED_ARG_KEYWORDS in \
             crates/toolr-core/src/parser/types/arg_keywords.rs and the `arg` signature in \
             {} together",
            path.display()
        );
        assert_eq!(
            rust_list.len(),
            rust.len(),
            "the keyword lists overlap or have duplicates"
        );
    }

    #[test]
    fn suggestion_strips_path_prefix() {
        assert_eq!(
            suggest_arg_keyword("path_must_exist").as_deref(),
            Some("must_exist")
        );
        assert_eq!(
            suggest_arg_keyword("path_must_be_dir").as_deref(),
            Some("must_be_dir")
        );
    }

    #[test]
    fn suggestion_within_edit_distance_two() {
        assert_eq!(
            suggest_arg_keyword("must_bee_file").as_deref(),
            Some("must_be_file")
        );
        assert_eq!(suggest_arg_keyword("metvar").as_deref(), Some("metavar"));
        assert_eq!(suggest_arg_keyword("alias").as_deref(), Some("aliases"));
    }

    #[test]
    fn suggestion_within_a_third_of_the_length() {
        // Distance 3 from `conflicts_with`, allowed because 14 / 3 = 4.
        assert_eq!(
            suggest_arg_keyword("conflicts_wxyz").as_deref(),
            Some("conflicts_with")
        );
    }

    #[test]
    fn no_suggestion_when_nothing_is_close() {
        assert_eq!(suggest_arg_keyword("foo"), None);
        assert_eq!(suggest_arg_keyword("path_foo"), None);
        assert_eq!(suggest_arg_keyword("completely_unrelated"), None);
    }

    #[test]
    fn suggestion_never_points_at_a_deprecated_keyword() {
        assert_eq!(suggest_arg_keyword("choice"), None);
        assert_eq!(suggest_arg_keyword("groups"), None);
        assert_eq!(suggest_arg_keyword("path_required"), None);
    }

    #[test]
    fn argparse_keywords_get_a_hint_instead_of_a_suggestion() {
        for keyword in ["help", "type", "default"] {
            assert!(argparse_hint(keyword).is_some(), "{keyword}");
            assert_eq!(suggest_arg_keyword(keyword), None, "{keyword}");
        }
        assert_eq!(argparse_hint("metavar"), None);
    }
}
