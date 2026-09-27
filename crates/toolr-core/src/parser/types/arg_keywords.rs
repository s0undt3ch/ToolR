//! Reject `arg(...)` calls the Python runtime would refuse, so a typo fails the
//! manifest build instead of raising `TypeError` the first time the command runs.

use ruff_python_ast::Expr;

use super::supported::UnsupportedType;
use super::toolr_arg_calls;
use crate::parser::build::edit_distance;

/// Every keyword `toolr.arg()` accepts, deprecated ones included.
///
/// Must match the keyword-only parameters of `arg` in
/// `crates/toolr-py/python/toolr/utils/_signature.py`; the
/// `arg_keywords_match_python_signature` test enforces it.
pub const ARG_KEYWORDS: &[&str] = &[
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
    // Deprecated, but still accepted at runtime.
    "required",
    "action",
    "choices",
    "nargs",
    "group",
];

/// One [`UnsupportedType`] per unknown keyword, plus one per `arg()`
/// call that passes positional arguments, across every toolr `arg()`
/// call inside an `Annotated[...]` annotation.
pub(super) fn check_arg_calls(annotation: &Expr) -> Vec<UnsupportedType> {
    let mut problems = Vec::new();
    for call in toolr_arg_calls(annotation) {
        // Splats can't be checked statically; Python validates them at call time.
        if call
            .arguments
            .args
            .iter()
            .any(|a| !matches!(a, Expr::Starred(_)))
        {
            problems.push(UnsupportedType::PositionalArgArgument);
        }
        for kw in &call.arguments.keywords {
            let Some(name) = kw.arg.as_ref().map(|n| n.as_str()) else {
                continue;
            };
            if !ARG_KEYWORDS.contains(&name) {
                problems.push(UnsupportedType::UnknownArgKeyword {
                    keyword: name.to_string(),
                    suggestion: suggest_arg_keyword(name),
                });
            }
        }
    }
    problems
}

/// `path_<keyword>` maps to `<keyword>` (the dropped `path_must_*`
/// spellings); otherwise the nearest keyword within the same
/// `(len / 3).max(2)` edit distance the unknown-group hint uses.
pub(super) fn suggest_arg_keyword(keyword: &str) -> Option<String> {
    if let Some(rest) = keyword.strip_prefix("path_") {
        if ARG_KEYWORDS.contains(&rest) {
            return Some(rest.to_string());
        }
    }
    let max = (keyword.len() / 3).max(2);
    let mut best: Option<(usize, &str)> = None;
    for candidate in ARG_KEYWORDS {
        let d = edit_distance(keyword, candidate);
        if d <= max && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, candidate));
        }
    }
    best.map(|(_, s)| s.to_string())
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
        let rust: BTreeSet<&str> = ARG_KEYWORDS.iter().copied().collect();
        assert_eq!(
            python,
            rust,
            "`arg()` keyword drift: update ARG_KEYWORDS in \
             crates/toolr-core/src/parser/types/arg_keywords.rs and the `arg` signature in \
             {} together",
            path.display()
        );
        assert_eq!(
            ARG_KEYWORDS.len(),
            rust.len(),
            "ARG_KEYWORDS has duplicates"
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
}
