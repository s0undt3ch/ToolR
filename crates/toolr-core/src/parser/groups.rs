//! Extract `group = command_group(...)` assignments from a module AST.

use std::collections::HashMap;

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
///
/// Nested groups (`child = parent.command_group("name", ...)`) get their
/// `parent` field set to the parent binding's `full_path()`, so the
/// CLI builder can reconstruct the hierarchy. Source order matters:
/// parents must be assigned before children (which Python already
/// requires).
pub fn extract_groups(
    module: &ModModule,
    module_docstring: &str,
    global_vars: &HashMap<String, String>,
) -> Vec<GroupBinding> {
    let mut out: Vec<GroupBinding> = Vec::new();
    for stmt in &module.body {
        // Two recognised shapes:
        //   1. `var = command_group(...)` — legacy binding style.
        //   2. `command_group("path", ...)` — bare expression-statement,
        //      the modern string-path style. No binding var to capture
        //      (the leaf name becomes a synthetic var so `@var.command`
        //      keeps working when a user mixes styles).
        let (var_name, call) = match stmt {
            Stmt::Assign(StmtAssign { targets, value, .. }) => {
                let Some(name) = single_name_target(targets) else {
                    continue;
                };
                let Expr::Call(call) = value.as_ref() else {
                    continue;
                };
                (Some(name), call)
            }
            Stmt::Expr(expr_stmt) => {
                let Expr::Call(call) = expr_stmt.value.as_ref() else {
                    continue;
                };
                (None, call)
            }
            _ => continue,
        };
        if !is_command_group_call(call) {
            continue;
        }
        // Parent path can come from three source patterns:
        //   1. Method-call style: `child = parent.command_group(...)` —
        //      look up `parent` locally first, then in the global
        //      cross-file binding map.
        //   2. Keyword-arg with variable reference:
        //      `child = command_group(..., parent=parent_var)`.
        //   3. Keyword-arg with literal string:
        //      `child = command_group(..., parent="ci")`.
        //
        // Plus the dotted-name shape — `command_group("ci.helm-diff-pr-comment", ...)`
        // — splits the leaf off inside `parse_group_call` and overrides
        // whatever parent_path we resolve here.
        let parent_path = parent_var_name(call)
            .or_else(|| parent_kwarg_var(call))
            .and_then(|pv| resolve_var(&pv, &out, global_vars))
            .or_else(|| parent_kwarg_literal(call));
        let chosen_var = var_name.clone().unwrap_or_default();
        let Some(mut binding) =
            parse_group_call(call, &chosen_var, module_docstring, parent_path)
        else {
            continue;
        };
        // For bare-form (no explicit var), use the leaf name as the
        // synthetic var so legacy `@<leaf>.command` decorators still
        // resolve when authors mix the new and old styles. Skip when
        // there's a real binding name.
        if var_name.is_none() {
            binding.var = binding.group.name.clone();
        }
        out.push(binding);
    }
    out
}

/// Resolve a variable name to its group's full_path. Checks
/// already-extracted local bindings first, then the cumulative global
/// map of bindings seen across previously-processed files.
fn resolve_var(
    var: &str,
    local: &[GroupBinding],
    global_vars: &HashMap<String, String>,
) -> Option<String> {
    local
        .iter()
        .find(|b| b.var == var)
        .map(|b| b.group.full_path())
        .or_else(|| global_vars.get(var).cloned())
}

/// If the call is a method invocation like `docker.command_group(...)`,
/// return the head variable name (`"docker"`). For free-function
/// `command_group(...)` returns `None`.
fn parent_var_name(call: &ExprCall) -> Option<String> {
    let Expr::Attribute(attr) = call.func.as_ref() else {
        return None;
    };
    match attr.value.as_ref() {
        Expr::Name(n) => Some(n.id.as_str().to_string()),
        _ => None,
    }
}

/// If the call passes `parent="..."` (literal string) as a keyword
/// argument, return that string.
fn parent_kwarg_literal(call: &ExprCall) -> Option<String> {
    call.arguments
        .keywords
        .iter()
        .find(|k| k.arg.as_ref().map(|n| n.as_str()) == Some("parent"))
        .and_then(|k| literal_str(&k.value))
}

