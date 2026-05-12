//! Extract `@<group>.command`-decorated function definitions.

use std::collections::HashMap;

use ruff_python_ast::{Decorator, Expr, ModModule, Stmt, StmtFunctionDef};

use super::groups::GroupBinding;
use super::signatures::extract_arguments;
use crate::manifest::{Command, Origin};

/// Walk module body for functions decorated with `@<var>.command` where
/// `<var>` matches a known group binding. Emit one Command per match.
pub fn extract_commands(
    module: &ModModule,
    module_path: &str,
    bindings: &[GroupBinding],
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
        out.push(build_command(func, group_name, module_path));
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

fn build_command(func: &StmtFunctionDef, group: &str, module_path: &str) -> Command {
    Command {
        name: func.name.as_str().replace('_', "-"),
        group: group.to_string(),
        module: module_path.to_string(),
        function: func.name.as_str().to_string(),
        summary: String::new(),
        description: String::new(),
        arguments: extract_arguments(func),
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
        let commands = extract_commands(&m, "tools.ci", &bindings);
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
        let commands = extract_commands(&m, "tools.x", &bindings);
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
        let commands = extract_commands(&m, "tools.ci", &bindings);
        assert!(commands.is_empty());
    }
}
