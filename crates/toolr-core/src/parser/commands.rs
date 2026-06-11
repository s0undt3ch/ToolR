//! Extract `@<group>.command`-decorated function definitions.

use std::collections::HashMap;

use ruff_python_ast::{Decorator, Expr, ModModule, Stmt, StmtFunctionDef};

use super::groups::GroupBinding;
use super::parse_docstring;
use super::signatures::extract_arguments;
use super::symbols::ArgSectionTable;
use super::symbols::ConstTable;
use super::symbols::EnumTable;
use super::symbols::TypeAliasTable;
use super::types::{SourcesImports, TypeImports, TypeResolutionError, resolve_arguments};
use crate::manifest::{Command, Origin};

type GlobalVars = HashMap<String, String>;

/// Walk module body for functions decorated with `@<var>.command` (legacy
/// binding style) or `@command(group="…")` (string-path style). Emit
/// one Command per match.
#[allow(clippy::too_many_arguments)] // each context parameter is distinct; bundling would obscure call sites.
pub fn extract_commands(
    module: &ModModule,
    module_path: &str,
    bindings: &[GroupBinding],
    enums: &EnumTable,
    consts: &ConstTable,
    type_imports: &TypeImports,
    sources: &SourcesImports,
    aliases: &TypeAliasTable,
    sections: &ArgSectionTable,
    global_vars: &GlobalVars,
    errors: &mut Vec<TypeResolutionError>,
) -> Vec<Command> {
    // Map `binding_var → group_full_path`. Local bindings shadow the
    // global map so a same-named variable defined in this file wins
    // over an inherited one; otherwise the global cross-file map
    // covers `from ._common import group; @group.command` style.
    let mut by_var: HashMap<String, String> = global_vars.clone();
    for b in bindings {
        by_var.insert(b.var.clone(), b.group.full_path());
    }
    let mut out = Vec::new();
    for stmt in &module.body {
        let Stmt::FunctionDef(func) = stmt else {
            continue;
        };
        let Some(target) = command_decorator(&func.decorator_list) else {
            continue;
        };
        let (group_full_path, override_name) = match target {
            CommandDecorator::LegacyVar { var, explicit_name } => {
                let Some(group_name) = by_var.get(var.as_str()) else {
                    continue;
                };
                (group_name.clone(), explicit_name)
            }
            CommandDecorator::Direct { explicit_name, group } => {
                // `group=` is required on the direct form. If it's
                // missing or the targeted group isn't registered
                // anywhere, we still emit the command — the build-
                // time validator surfaces a clear error pointing the
                // user at `command_group("…")`.
                (group.unwrap_or_default(), explicit_name)
            }
        };
        out.push(build_command(
            func,
            &group_full_path,
            override_name.as_deref(),
            module_path,
            enums,
            consts,
            type_imports,
            sources,
            aliases,
            sections,
            errors,
        ));
    }
    out
}

/// Which flavour of `@command` decorator we saw on a function, plus the
/// metadata pulled out of it.
enum CommandDecorator {
    /// Legacy: `@<binding>.command` or `@<binding>.command("name")` —
    /// `<binding>` is a Python variable name that must resolve to a
    /// known `CommandGroup`. The call form allows overriding the CLI
    /// name via a leading string positional.
    LegacyVar {
        var: String,
        explicit_name: Option<String>,
    },
    /// Modern: `@command(group="dotted.path")` or `@command("name", group=...)`
    /// or bare `@command` (no group — caught by validator).
    Direct {
        explicit_name: Option<String>,
        group: Option<String>,
    },
}

fn command_decorator(decorators: &[Decorator]) -> Option<CommandDecorator> {
    for d in decorators {
        if let Some(meta) = parse_direct_command(&d.expression) {
            return Some(meta);
        }
        if let Some(meta) = parse_legacy_command(&d.expression) {
            return Some(meta);
        }
    }
    None
}

/// Recognise the legacy `@<var>.command` shape, as well as the call form
/// `@<var>.command("explicit-name")` which overrides the derived CLI
/// name. Both shapes resolve `<var>` against the binding tables in
/// `extract_commands`.
fn parse_legacy_command(expr: &Expr) -> Option<CommandDecorator> {
    // Bare attribute: `@<var>.command`.
    if let Expr::Attribute(attr) = expr {
        if attr.attr.as_str() != "command" {
            return None;
        }
        let Expr::Name(n) = attr.value.as_ref() else {
            return None;
        };
        return Some(CommandDecorator::LegacyVar {
            var: n.id.as_str().to_string(),
            explicit_name: None,
        });
    }
    // Call form: `@<var>.command(...)` — func is an Attribute whose
    // value is a Name. The first positional, if a string literal, is
    // the CLI-name override.
    let Expr::Call(call) = expr else {
        return None;
    };
    let Expr::Attribute(attr) = call.func.as_ref() else {
        return None;
    };
    if attr.attr.as_str() != "command" {
        return None;
    }
    let Expr::Name(n) = attr.value.as_ref() else {
        return None;
    };
    let explicit_name = call.arguments.args.first().and_then(literal_str);
    Some(CommandDecorator::LegacyVar {
        var: n.id.as_str().to_string(),
        explicit_name,
    })
}

