//! Extract function arguments from a `def` AST node.

use ruff_python_ast::{Expr, Parameters, StmtFunctionDef};

use crate::manifest::{Argument, ArgumentKind};

/// Build the argument list for a command from its function definition.
/// Skips the first parameter (assumed to be `ctx: Context`).
pub fn extract_arguments(func: &StmtFunctionDef) -> Vec<Argument> {
    let Parameters { args, kwonlyargs, .. } = func.parameters.as_ref();
    // Skip ctx (first positional).
    let positional: Vec<_> = args.iter().skip(1).collect();
    let mut out = Vec::new();
    for p in positional {
        let kind = if p.default.is_some() {
            ArgumentKind::Optional
        } else {
            ArgumentKind::Positional
        };
        out.push(Argument {
            name: p.parameter.name.to_string(),
            kind,
            help: String::new(),
            default: p.default.as_ref().map(|d| literal_default(d)),
            type_annotation: p
                .parameter
                .annotation
                .as_ref()
                .map(|a| annotation_to_string(a)),
            allowed_values: Vec::new(),
        });
    }
    for p in kwonlyargs {
        out.push(Argument {
            name: p.parameter.name.to_string(),
            kind: ArgumentKind::Flag,
            help: String::new(),
            default: p.default.as_ref().map(|d| literal_default(d)),
            type_annotation: p
                .parameter
                .annotation
                .as_ref()
                .map(|a| annotation_to_string(a)),
            allowed_values: Vec::new(),
        });
    }
    out
}

fn literal_default(expr: &Expr) -> String {
    match expr {
        Expr::StringLiteral(s) => format!("\"{}\"", s.value.to_str()),
        Expr::NumberLiteral(n) => format!("{:?}", n.value),
        Expr::BooleanLiteral(b) => b.value.to_string(),
        Expr::NoneLiteral(_) => "None".to_string(),
        _ => "<expr>".to_string(),
    }
}

fn annotation_to_string(expr: &Expr) -> String {
    // Best-effort textual rendering. Detailed resolution lands in
    // Tasks 12-14.
    match expr {
        Expr::Name(n) => n.id.as_str().to_string(),
        Expr::Attribute(a) => format!("{}.{}", annotation_to_string(&a.value), a.attr),
        Expr::Subscript(s) => format!(
            "{}[{}]",
            annotation_to_string(&s.value),
            annotation_to_string(&s.slice)
        ),
        _ => "<expr>".to_string(),
    }
}

#[cfg(test)]
mod tests {
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
        let args = extract_arguments(&func);
        assert_eq!(args.len(), 1);
        assert_eq!(args[0].name, "name");
    }

    #[test]
    fn marks_arguments_with_defaults_as_optional() {
        let func = first_func("def f(ctx, name=\"x\"): pass\n");
        let args = extract_arguments(&func);
        assert_eq!(args[0].kind, ArgumentKind::Optional);
        assert_eq!(args[0].default.as_deref(), Some("\"x\""));
    }

    #[test]
    fn captures_type_annotations_as_strings() {
        let func = first_func("def f(ctx, name: str = \"x\"): pass\n");
        let args = extract_arguments(&func);
        assert_eq!(args[0].type_annotation.as_deref(), Some("str"));
    }
}
