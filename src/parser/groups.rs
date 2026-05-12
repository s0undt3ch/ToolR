//! Extract `group = command_group(...)` assignments from a module AST.

use ruff_python_ast::{Expr, ExprCall, ModModule, Stmt, StmtAssign};

use crate::manifest::{Group, Origin};

/// A group binding extracted from source. The `var` is the Python local name
/// that subsequent `@var.command` decorators reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupBinding {
    pub var: String,
    pub group: Group,
}

/// Walk the module's top-level statements and collect group bindings.
pub fn extract_groups(module: &ModModule, module_docstring: &str) -> Vec<GroupBinding> {
    let mut out = Vec::new();
    for stmt in &module.body {
        let Stmt::Assign(StmtAssign { targets, value, .. }) = stmt else {
            continue;
        };
        // Only handle `single_var = command_group(...)`.
        let Some(var_name) = single_name_target(targets) else {
            continue;
        };
        let Expr::Call(call) = value.as_ref() else {
            continue;
        };
        if !is_command_group_call(call) {
            continue;
        }
        let Some(binding) = parse_group_call(call, &var_name, module_docstring) else {
            continue;
        };
        out.push(binding);
    }
    out
}

fn single_name_target(targets: &[Expr]) -> Option<String> {
    if targets.len() != 1 {
        return None;
    }
    match &targets[0] {
        Expr::Name(n) => Some(n.id.as_str().to_string()),
        _ => None,
    }
}

fn is_command_group_call(call: &ExprCall) -> bool {
    match call.func.as_ref() {
        Expr::Name(n) => n.id.as_str() == "command_group",
        Expr::Attribute(a) => a.attr.as_str() == "command_group",
        _ => false,
    }
}

fn parse_group_call(call: &ExprCall, var: &str, module_docstring: &str) -> Option<GroupBinding> {
    // Positional args: name, title. Keyword `docstring` may be __doc__.
    let name = call.arguments.args.first().and_then(literal_str)?;
    let title = call
        .arguments
        .args
        .get(1)
        .and_then(literal_str)
        .unwrap_or_default();
    let description = call
        .arguments
        .keywords
        .iter()
        .find(|k| k.arg.as_ref().map(|n| n.as_str()) == Some("docstring"))
        .and_then(|k| match &k.value {
            Expr::Name(n) if n.id.as_str() == "__doc__" => Some(module_docstring.to_string()),
            e => literal_str(e),
        })
        .unwrap_or_default();
    Some(GroupBinding {
        var: var.to_string(),
        group: Group {
            name,
            title,
            description,
            origin: Origin::Static,
        },
    })
}

fn literal_str(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_python_file;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn parse_src(src: &str) -> ModModule {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(src.as_bytes()).unwrap();
        parse_python_file(f.path()).unwrap()
    }

    #[test]
    fn extracts_command_group_with_literal_args() {
        let src = r#"group = command_group("ci", "CI utilities")"#;
        let m = parse_src(src);
        let groups = extract_groups(&m, "");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].var, "group");
        assert_eq!(groups[0].group.name, "ci");
        assert_eq!(groups[0].group.title, "CI utilities");
    }

    #[test]
    fn resolves_docstring_keyword_to_module_doc() {
        let src = r#"group = command_group("ci", "CI utilities", docstring=__doc__)"#;
        let m = parse_src(src);
        let groups = extract_groups(&m, "module-level doc");
        assert_eq!(groups[0].group.description, "module-level doc");
    }

    #[test]
    fn ignores_non_command_group_assignments() {
        let src = "x = 1\ny = some_other_func(\"ci\")\n";
        let m = parse_src(src);
        assert!(extract_groups(&m, "").is_empty());
    }
}