/// Local copy of the `literal_str` helper used elsewhere in the parser.
/// Keeps this module self-contained without exposing a parser-wide
/// helper as public API.
fn literal_str(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
        _ => None,
    }
}

/// Recognise the new `@command` / `@command("name", group="…")` shape.
fn parse_direct_command(expr: &Expr) -> Option<CommandDecorator> {
    // Bare `@command` — Name expression on the decorator.
    if let Expr::Name(n) = expr {
        if n.id.as_str() == "command" {
            return Some(CommandDecorator::Direct {
                explicit_name: None,
                group: None,
            });
        }
        return None;
    }
    // `@command(...)` — Call whose func is a Name "command".
    let Expr::Call(call) = expr else {
        return None;
    };
    let Expr::Name(callee) = call.func.as_ref() else {
        return None;
    };
    if callee.id.as_str() != "command" {
        return None;
    }
    // First positional, if a string literal, is the explicit name.
    let explicit_name = call
        .arguments
        .args
        .first()
        .and_then(|e| match e {
            Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
            _ => None,
        });
    // `group=` kwarg, string literal only.
    let group = call
        .arguments
        .keywords
        .iter()
        .find(|k| k.arg.as_ref().map(|n| n.as_str()) == Some("group"))
        .and_then(|k| match &k.value {
            Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
            _ => None,
        });
    Some(CommandDecorator::Direct {
        explicit_name,
        group,
    })
}

/// Extract the raw docstring of a function (the leading string-literal
/// statement in its body), or an empty string if it has none.
fn function_docstring(func: &StmtFunctionDef) -> String {
    let Some(first) = func.body.first() else {
        return String::new();
    };
    let Stmt::Expr(e) = first else {
        return String::new();
    };
    let Expr::StringLiteral(s) = e.value.as_ref() else {
        return String::new();
    };
    s.value.to_str().to_string()
}

