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
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &SourcesImports::default(),
        "tools.test",
    );
    assert_eq!(args.len(), 1);
    assert_eq!(args[0].name, "name");
}

#[test]
fn includes_positional_only_params_and_skips_only_ctx() {
    for src in [
        "def f(ctx, name, /, count): pass\n",
        "def f(ctx, /, name, count): pass\n",
    ] {
        let func = first_func(src);
        let args = extract_arguments(
            &func,
            &EnumTable::default(),
            &std::collections::HashMap::new(),
            &ConstTable::default(),
            &SourcesImports::default(),
            "tools.test",
        );
        let names: Vec<_> = args.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["name", "count"], "{src}");
    }
}

#[test]
fn marks_arguments_with_defaults_as_optional() {
    let func = first_func("def f(ctx, name=\"x\"): pass\n");
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &SourcesImports::default(),
        "tools.test",
    );
    assert_eq!(args[0].kind, ArgumentKind::Optional);
    assert_eq!(args[0].default.as_deref(), Some("x"));
}

#[test]
fn bool_with_false_default_classified_as_flag() {
    let func = first_func("def f(ctx, verbose: bool = False): pass\n");
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &SourcesImports::default(),
        "tools.test",
    );
    assert_eq!(args[0].kind, ArgumentKind::Flag);
    assert_eq!(args[0].default.as_deref(), Some("false"));
}

/// `Annotated[bool, arg(...)]` should classify as `Flag` too.
/// Regression: the classifier used to match `Expr::Name == "bool"`
/// only, so wrapping a `bool` in `Annotated[...]` (e.g. to set
/// `env="…"` on the flag) silently downgraded to `Optional`, which
/// rendered as a value-taking `--verbose VALUE` flag at the CLI.
#[test]
fn annotated_bool_classified_as_flag() {
    let func = first_func(
        "from typing import Annotated\n\
         from toolr import arg\n\
         def f(ctx, verbose: Annotated[bool, arg(env=\"VERBOSE\")] = False): pass\n",
    );
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &SourcesImports::default(),
        "tools.test",
    );
    assert_eq!(args[0].kind, ArgumentKind::Flag);
}

#[test]
fn annotated_list_classified_as_repeated() {
    let func = first_func(
        "from typing import Annotated\n\
         from toolr import arg\n\
         def f(ctx, items: Annotated[list[str], arg(aliases=[\"-i\"])] = []): pass\n",
    );
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &SourcesImports::default(),
        "tools.test",
    );
    assert_eq!(args[0].kind, ArgumentKind::Repeated);
}

#[test]
fn list_keyword_classified_as_repeated() {
    let func = first_func("def f(ctx, files: list[str] = []): pass\n");
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &SourcesImports::default(),
        "tools.test",
    );
    assert_eq!(args[0].kind, ArgumentKind::Repeated);
}

#[test]
fn star_args_emits_var_positional() {
    let func = first_func("def f(ctx, *files: str): pass\n");
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &SourcesImports::default(),
        "tools.test",
    );
    assert_eq!(args.len(), 1);
    assert_eq!(args[0].name, "files");
    assert_eq!(args[0].kind, ArgumentKind::VarPositional);
    assert_eq!(args[0].type_annotation.as_deref(), Some("str"));
}

#[test]
fn integer_default_serialized_without_format_noise() {
    let func = first_func("def f(ctx, n: int = 5): pass\n");
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &SourcesImports::default(),
        "tools.test",
    );
    assert_eq!(args[0].default.as_deref(), Some("5"));
}

#[test]
fn string_default_has_no_embedded_quotes() {
    let func = first_func("def f(ctx, name: str = \"world\"): pass\n");
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &SourcesImports::default(),
        "tools.test",
    );
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
    enums.merge(EnumTable::from_module(&m, "tools.test"));
    let func = m
        .body
        .iter()
        .find_map(|s| match s {
            ruff_python_ast::Stmt::FunctionDef(f) => Some(f.clone()),
            _ => None,
        })
        .unwrap();
    let args = extract_arguments(
        &func,
        &enums,
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &SourcesImports::default(),
        "tools.test",
    );
    assert_eq!(args[0].default.as_deref(), Some("add"));
}

#[test]
fn unknown_enum_attribute_falls_back_to_expr_placeholder() {
    let func = first_func("def f(ctx, x = Unknown.MEMBER): pass\n");
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &SourcesImports::default(),
        "tools.test",
    );
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
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &SourcesImports::default(),
        "tools.test",
    );
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
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &SourcesImports::default(),
        "tools.test",
    );
    assert_eq!(
        args[0].allowed_values,
        vec!["a".to_string(), "b".to_string()]
    );
}

