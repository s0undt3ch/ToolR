use super::*;
use crate::parser::groups::extract_groups;
use crate::parser::parse_python_file;
use std::io::Write;
use tempfile::NamedTempFile;

fn parse_src(src: &str) -> ModModule {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(src.as_bytes()).unwrap();
    parse_python_file(f.path()).unwrap()
}

/// `all_imports` for a module parsed as `tools.ci`, so `from toolr import command` counts.
fn imports_of(m: &ModModule) -> HashMap<String, ImportTable> {
    ["tools.ci", "tools.repro", "tools.x"]
        .into_iter()
        .map(|path| (path.to_string(), ImportTable::from_module(m, path, false)))
        .collect()
}

#[test]
fn extracts_decorated_function_as_command() {
    let src = r#"group = command_group("ci", "CI utilities")

@group.command
def generate_build_matrix(ctx):
    pass
"#;
    let m = parse_src(src);
    let bindings = extract_groups(
        &m,
        None,
        &HashMap::new(),
        &ToolrNames::new(&imports_of(&m)),
        "tools.ci",
    );
    let commands = extract_commands(
        &m,
        "tools.ci",
        &bindings,
        &EnumTable::default(),
        &imports_of(&m),
        &ToolrNames::new(&imports_of(&m)),
        &ConstTable::default(),
        &TypeImports::default(),
        &SourcesImports::default(),
        &TypeAliasTable::default(),
        &ArgSectionTable::default(),
        &HashMap::new(),
        &mut Vec::new(),
    );
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "generate-build-matrix");
    assert_eq!(commands[0].group, "ci");
    assert_eq!(commands[0].function, "generate_build_matrix");
    assert_eq!(commands[0].module, "tools.ci");
}

#[test]
fn ignores_functions_with_unknown_group_var() {
    let src = r#"
@other.command
def x(ctx):
    pass
"#;
    let m = parse_src(src);
    let bindings = vec![];
    let commands = extract_commands(
        &m,
        "tools.x",
        &bindings,
        &EnumTable::default(),
        &imports_of(&m),
        &ToolrNames::new(&imports_of(&m)),
        &ConstTable::default(),
        &TypeImports::default(),
        &SourcesImports::default(),
        &TypeAliasTable::default(),
        &ArgSectionTable::default(),
        &HashMap::new(),
        &mut Vec::new(),
    );
    assert!(commands.is_empty());
}

#[test]
fn ignores_undecorated_functions() {
    let src = r#"group = command_group("ci", "CI utilities")

def bare_function(ctx):
    pass
"#;
    let m = parse_src(src);
    let bindings = extract_groups(
        &m,
        None,
        &HashMap::new(),
        &ToolrNames::new(&imports_of(&m)),
        "tools.ci",
    );
    let commands = extract_commands(
        &m,
        "tools.ci",
        &bindings,
        &EnumTable::default(),
        &imports_of(&m),
        &ToolrNames::new(&imports_of(&m)),
        &ConstTable::default(),
        &TypeImports::default(),
        &SourcesImports::default(),
        &TypeAliasTable::default(),
        &ArgSectionTable::default(),
        &HashMap::new(),
        &mut Vec::new(),
    );
    assert!(commands.is_empty());
}

#[test]
fn populates_summary_from_first_docstring_line() {
    let src = r#"group = command_group("ci", "CI utilities")

@group.command
def hello(ctx):
    """Say hello."""
    pass
"#;
    let m = parse_src(src);
    let bindings = extract_groups(
        &m,
        None,
        &HashMap::new(),
        &ToolrNames::new(&imports_of(&m)),
        "tools.ci",
    );
    let commands = extract_commands(
        &m,
        "tools.ci",
        &bindings,
        &EnumTable::default(),
        &imports_of(&m),
        &ToolrNames::new(&imports_of(&m)),
        &ConstTable::default(),
        &TypeImports::default(),
        &SourcesImports::default(),
        &TypeAliasTable::default(),
        &ArgSectionTable::default(),
        &HashMap::new(),
        &mut Vec::new(),
    );
    assert_eq!(commands[0].summary, "Say hello.");
}