#[allow(clippy::too_many_arguments)] // contextual parameters; bundling would obscure call sites.
fn build_command(
    func: &StmtFunctionDef,
    group: &str,
    override_name: Option<&str>,
    module_path: &str,
    enums: &EnumTable,
    consts: &ConstTable,
    type_imports: &TypeImports,
    sources: &SourcesImports,
    aliases: &TypeAliasTable,
    sections: &ArgSectionTable,
    errors: &mut Vec<TypeResolutionError>,
) -> Command {
    let raw_doc = function_docstring(func);
    let parsed = parse_docstring(&raw_doc);
    let summary = parsed
        .as_ref()
        .map(|d| d.short_description.clone())
        .unwrap_or_default();
    // Hand clap a multi-section render (short + long + Examples/Notes/…)
    // for `--help` so users see more than the first paragraph there.
    // The `summary` field separately drives `-h`'s shorter `about` slot.
    let description = parsed
        .as_ref()
        .map(|d| d.full_description())
        .unwrap_or_default();
    let mut arguments = extract_arguments(func, enums, consts, sources);
    if let Some(d) = parsed.as_ref() {
        for arg in &mut arguments {
            if let Some(Some(help)) = d.params.get(&arg.name) {
                arg.help = help.clone();
            }
        }
    }
    resolve_arguments(
        func,
        &mut arguments,
        enums,
        type_imports,
        sources,
        aliases,
        sections,
        module_path,
        errors,
    );
    // Backfill `allowed_values` from `resolved_type` for arguments whose
    // type annotation is a `Literal[...]` alias declared in another file.
    // `extract_arguments` runs before `resolve_arguments` and only consults
    // `EnumTable` for allowed-value collection, so a bare `Name("Mode")`
    // where `Mode = Literal["fast", "slow"]` lives in another module
    // would land here with `allowed_values: []`. After the resolver fills
    // `resolved_type`, copy the Literal's values across.
    for arg in &mut arguments {
        if arg.allowed_values.is_empty() {
            if let Some(crate::parser::types::SupportedType::Literal(values)) = &arg.resolved_type {
                arg.allowed_values = values.clone();
            }
        }
    }
    let cli_name = override_name
        .map(str::to_string)
        .unwrap_or_else(|| func.name.as_str().replace('_', "-"));
    Command {
        name: cli_name,
        group: group.to_string(),
        module: module_path.to_string(),
        function: func.name.as_str().to_string(),
        summary,
        description,
        arguments,
        origin: Origin::Static,
        dispatched_from: None,
        is_dispatcher: false,
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn extracts_decorated_function_as_command() {
        let src = r#"group = command_group("ci", "CI utilities")

@group.command
def generate_build_matrix(ctx):
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(&m, "tools.ci", &bindings, &EnumTable::default(), &ConstTable::default(), &TypeImports::default(), &SourcesImports::default(), &TypeAliasTable::default(), &ArgSectionTable::default(), &HashMap::new(), &mut Vec::new());
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
        let commands = extract_commands(&m, "tools.x", &bindings, &EnumTable::default(), &ConstTable::default(), &TypeImports::default(), &SourcesImports::default(), &TypeAliasTable::default(), &ArgSectionTable::default(), &HashMap::new(), &mut Vec::new());
        assert!(commands.is_empty());
    }

    #[test]
    fn ignores_undecorated_functions() {
        let src = r#"group = command_group("ci", "CI utilities")

def bare_function(ctx):
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(&m, "tools.ci", &bindings, &EnumTable::default(), &ConstTable::default(), &TypeImports::default(), &SourcesImports::default(), &TypeAliasTable::default(), &ArgSectionTable::default(), &HashMap::new(), &mut Vec::new());
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
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(&m, "tools.ci", &bindings, &EnumTable::default(), &ConstTable::default(), &TypeImports::default(), &SourcesImports::default(), &TypeAliasTable::default(), &ArgSectionTable::default(), &HashMap::new(), &mut Vec::new());
        assert_eq!(commands[0].summary, "Say hello.");
    }

    #[test]
    fn direct_command_decorator_attaches_to_named_group() {
        let src = r#"command_group("ci", "CI utilities")

@command(group="ci")
def check_run_build(ctx):
    """Check the run."""
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.ci",
            &bindings,
            &EnumTable::default(),
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
    fn direct_command_decorator_supports_explicit_name() {
        let src = r#"command_group("ci.helm-diff-pr-comment", "Helm diff")

@command("snippet-checker", group="ci.helm-diff-pr-comment")
def check_snippets(ctx):
    """Check snippets."""
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.ci",
            &bindings,
            &EnumTable::default(),
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
    fn legacy_var_decorator_still_works() {
        let src = r#"group = command_group("ci", "CI")

@group.command
def hello(ctx):
    """Hi."""
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.ci",
            &bindings,
            &EnumTable::default(),
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
    fn legacy_decorator_attribute_form_no_explicit_name() {
        let src = r#"group = command_group("ci", "CI")

@group.command
def do_thing(ctx):
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.ci",
            &bindings,
            &EnumTable::default(),
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
        let decorated = command_decorator(&func.decorator_list).unwrap();
        match decorated {
            CommandDecorator::LegacyVar { var, explicit_name } => {
                assert_eq!(var, "group");
                assert_eq!(explicit_name, None);
            }
            _ => panic!("expected LegacyVar"),
        }
        // And confirm the function's own name is used (kebab-cased).
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "do-thing");
    }

    #[test]
    fn legacy_decorator_call_form_with_explicit_name() {
        let src = r#"group = command_group("ci", "CI")

@group.command("hello")
def do_thing(ctx):
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.ci",
            &bindings,
            &EnumTable::default(),
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
        let decorated = command_decorator(&func.decorator_list).unwrap();
        match decorated {
            CommandDecorator::LegacyVar { var, explicit_name } => {
                assert_eq!(var, "group");
                assert_eq!(explicit_name.as_deref(), Some("hello"));
            }
            _ => panic!("expected LegacyVar"),
        }
        // CLI name comes from the decorator argument, not the function.
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "hello");
        assert_eq!(commands[0].function, "do_thing");
    }

    #[test]
    fn legacy_decorator_call_form_without_name_arg() {
        let src = r#"group = command_group("ci", "CI")

@group.command()
def do_thing(ctx):
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.ci",
            &bindings,
            &EnumTable::default(),
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
        let decorated = command_decorator(&func.decorator_list).unwrap();
        match decorated {
            CommandDecorator::LegacyVar { var, explicit_name } => {
                assert_eq!(var, "group");
                assert_eq!(explicit_name, None);
            }
            _ => panic!("expected LegacyVar"),
        }
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
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(&m, "tools.ci", &bindings, &EnumTable::default(), &ConstTable::default(), &TypeImports::default(), &SourcesImports::default(), &TypeAliasTable::default(), &ArgSectionTable::default(), &HashMap::new(), &mut Vec::new());
        let name_arg = commands[0]
            .arguments
            .iter()
            .find(|a| a.name == "name")
            .unwrap();
        assert_eq!(name_arg.help, "Who to greet.");
    }
}
