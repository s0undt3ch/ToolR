//! Extract `@<group>.command`-decorated function definitions.

use std::collections::HashMap;

use ruff_python_ast::{Decorator, Expr, ModModule, Stmt, StmtFunctionDef};

use super::groups::GroupBinding;
use super::signatures::extract_arguments;
use super::symbols::EnumTable;
use crate::SimpleDocstringParser;
use crate::manifest::{Command, Origin};

/// Walk module body for functions decorated with `@<var>.command` where
/// `<var>` matches a known group binding. Emit one Command per match.
pub fn extract_commands(
    module: &ModModule,
    module_path: &str,
    bindings: &[GroupBinding],
    enums: &EnumTable,
) -> Vec<Command> {
    let by_var: HashMap<&str, &str> = bindings
        .iter()
        .map(|b| (b.var.as_str(), b.group.name.as_str()))
        .collect();
    let mut out = Vec::new();
    for stmt in &module.body {
        let Stmt::FunctionDef(func) = stmt else {
            continue;
        };
        let Some(group_var) = command_decorator_target(&func.decorator_list) else {
            continue;
        };
        let Some(&group_name) = by_var.get(group_var.as_str()) else {
            continue;
        };
        out.push(build_command(func, group_name, module_path, enums));
    }
    out
}

fn command_decorator_target(decorators: &[Decorator]) -> Option<String> {
    for d in decorators {
        if let Expr::Attribute(attr) = &d.expression {
            if attr.attr.as_str() == "command" {
                if let Expr::Name(n) = attr.value.as_ref() {
                    return Some(n.id.as_str().to_string());
                }
            }
        }
    }
    None
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

fn build_command(
    func: &StmtFunctionDef,
    group: &str,
    module_path: &str,
    enums: &EnumTable,
) -> Command {
    let raw_doc = function_docstring(func);
    let parsed = SimpleDocstringParser::new().parse(&raw_doc).ok();
    let summary = parsed
        .as_ref()
        .map(|d| d.short_description.clone())
        .unwrap_or_default();
    let description = parsed
        .as_ref()
        .and_then(|d| d.long_description.clone())
        .unwrap_or_default();
    let mut arguments = extract_arguments(func, enums);
    if let Some(d) = parsed.as_ref() {
        for arg in &mut arguments {
            if let Some(Some(help)) = d.params.get(&arg.name) {
                arg.help = help.clone();
            }
        }
    }
    Command {
        name: func.name.as_str().replace('_', "-"),
        group: group.to_string(),
        module: module_path.to_string(),
        function: func.name.as_str().to_string(),
        summary,
        description,
        arguments,
        imports: Vec::new(),
        origin: Origin::Static,
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
        let bindings = extract_groups(&m, "");
        let commands = extract_commands(&m, "tools.ci", &bindings, &EnumTable::default());
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
        let commands = extract_commands(&m, "tools.x", &bindings, &EnumTable::default());
        assert!(commands.is_empty());
    }

    #[test]
    fn ignores_undecorated_functions() {
        let src = r#"group = command_group("ci", "CI utilities")

def bare_function(ctx):
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "");
        let commands = extract_commands(&m, "tools.ci", &bindings, &EnumTable::default());
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
        let bindings = extract_groups(&m, "");
        let commands = extract_commands(&m, "tools.ci", &bindings, &EnumTable::default());
        assert_eq!(commands[0].summary, "Say hello.");
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
        let bindings = extract_groups(&m, "");
        let commands = extract_commands(&m, "tools.ci", &bindings, &EnumTable::default());
        let name_arg = commands[0]
            .arguments
            .iter()
            .find(|a| a.name == "name")
            .unwrap();
        assert_eq!(name_arg.help, "Who to greet.");
    }
}