#[test]
fn string_path_decorator_attaches_to_named_group() {
    let src = r#"from toolr import command
command_group("ci", "CI utilities")

@command(group="ci")
def check_run_build(ctx):
    """Check the run."""
    pass
"#;
    let m = parse_src(src);
    let bindings = extract_groups(
        &m,
        None,
        &HashMap::new(),
        &ToolrNames::new(&imports_of(&m)),
        "tools.ci",
    );
    let commands = extract_commands(
        &m,
        "tools.ci",
        &bindings,
        &EnumTable::default(),
        &imports_of(&m),
        &ToolrNames::new(&imports_of(&m)),
        &ConstTable::default(),
        &TypeImports::default(),
        &SourcesImports::default(),
        &TypeAliasTable::default(),
        &ArgSectionTable::default(),
        &HashMap::new(),
        &mut Vec::new(),
    );
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "check-run-build");
    assert_eq!(commands[0].group, "ci");
}

#[test]
fn string_path_decorator_supports_explicit_name() {
    let src = r#"from toolr import command
command_group("ci.helm-diff-pr-comment", "Helm diff")

@command("snippet-checker", group="ci.helm-diff-pr-comment")
def check_snippets(ctx):
    """Check snippets."""
    pass
"#;
    let m = parse_src(src);
    let bindings = extract_groups(
        &m,
        None,
        &HashMap::new(),
        &ToolrNames::new(&imports_of(&m)),
        "tools.ci",
    );
    let commands = extract_commands(
        &m,
        "tools.ci",
        &bindings,
        &EnumTable::default(),
        &imports_of(&m),
        &ToolrNames::new(&imports_of(&m)),
        &ConstTable::default(),
        &TypeImports::default(),
        &SourcesImports::default(),
        &TypeAliasTable::default(),
        &ArgSectionTable::default(),
        &HashMap::new(),
        &mut Vec::new(),
    );
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "snippet-checker");
    assert_eq!(commands[0].group, "ci.helm-diff-pr-comment");
    assert_eq!(commands[0].function, "check_snippets");
}

#[test]
fn string_path_decorator_supports_name_keyword() {
    let src = r#"from toolr import command
command_group("ci.helm-diff-pr-comment", "Helm diff")

@command(name="snippet-checker", group="ci.helm-diff-pr-comment")
def check_snippets(ctx):
    """Check snippets."""
    pass
"#;
    let m = parse_src(src);
    let bindings = extract_groups(
        &m,
        None,
        &HashMap::new(),
        &ToolrNames::new(&imports_of(&m)),
        "tools.ci",
    );
    let commands = extract_commands(
        &m,
        "tools.ci",
        &bindings,
        &EnumTable::default(),
        &imports_of(&m),
        &ToolrNames::new(&imports_of(&m)),
        &ConstTable::default(),
        &TypeImports::default(),
        &SourcesImports::default(),
        &TypeAliasTable::default(),
        &ArgSectionTable::default(),
        &HashMap::new(),
        &mut Vec::new(),
    );
    assert!(detect_name_conflicts(&m, "tools.ci", &ToolrNames::new(&imports_of(&m))).is_empty());
    assert_eq!(commands.len(), 1);
    // The `name=` keyword wins over the `check-snippets` function name.
    assert_eq!(commands[0].name, "snippet-checker");
    assert_eq!(commands[0].group, "ci.helm-diff-pr-comment");
    assert_eq!(commands[0].function, "check_snippets");
}