/// If the call passes `parent=<var>` (variable reference) as a keyword
/// argument, return the variable name so the caller can resolve it
/// against the local + global binding tables.
fn parent_kwarg_var(call: &ExprCall) -> Option<String> {
    let kw = call
        .arguments
        .keywords
        .iter()
        .find(|k| k.arg.as_ref().map(|n| n.as_str()) == Some("parent"))?;
    match &kw.value {
        Expr::Name(n) => Some(n.id.as_str().to_string()),
        _ => None,
    }
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

fn parse_group_call(
    call: &ExprCall,
    var: &str,
    module_docstring: &str,
    parent: Option<String>,
) -> Option<GroupBinding> {
    // Positional args: name, title. Keyword `docstring` may be __doc__.
    let raw_name = call.arguments.args.first().and_then(literal_str)?;
    // Dotted name form (`command_group("ci.helm-diff-pr-comment", ...)`):
    // split the leaf off the parent path. Explicit `parent=` (from the
    // earlier resolution path) takes a back seat — pick one style.
    let (name, parent) = if let Some(idx) = raw_name.rfind('.') {
        let dotted_parent = raw_name[..idx].to_string();
        let leaf = raw_name[idx + 1..].to_string();
        (leaf, Some(dotted_parent))
    } else {
        (raw_name, parent)
    };
    let title = call
        .arguments
        .args
        .get(1)
        .and_then(literal_str)
        .unwrap_or_default();
    // Description precedence (matches the Python decorator's resolution
    // order in `toolr._decorators.command_group`):
    //   1. `description=` kwarg (explicit, wins).
    //   2. Third positional argument.
    //   3. `docstring=` kwarg (may resolve to module __doc__).
    //   4. Empty string.
    let description = description_kwarg(call)
        .or_else(|| call.arguments.args.get(2).and_then(literal_str))
        .or_else(|| docstring_kwarg(call, module_docstring))
        .unwrap_or_default();
    Some(GroupBinding {
        var: var.to_string(),
        group: Group {
            name,
            title,
            description,
            parent,
            origin: Origin::Static,
        },
    })
}

/// Extract the `description=` kwarg's string-literal value, if present.
fn description_kwarg(call: &ExprCall) -> Option<String> {
    call.arguments
        .keywords
        .iter()
        .find(|k| k.arg.as_ref().map(|n| n.as_str()) == Some("description"))
        .and_then(|k| literal_str(&k.value))
}

/// Extract the `docstring=` kwarg's value. A bare `__doc__` reference
/// resolves to the supplied module docstring; otherwise it must be a
/// string literal.
fn docstring_kwarg(call: &ExprCall, module_docstring: &str) -> Option<String> {
    call.arguments
        .keywords
        .iter()
        .find(|k| k.arg.as_ref().map(|n| n.as_str()) == Some("docstring"))
        .and_then(|k| match &k.value {
            Expr::Name(n) if n.id.as_str() == "__doc__" => Some(module_docstring.to_string()),
            e => literal_str(e),
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
        let groups = extract_groups(&m, "", &HashMap::new());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].var, "group");
        assert_eq!(groups[0].group.name, "ci");
        assert_eq!(groups[0].group.title, "CI utilities");
    }

    #[test]
    fn resolves_docstring_keyword_to_module_doc() {
        let src = r#"group = command_group("ci", "CI utilities", docstring=__doc__)"#;
        let m = parse_src(src);
        let groups = extract_groups(&m, "module-level doc", &HashMap::new());
        assert_eq!(groups[0].group.description, "module-level doc");
    }

    #[test]
    fn parses_description_from_third_positional() {
        let src = r#"group = command_group("n", "t", "desc body")"#;
        let m = parse_src(src);
        let groups = extract_groups(&m, "", &HashMap::new());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].group.description, "desc body");
    }

    #[test]
    fn description_kwarg_wins_over_positional() {
        let src = r#"group = command_group("n", "t", "pos", description="kw")"#;
        let m = parse_src(src);
        let groups = extract_groups(&m, "", &HashMap::new());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].group.description, "kw");
    }

    #[test]
    fn ignores_non_command_group_assignments() {
        let src = "x = 1\ny = some_other_func(\"ci\")\n";
        let m = parse_src(src);
        assert!(extract_groups(&m, "", &HashMap::new()).is_empty());
    }

    #[test]
    fn nested_group_records_parent_full_path() {
        let src = r#"docker = command_group("docker", "Docker")
image = docker.command_group("image", "Image")
"#;
        let m = parse_src(src);
        let groups = extract_groups(&m, "", &HashMap::new());
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].group.name, "docker");
        assert_eq!(groups[0].group.parent, None);
        assert_eq!(groups[0].group.full_path(), "docker");
        assert_eq!(groups[1].group.name, "image");
        assert_eq!(groups[1].group.parent.as_deref(), Some("docker"));
        assert_eq!(groups[1].group.full_path(), "docker.image");
    }

    #[test]
    fn explicit_parent_kwarg_nests_under_named_group() {
        // Matches the `command_group("...", parent="ci")` signature
        // exposed by `toolr._registry.command_group`. The parent
        // doesn't have to be declared in the same file — we trust the
        // string as the dotted full_path.
        let src = r#"
helm_diff = command_group("helm-diff-pr-comment", "Helm diff", parent="ci")
"#;
        let m = parse_src(src);
        let groups = extract_groups(&m, "", &HashMap::new());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].group.name, "helm-diff-pr-comment");
        assert_eq!(groups[0].group.parent.as_deref(), Some("ci"));
        assert_eq!(groups[0].group.full_path(), "ci.helm-diff-pr-comment");
    }

    #[test]
    fn two_level_nesting_concatenates_full_path() {
        let src = r#"a = command_group("a", "A")
b = a.command_group("b", "B")
c = b.command_group("c", "C")
"#;
        let m = parse_src(src);
        let groups = extract_groups(&m, "", &HashMap::new());
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[2].group.full_path(), "a.b.c");
        assert_eq!(groups[2].group.parent.as_deref(), Some("a.b"));
    }
}
