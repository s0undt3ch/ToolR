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
    let shown = path.display();
    let module = parse_python_file(&path).unwrap();
    let func = module
        .body
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::FunctionDef(f) if f.name.as_str() == "arg" => Some(f),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no `def arg` in {shown}"));
    let params = func.parameters.as_ref();
    // The positional-argument and unknown-keyword checks both rely on
    // `arg` being `def arg(*, ...)` with no `**kwargs`.
    assert!(
        params.posonlyargs.is_empty() && params.args.is_empty() && params.vararg.is_none(),
        "`arg()` in {shown} now accepts positional arguments; update \
         `check_arg_calls` in crates/toolr-core/src/parser/types/arg_keywords.rs",
    );
    assert!(
        params.kwarg.is_none(),
        "`arg()` in {shown} now accepts `**kwargs`; the unknown-keyword check in \
         crates/toolr-core/src/parser/types/arg_keywords.rs no longer holds",
    );
    let python: BTreeSet<&str> = params
        .kwonlyargs
        .iter()
        .map(|p| p.parameter.name.as_str())
        .collect();
    let rust_list: Vec<&str> = ACTIVE_ARG_KEYWORDS.to_vec();
    let rust: BTreeSet<&str> = rust_list.iter().copied().collect();
    assert_eq!(
        python, rust,
        "`arg()` keyword drift: update ACTIVE_ARG_KEYWORDS in \
         crates/toolr-core/src/parser/types/arg_keywords.rs and the `arg` signature in \
         {shown} together",
    );
    assert_eq!(
        rust_list.len(),
        rust.len(),
        "the keyword lists overlap or have duplicates"
    );
}

#[test]
fn removed_path_keywords_point_at_the_path_types() {
    for (keyword, hint) in [
        ("must_exist", "use `toolr.types.ResolvedPath` instead"),
        ("path_must_exist", "use `toolr.types.ResolvedPath` instead"),
        ("must_be_file", "use `toolr.types.FilePath` instead"),
        ("path_must_be_file", "use `toolr.types.FilePath` instead"),
        ("must_be_dir", "use `toolr.types.DirectoryPath` instead"),
        (
            "path_must_be_dir",
            "use `toolr.types.DirectoryPath` instead",
        ),
    ] {
        assert_eq!(keyword_hint(keyword), Some(hint), "{keyword}");
        assert_eq!(suggest_arg_keyword(keyword), None, "{keyword}");
    }
}

#[test]
fn suggestion_within_edit_distance_two() {
    assert_eq!(
        suggest_arg_keyword("conflict_with").as_deref(),
        Some("conflicts_with")
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
fn removed_argparse_style_keywords_get_a_hint() {
    for keyword in ["required", "action", "choices", "nargs", "group"] {
        assert!(keyword_hint(keyword).is_some(), "{keyword}");
        assert!(!is_arg_keyword(keyword), "{keyword}");
    }
    assert_eq!(suggest_arg_keyword("choice"), None);
    assert_eq!(suggest_arg_keyword("groups"), None);
    assert_eq!(suggest_arg_keyword("path_required"), None);
    assert_eq!(keyword_hint("path_required"), None);
}

#[test]
fn argparse_keywords_get_a_hint_instead_of_a_suggestion() {
    for keyword in ["help", "type", "default"] {
        assert!(keyword_hint(keyword).is_some(), "{keyword}");
        assert_eq!(suggest_arg_keyword(keyword), None, "{keyword}");
    }
    assert_eq!(keyword_hint("metavar"), None);
}