#[test]
fn optional_list_keyword_resolves_to_repeated() {
    // Regression for #435: `list[T] | None` was classified as
    // `Optional` (single-value) by the syntactic pass, since it
    // can't see through the `| None` union — only the post-resolution
    // override in `resolve_arguments` catches it.
    let src = r#"from toolr import command
command_group("repro", "Repro")

@command(group="repro")
def go(ctx, items: list[str] | None = None):
    """Repro."""
    pass
"#;
    let m = parse_src(src);
    let bindings = extract_groups(
        &m,
        None,
        &HashMap::new(),
        &ToolrNames::new(&imports_of(&m)),
        "tools.ci",
    );
    let commands = extract_commands(
        &m,
        "tools.repro",
        &bindings,
        &EnumTable::default(),
        &imports_of(&m),
        &ToolrNames::new(&imports_of(&m)),
        &ConstTable::default(),
        &TypeImports::default(),
        &SourcesImports::default(),
        &TypeAliasTable::default(),
        &ArgSectionTable::default(),
        &HashMap::new(),
        &mut Vec::new(),
    );
    assert_eq!(commands.len(), 1);
    let items = &commands[0].arguments[0];
    assert_eq!(items.name, "items");
    assert_eq!(items.kind, crate::manifest::ArgumentKind::Repeated);
    assert_eq!(
        items.resolved_type,
        Some(crate::parser::types::SupportedType::Optional(Box::new(
            crate::parser::types::SupportedType::List(Box::new(
                crate::parser::types::SupportedType::Str
            ))
        )))
    );
}

#[test]
fn positional_only_params_resolve_in_lockstep() {
    let src = r#"from toolr import command
command_group("repro", "Repro")

@command(group="repro")
def go(ctx, name: str, /, count: int, *rest: int):
    """Repro."""
    pass
"#;
    let m = parse_src(src);
    let bindings = extract_groups(
        &m,
        None,
        &HashMap::new(),
        &ToolrNames::new(&imports_of(&m)),
        "tools.ci",
    );
    let commands = extract_commands(
        &m,
        "tools.repro",
        &bindings,
        &EnumTable::default(),
        &imports_of(&m),
        &ToolrNames::new(&imports_of(&m)),
        &ConstTable::default(),
        &TypeImports::default(),
        &SourcesImports::default(),
        &TypeAliasTable::default(),
        &ArgSectionTable::default(),
        &HashMap::new(),
        &mut Vec::new(),
    );
    use crate::manifest::ArgumentKind;
    use crate::parser::types::SupportedType;
    let got: Vec<_> = commands[0]
        .arguments
        .iter()
        .map(|a| (a.name.as_str(), a.kind, a.resolved_type.clone()))
        .collect();
    assert_eq!(
        got,
        [
            ("name", ArgumentKind::Positional, Some(SupportedType::Str)),
            ("count", ArgumentKind::Positional, Some(SupportedType::Int)),
            (
                "rest",
                ArgumentKind::VarPositional,
                Some(SupportedType::Int)
            ),
        ]
    );
}

#[test]
fn optional_tuple_keyword_resolves_to_repeated() {
    // Same fix as `optional_list_keyword_resolves_to_repeated`, for
    // the other collection shape `is_list_like_annotation` accepts.
    let src = r#"from toolr import command
command_group("repro", "Repro")

@command(group="repro")
def go(ctx, pair: tuple[str, int] | None = None):
    """Repro."""
    pass
"#;
    let m = parse_src(src);
    let bindings = extract_groups(
        &m,
        None,
        &HashMap::new(),
        &ToolrNames::new(&imports_of(&m)),
        "tools.ci",
    );
    let commands = extract_commands(
        &m,
        "tools.repro",
        &bindings,
        &EnumTable::default(),
        &imports_of(&m),
        &ToolrNames::new(&imports_of(&m)),
        &ConstTable::default(),
        &TypeImports::default(),
        &SourcesImports::default(),
        &TypeAliasTable::default(),
        &ArgSectionTable::default(),
        &HashMap::new(),
        &mut Vec::new(),
    );
    assert_eq!(commands.len(), 1);
    let pair = &commands[0].arguments[0];
    assert_eq!(pair.name, "pair");
    assert_eq!(pair.kind, crate::manifest::ArgumentKind::Repeated);
    assert_eq!(
        pair.resolved_type,
        Some(crate::parser::types::SupportedType::Optional(Box::new(
            crate::parser::types::SupportedType::Tuple(vec![
                crate::parser::types::SupportedType::Str,
                crate::parser::types::SupportedType::Int,
            ])
        )))
    );
}