#[test]
fn leaves_allowed_values_empty_for_non_literal_types() {
    let func = first_func("def f(ctx, name: str): pass\n");
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &SourcesImports::default(),
        "tools.test",
    );
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
    enums.merge(EnumTable::from_module(&m, "tools.test"));
    // Pull out the function manually (skip the enum class above).
    let func = m
        .body
        .iter()
        .find_map(|s| match s {
            ruff_python_ast::Stmt::FunctionDef(f) => Some(f.clone()),
            _ => None,
        })
        .unwrap();
    let args = extract_arguments(
        &func,
        &enums,
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &SourcesImports::default(),
        "tools.test",
    );
    assert_eq!(
        args[0].allowed_values,
        vec!["fast".to_string(), "slow".to_string()]
    );
}

/// A dispatcher command's `DispatchCommand`-annotated kwarg is a
/// runtime injection slot, not a CLI argument. It must be filtered
/// out before the type resolver sees it — otherwise
/// `build_static_manifest` rejects the whole module with
/// "type `DispatchCommand` is not supported".
#[test]
fn dispatch_command_kwarg_is_filtered_out() {
    let src = r#"
from toolr.sources import DispatchCommand

def f(ctx, *, cpu: str = "1", dispatched: DispatchCommand): pass
"#;
    let m = module(src);
    let sources = SourcesImports::from_module(&m);
    let func = m
        .body
        .iter()
        .find_map(|s| match s {
            ruff_python_ast::Stmt::FunctionDef(f) => Some(f.clone()),
            _ => None,
        })
        .unwrap();
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &sources,
        "tools.test",
    );
    assert_eq!(
        args.len(),
        1,
        "expected `dispatched` to be filtered out, got {args:?}"
    );
    assert_eq!(args[0].name, "cpu");
}

/// `from toolr.sources import DispatchCommand as DC` — aliased
/// import should still resolve to the runtime-injection type and
/// drop the kwarg from the CLI surface.
#[test]
fn aliased_dispatch_command_kwarg_is_filtered_out() {
    let src = r#"
from toolr.sources import DispatchCommand as DC

def f(ctx, *, name: str = "x", dispatched: DC): pass
"#;
    let m = module(src);
    let sources = SourcesImports::from_module(&m);
    let func = m
        .body
        .iter()
        .find_map(|s| match s {
            ruff_python_ast::Stmt::FunctionDef(f) => Some(f.clone()),
            _ => None,
        })
        .unwrap();
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &sources,
        "tools.test",
    );
    assert_eq!(args.len(), 1);
    assert_eq!(args[0].name, "name");
}

/// A normal (non-dispatcher) command, with the same import in
/// scope, must be unaffected — only kwargs whose annotation actually
/// resolves to `DispatchCommand` are skipped.
#[test]
fn regular_kwarg_unaffected_by_sources_import() {
    let src = r#"
from toolr.sources import DispatchCommand

def f(ctx, *, name: str = "x"): pass
"#;
    let m = module(src);
    let sources = SourcesImports::from_module(&m);
    let func = m
        .body
        .iter()
        .find_map(|s| match s {
            ruff_python_ast::Stmt::FunctionDef(f) => Some(f.clone()),
            _ => None,
        })
        .unwrap();
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &sources,
        "tools.test",
    );
    assert_eq!(args.len(), 1);
    assert_eq!(args[0].name, "name");
    // The annotation rendering hasn't lost the type tag.
    assert_eq!(args[0].type_annotation.as_deref(), Some("str"));
}

/// Qualified `toolr.sources.DispatchCommand` reference via
/// `import toolr.sources` (no alias) also filters out.
#[test]
fn qualified_dispatch_command_kwarg_is_filtered_out() {
    let src = r#"
import toolr.sources

def f(ctx, *, name: str = "x", dispatched: toolr.sources.DispatchCommand): pass
"#;
    let m = module(src);
    let sources = SourcesImports::from_module(&m);
    let func = m
        .body
        .iter()
        .find_map(|s| match s {
            ruff_python_ast::Stmt::FunctionDef(f) => Some(f.clone()),
            _ => None,
        })
        .unwrap();
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &sources,
        "tools.test",
    );
    assert_eq!(args.len(), 1);
    assert_eq!(args[0].name, "name");
}

/// `Annotated[DispatchCommand, arg(...)]` should be skipped just
/// like a bare `DispatchCommand` annotation — users reasonably want
/// to attach `arg(...)` metadata to the injection slot. Without
/// peeling, the kwarg falls through to the type resolver and the
/// whole module is rejected with "type `DispatchCommand` is not
/// supported".
#[test]
fn dispatch_command_kwarg_in_annotated_is_filtered_out() {
    let src = r#"
from typing import Annotated
from toolr.sources import DispatchCommand
from toolr import arg

def f(ctx, *, cpu: str = "1", dispatched: Annotated[DispatchCommand, arg(help="x")]): pass
"#;
    let m = module(src);
    let sources = SourcesImports::from_module(&m);
    let func = m
        .body
        .iter()
        .find_map(|s| match s {
            ruff_python_ast::Stmt::FunctionDef(f) => Some(f.clone()),
            _ => None,
        })
        .unwrap();
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &sources,
        "tools.test",
    );
    assert!(
        args.iter().all(|a| a.name != "dispatched"),
        "expected `dispatched` to be filtered out, got {args:?}"
    );
}