#[test]
fn binding_decorator_rejects_positional_and_name_keyword() {
    let src = r#"group = command_group("ci", "CI")

@group.command("positional", name="keyword")
def do_thing(ctx):
    pass
"#;
    let m = parse_src(src);
    let func = m
        .body
        .iter()
        .find_map(|s| match s {
            Stmt::FunctionDef(f) => Some(f),
            _ => None,
        })
        .unwrap();
    // The decorator resolves to no explicit name, flagged as a conflict.
    assert_eq!(
        command_decorator(
            &func.decorator_list,
            &ToolrNames::new(&imports_of(&m)),
            "tools.ci"
        )
        .unwrap(),
        CommandDecorator::Binding {
            var: "group".to_string(),
            explicit_name: None,
            name_conflict: true,
        }
    );
    let conflicts = detect_name_conflicts(&m, "tools.ci", &ToolrNames::new(&imports_of(&m)));
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].module, "tools.ci");
    assert_eq!(conflicts[0].function, "do_thing");
}

#[test]
fn string_path_decorator_rejects_positional_and_name_keyword() {
    let src = r#"from toolr import command
command_group("ci", "CI")

@command("positional", name="keyword", group="ci")
def do_thing(ctx):
    pass
"#;
    let m = parse_src(src);
    let func = m
        .body
        .iter()
        .find_map(|s| match s {
            Stmt::FunctionDef(f) => Some(f),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        command_decorator(
            &func.decorator_list,
            &ToolrNames::new(&imports_of(&m)),
            "tools.ci"
        )
        .unwrap(),
        CommandDecorator::StringPath {
            explicit_name: None,
            group: Some("ci".to_string()),
            name_conflict: true,
        }
    );
    let conflicts = detect_name_conflicts(&m, "tools.ci", &ToolrNames::new(&imports_of(&m)));
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].function, "do_thing");
}

#[test]
fn binding_decorator_works() {
    let src = r#"group = command_group("ci", "CI")

@group.command
def hello(ctx):
    """Hi."""
    pass
"#;
    let m = parse_src(src);
    let bindings = extract_groups(
        &m,
        None,
        &HashMap::new(),
        &ToolrNames::new(&imports_of(&m)),
        "tools.ci",
    );
    let commands = extract_commands(
        &m,
        "tools.ci",
        &bindings,
        &EnumTable::default(),
        &imports_of(&m),
        &ToolrNames::new(&imports_of(&m)),
        &ConstTable::default(),
        &TypeImports::default(),
        &SourcesImports::default(),
        &TypeAliasTable::default(),
        &ArgSectionTable::default(),
        &HashMap::new(),
        &mut Vec::new(),
    );
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].group, "ci");
}

#[test]
fn binding_decorator_attribute_form_no_explicit_name() {
    let src = r#"group = command_group("ci", "CI")

@group.command
def do_thing(ctx):
    pass
"#;
    let m = parse_src(src);
    let bindings = extract_groups(
        &m,
        None,
        &HashMap::new(),
        &ToolrNames::new(&imports_of(&m)),
        "tools.ci",
    );
    let commands = extract_commands(
        &m,
        "tools.ci",
        &bindings,
        &EnumTable::default(),
        &imports_of(&m),
        &ToolrNames::new(&imports_of(&m)),
        &ConstTable::default(),
        &TypeImports::default(),
        &SourcesImports::default(),
        &TypeAliasTable::default(),
        &ArgSectionTable::default(),
        &HashMap::new(),
        &mut Vec::new(),
    );
    // Probe the decorator parser directly to assert variant shape.
    let func = m
        .body
        .iter()
        .find_map(|s| match s {
            Stmt::FunctionDef(f) => Some(f),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        command_decorator(
            &func.decorator_list,
            &ToolrNames::new(&imports_of(&m)),
            "tools.ci"
        )
        .unwrap(),
        CommandDecorator::Binding {
            var: "group".to_string(),
            explicit_name: None,
            name_conflict: false,
        }
    );
    // And confirm the function's own name is used (kebab-cased).
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "do-thing");
}

#[test]
fn binding_decorator_call_form_with_explicit_name() {
    let src = r#"group = command_group("ci", "CI")

@group.command("hello")
def do_thing(ctx):
    pass
"#;
    let m = parse_src(src);
    let bindings = extract_groups(
        &m,
        None,
        &HashMap::new(),
        &ToolrNames::new(&imports_of(&m)),
        "tools.ci",
    );
    let commands = extract_commands(
        &m,
        "tools.ci",
        &bindings,
        &EnumTable::default(),
        &imports_of(&m),
        &ToolrNames::new(&imports_of(&m)),
        &ConstTable::default(),
        &TypeImports::default(),
        &SourcesImports::default(),
        &TypeAliasTable::default(),
        &ArgSectionTable::default(),
        &HashMap::new(),
        &mut Vec::new(),
    );
    let func = m
        .body
        .iter()
        .find_map(|s| match s {
            Stmt::FunctionDef(f) => Some(f),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        command_decorator(
            &func.decorator_list,
            &ToolrNames::new(&imports_of(&m)),
            "tools.ci"
        )
        .unwrap(),
        CommandDecorator::Binding {
            var: "group".to_string(),
            explicit_name: Some("hello".to_string()),
            name_conflict: false,
        }
    );
    // CLI name comes from the decorator argument, not the function.
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "hello");
    assert_eq!(commands[0].function, "do_thing");
}

#[test]
fn binding_decorator_call_form_with_name_keyword() {
    let src = r#"group = command_group("ci", "CI")

@group.command(name="collect")
def collect_data(ctx):
    pass
"#;
    let m = parse_src(src);
    let bindings = extract_groups(
        &m,
        None,
        &HashMap::new(),
        &ToolrNames::new(&imports_of(&m)),
        "tools.ci",
    );
    let commands = extract_commands(
        &m,
        "tools.ci",
        &bindings,
        &EnumTable::default(),
        &imports_of(&m),
        &ToolrNames::new(&imports_of(&m)),
        &ConstTable::default(),
        &TypeImports::default(),
        &SourcesImports::default(),
        &TypeAliasTable::default(),
        &ArgSectionTable::default(),
        &HashMap::new(),
        &mut Vec::new(),
    );
    let func = m
        .body
        .iter()
        .find_map(|s| match s {
            Stmt::FunctionDef(f) => Some(f),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        command_decorator(
            &func.decorator_list,
            &ToolrNames::new(&imports_of(&m)),
            "tools.ci"
        )
        .unwrap(),
        CommandDecorator::Binding {
            var: "group".to_string(),
            explicit_name: Some("collect".to_string()),
            name_conflict: false,
        }
    );
    // The `name=` keyword wins over the hyphenated function name
    // (`collect`, not the `collect-data` the function name would give).
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "collect");
    assert_eq!(commands[0].function, "collect_data");
}

#[test]
fn binding_decorator_call_form_without_name_arg() {
    let src = r#"group = command_group("ci", "CI")

@group.command()
def do_thing(ctx):
    pass
"#;
    let m = parse_src(src);
    let bindings = extract_groups(
        &m,
        None,
        &HashMap::new(),
        &ToolrNames::new(&imports_of(&m)),
        "tools.ci",
    );
    let commands = extract_commands(
        &m,
        "tools.ci",
        &bindings,
        &EnumTable::default(),
        &imports_of(&m),
        &ToolrNames::new(&imports_of(&m)),
        &ConstTable::default(),
        &TypeImports::default(),
        &SourcesImports::default(),
        &TypeAliasTable::default(),
        &ArgSectionTable::default(),
        &HashMap::new(),
        &mut Vec::new(),
    );
    let func = m
        .body
        .iter()
        .find_map(|s| match s {
            Stmt::FunctionDef(f) => Some(f),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        command_decorator(
            &func.decorator_list,
            &ToolrNames::new(&imports_of(&m)),
            "tools.ci"
        )
        .unwrap(),
        CommandDecorator::Binding {
            var: "group".to_string(),
            explicit_name: None,
            name_conflict: false,
        }
    );
    // Falls back to function name (kebab-cased) when no override.
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "do-thing");
}