/// String forward-reference annotations (`"DispatchCommand"`) are
/// common under `from __future__ import annotations` or just
/// stylistic forward refs. Must filter out the same way.
#[test]
fn dispatch_command_kwarg_in_string_forward_ref_is_filtered_out() {
    let src = r#"
from toolr.sources import DispatchCommand

def f(ctx, *, cpu: str = "1", dispatched: "DispatchCommand"): pass
"#;
    let m = module(src);
    let sources = SourcesImports::from_module(&m);
    let func = m
        .body
        .iter()
        .find_map(|s| match s {
            ruff_python_ast::Stmt::FunctionDef(f) => Some(f.clone()),
            _ => None,
        })
        .unwrap();
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &sources,
        "tools.test",
    );
    assert!(
        args.iter().all(|a| a.name != "dispatched"),
        "expected `dispatched` to be filtered out, got {args:?}"
    );
}

/// An aliased import (`from toolr.sources import DispatchCommand as DC`)
/// combined with a string forward ref (`"DC"`) should also filter
/// out — the string contents are compared against the local alias
/// table, not just the canonical `"DispatchCommand"` literal.
#[test]
fn dispatch_command_kwarg_in_aliased_string_forward_ref_is_filtered_out() {
    let src = r#"
from toolr.sources import DispatchCommand as DC

def f(ctx, *, cpu: str = "1", dispatched: "DC"): pass
"#;
    let m = module(src);
    let sources = SourcesImports::from_module(&m);
    let func = m
        .body
        .iter()
        .find_map(|s| match s {
            ruff_python_ast::Stmt::FunctionDef(f) => Some(f.clone()),
            _ => None,
        })
        .unwrap();
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &ConstTable::default(),
        &sources,
        "tools.test",
    );
    assert!(
        args.iter().all(|a| a.name != "dispatched"),
        "expected `dispatched` to be filtered out, got {args:?}"
    );
}

/// `param: str = NAME` resolves to the module-level constant when
/// the table is provided. Without the table the default falls
/// through to the `<expr>` sentinel.
#[test]
fn resolves_same_module_string_constant_default() {
    let src = "\
DEFAULT_MODE = \"fast\"

def f(ctx, *, mode: str = DEFAULT_MODE): pass
";
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(src.as_bytes()).unwrap();
    let module = parse_python_file(f.path()).unwrap();
    let consts = ConstTable::from_module(&module);
    let func = module
        .body
        .into_iter()
        .find_map(|s| match s {
            Stmt::FunctionDef(f) => Some(f),
            _ => None,
        })
        .unwrap();
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &consts,
        &SourcesImports::default(),
        "tools.test",
    );
    assert_eq!(args[0].default.as_deref(), Some("fast"));
}

#[test]
fn resolves_int_and_bool_module_constants() {
    let src = "\
MAX_RETRIES = 3
VERBOSE = True

def f(ctx, *, retries: int = MAX_RETRIES, loud: bool = VERBOSE): pass
";
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(src.as_bytes()).unwrap();
    let module = parse_python_file(f.path()).unwrap();
    let consts = ConstTable::from_module(&module);
    let func = module
        .body
        .into_iter()
        .find_map(|s| match s {
            Stmt::FunctionDef(f) => Some(f),
            _ => None,
        })
        .unwrap();
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &consts,
        &SourcesImports::default(),
        "tools.test",
    );
    assert_eq!(args[0].default.as_deref(), Some("3"));
    assert_eq!(args[1].default.as_deref(), Some("true"));
}

/// Names that don't point at a literal constant (function calls,
/// attribute references that aren't enum members, imports from
/// other modules) keep falling through to `<expr>`.
#[test]
fn unresolvable_name_falls_through_to_expr_sentinel() {
    let src = "\
def f(ctx, *, path: str = compute_default()): pass
";
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(src.as_bytes()).unwrap();
    let module = parse_python_file(f.path()).unwrap();
    let consts = ConstTable::from_module(&module);
    let func = module
        .body
        .into_iter()
        .find_map(|s| match s {
            Stmt::FunctionDef(f) => Some(f),
            _ => None,
        })
        .unwrap();
    let args = extract_arguments(
        &func,
        &EnumTable::default(),
        &std::collections::HashMap::new(),
        &consts,
        &SourcesImports::default(),
        "tools.test",
    );
    assert_eq!(args[0].default.as_deref(), Some("<expr>"));
}