#[test]
fn populates_arg_help_from_args_section() {
    let src = r#"group = command_group("ci", "CI utilities")

@group.command
def hello(ctx, name="world"):
    """Say hello.

    Args:
        name: Who to greet.
    """
    pass
"#;
    let m = parse_src(src);
    let bindings = extract_groups(
        &m,
        None,
        &HashMap::new(),
        &ToolrNames::new(&imports_of(&m)),
        "tools.ci",
    );
    let commands = extract_commands(
        &m,
        "tools.ci",
        &bindings,
        &EnumTable::default(),
        &imports_of(&m),
        &ToolrNames::new(&imports_of(&m)),
        &ConstTable::default(),
        &TypeImports::default(),
        &SourcesImports::default(),
        &TypeAliasTable::default(),
        &ArgSectionTable::default(),
        &HashMap::new(),
        &mut Vec::new(),
    );
    let name_arg = commands[0]
        .arguments
        .iter()
        .find(|a| a.name == "name")
        .unwrap();
    assert_eq!(name_arg.help, "Who to greet.");
}

/// `ToolrNames` over `modules` (`(path, source)`), asking about the first.
fn binding_of(modules: &[(&str, &str)], name: &str, member: &'static str) -> Binding {
    let all = imports_for(modules);
    ToolrNames::new(&all).binds_toolr_member(modules[0].0, name, member, 0)
}

fn imports_for(modules: &[(&str, &str)]) -> HashMap<String, ImportTable> {
    modules
        .iter()
        .map(|(path, src)| {
            (
                path.to_string(),
                ImportTable::from_module(&parse_src(src), path, false),
            )
        })
        .collect()
}

#[test]
fn bare_annotation_does_not_hide_an_imported_command() {
    let src = "from toolr import command\ncommand: object\n";
    assert_eq!(
        binding_of(&[("tools.m", src)], "command", "command"),
        Binding::Toolr
    );
}

#[test]
fn self_copy_of_command_resolves_through_the_import() {
    for src in [
        "from toolr import command\ncommand = command\n",
        "from toolr import command\ncommand: object = command\n",
    ] {
        assert_eq!(
            binding_of(&[("tools.m", src)], "command", "command"),
            Binding::Toolr
        );
    }
    assert_eq!(
        binding_of(&[("tools.m", "command = command\n")], "command", "command"),
        Binding::Other
    );
}

#[test]
fn memo_bounds_the_walk_and_agrees_with_a_fresh_cache() {
    // Every module star-imports every other, so an unmemoised walk fans out
    // exponentially in the depth cap.
    const MODULES: usize = 6;
    let names: Vec<String> = (0..MODULES).map(|n| format!("tools.m{n}")).collect();
    let sources: Vec<String> = (0..MODULES)
        .map(|n| {
            let mut src = String::new();
            for o in (0..MODULES).filter(|o| *o != n) {
                src += &format!("from tools.m{o} import *\n");
            }
            src += &format!("def helper{n}():\n    pass\n");
            src
        })
        .collect();
    let modules: Vec<(&str, &str)> = names
        .iter()
        .map(String::as_str)
        .zip(sources.iter().map(String::as_str))
        .collect();
    let all = imports_for(&modules);
    let shared = ToolrNames::new(&all);
    for module in &names {
        for o in 0..MODULES {
            let name = format!("helper{o}");
            let fresh = ToolrNames::new(&all).binds_toolr_member(module, &name, "command", 0);
            assert_eq!(
                shared.binds_toolr_member(module, &name, "command", 0),
                fresh
            );
            assert_eq!(fresh, Binding::Other);
        }
    }
    // At most one entry per (module, name, depth in 0..=MAX_REEXPORT_DEPTH).
    assert!(shared.cache.borrow().len() <= MODULES * MODULES * (MAX_REEXPORT_DEPTH + 1));
}
