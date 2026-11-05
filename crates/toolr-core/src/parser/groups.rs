//! Extract `group = command_group(...)` assignments from a module AST.

use std::collections::HashMap;

use ruff_python_ast::{Expr, ExprCall, ModModule, Stmt, StmtAssign};

use super::parse_docstring;
use crate::manifest::{Group, Origin};

/// A group binding extracted from source. The `var` is the Python local name
/// that subsequent `@var.command` decorators reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupBinding {
    pub var: String,
    /// The group, with `title` and `description` holding the readable text of
    /// `fields`.
    pub group: Group,
    /// The group's fields as this declaration gives them.
    pub fields: GroupFields,
    /// Whether the call passes a non-empty description or docstring, readable
    /// or not. A group no declaration describes fails the build.
    pub describes: bool,
    /// Whether the call passes a docstring together with a description or
    /// long description, which Python's `command_group` refuses.
    pub contradictory: bool,
    /// Whether the call may pass a description or docstring the static parser
    /// can't see: through `*`/`**` unpacking, or as the `__doc__` of a module
    /// without a docstring.
    pub unreadable: bool,
}

/// One `command_group` field as the static parser reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Field {
    /// Not passed, `None`, or empty: no opinion.
    #[default]
    Empty,
    /// A string literal, or a string constant assigned earlier in the module.
    Text(String),
    /// Passed as something the static parser can't read, such as a call or an
    /// imported name. It has an opinion, but not one the build can compare.
    Opaque,
}

impl Field {
    fn from_text(text: String) -> Self {
        if text.is_empty() {
            Field::Empty
        } else {
            Field::Text(text)
        }
    }

    /// The readable text, or `""`.
    pub fn text(&self) -> &str {
        match self {
            Field::Text(text) => text,
            _ => "",
        }
    }
}

/// The fields one declaration gives a group, in the order Python's
/// `command_group` compares them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupFields {
    pub title: Field,
    pub description: Field,
    pub long_description: Field,
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
    // `NAME = "text"` assignments so far: a call reads the value a name holds
    // when it runs.
    let mut consts: HashMap<String, String> = HashMap::new();
    for stmt in &module.body {
        let source = Source {
            module_docstring,
            consts: &consts,
        };
        if let Some(binding) = group_binding(stmt, &source, &out, global_vars) {
            out.push(binding);
        }
        if let Stmt::Assign(StmtAssign { targets, value, .. }) = stmt
            && let Some(name) = single_name_target(targets)
        {
            match literal_str(value) {
                Some(text) => consts.insert(name, text),
                None => consts.remove(&name),
            };
        }
    }
    out
}

/// What a `command_group` call's arguments can refer to.
struct Source<'a> {
    module_docstring: &'a str,
    consts: &'a HashMap<String, String>,
}

impl Source<'_> {
    /// Read one argument. A `__doc__` reference is the module docstring, and
    /// one that is empty or whitespace-only has no opinion.
    fn read(&self, expr: Option<&Expr>) -> Field {
        match expr {
            None | Some(Expr::NoneLiteral(_)) => Field::Empty,
            Some(Expr::Name(n)) if n.id.as_str() == "__doc__" => {
                if self.module_docstring.trim().is_empty() {
                    Field::Empty
                } else {
                    Field::Text(self.module_docstring.to_string())
                }
            }
            Some(Expr::Name(n)) => self
                .consts
                .get(n.id.as_str())
                .map_or(Field::Opaque, |text| Field::from_text(text.clone())),
            Some(e) => literal_str(e).map_or(Field::Opaque, Field::from_text),
        }
    }
}

/// The binding `stmt` makes, when it is a `command_group(...)` call.
fn group_binding(
    stmt: &Stmt,
    source: &Source<'_>,
    local: &[GroupBinding],
    global_vars: &HashMap<String, String>,
) -> Option<GroupBinding> {
    // Two recognised shapes:
    //   1. `var = command_group(...)` — group-binding style.
    //   2. `command_group("path", ...)` — bare expression-statement,
    //      the string-path style. No binding var to capture
    //      (the leaf name becomes a synthetic var so `@var.command`
    //      keeps working when a user mixes styles).
    let (var_name, call) = match stmt {
        Stmt::Assign(StmtAssign { targets, value, .. }) => {
            let name = single_name_target(targets)?;
            let Expr::Call(call) = value.as_ref() else {
                return None;
            };
            (Some(name), call)
        }
        Stmt::Expr(expr_stmt) => {
            let Expr::Call(call) = expr_stmt.value.as_ref() else {
                return None;
            };
            (None, call)
        }
        _ => return None,
    };
    if !is_command_group_call(call) {
        return None;
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
        .and_then(|pv| resolve_var(&pv, local, global_vars))
        .or_else(|| parent_kwarg_literal(call));
    let chosen_var = var_name.clone().unwrap_or_default();
    let mut binding = parse_group_call(call, &chosen_var, source, parent_path)?;
    // For bare-form (no explicit var), use the leaf name as the
    // synthetic var so `@<leaf>.command` decorators still
    // resolve when authors mix the two styles. Skip when
    // there's a real binding name.
    if var_name.is_none() {
        binding.var = binding.group.name.clone();
    }
    Some(binding)
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

/// If the call passes `parent="..."` (literal string), by keyword or sixth
/// position, return that string.
fn parent_kwarg_literal(call: &ExprCall) -> Option<String> {
    argument(call, "parent", 5).and_then(literal_str)
}

/// If the call passes `parent=<var>` (variable reference), by keyword or
/// sixth position, return the variable name so the caller can resolve it
/// against the local + global binding tables.
fn parent_kwarg_var(call: &ExprCall) -> Option<String> {
    match argument(call, "parent", 5)? {
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
    source: &Source<'_>,
    parent: Option<String>,
) -> Option<GroupBinding> {
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
    // `None` is no argument, and `__doc__` is `None` in a module without a
    // docstring, as at runtime.
    let passed = |keyword: &str, position: usize| {
        argument(call, keyword, position).is_some_and(|e| match e {
            Expr::NoneLiteral(_) => false,
            Expr::Name(n) if n.id.as_str() == "__doc__" => !source.module_docstring.is_empty(),
            _ => true,
        })
    };
    let passes_docstring = passed("docstring", 4);
    let docstring = source.read(argument(call, "docstring", 4));
    let explicit_description = source.read(argument(call, "description", 2));
    // When `docstring=` is supplied, split it the same way `parser/commands.rs`
    // splits function docstrings: the first paragraph becomes the short
    // `title` (clap's `about`, shown in the parent's command listing) and
    // the full rendered docstring (short + long + Examples/Notes/…) becomes
    // the long `description` (clap's `long_about`, shown on `--help`).
    // An explicit title or description still wins, so callers can override
    // one side without losing the other.
    let parsed_doc = match &docstring {
        Field::Text(raw) => parse_docstring(raw),
        _ => None,
    };
    let from_docstring = |part: fn(&crate::Docstring) -> String| match docstring {
        Field::Opaque => Field::Opaque,
        _ => parsed_doc
            .as_ref()
            .map_or(Field::Empty, |d| Field::from_text(part(d))),
    };
    let title = match source.read(argument(call, "title", 1)) {
        Field::Empty => from_docstring(|d| d.short_description.clone()),
        title => title,
    };
    // An empty or whitespace-only docstring has no opinion: it leaves the
    // group undescribed.
    let describes = explicit_description != Field::Empty
        || match &docstring {
            Field::Text(raw) => !raw.trim().is_empty(),
            Field::Opaque => true,
            Field::Empty => false,
        };
    let description = match explicit_description {
        Field::Empty => from_docstring(crate::Docstring::full_description),
        description => description,
    };
    // `command_group` drops the long description when a docstring is given.
    let long_description = if passes_docstring {
        Field::Empty
    } else {
        source.read(argument(call, "long_description", 3))
    };
    let blank_doc = |keyword: &str, position: usize| {
        matches!(argument(call, keyword, position), Some(Expr::Name(n)) if n.id.as_str() == "__doc__")
            && source.module_docstring.trim().is_empty()
    };
    let unpacks = call.arguments.args.iter().any(Expr::is_starred_expr)
        || call.arguments.keywords.iter().any(|k| k.arg.is_none());
    Some(GroupBinding {
        var: var.to_string(),
        describes,
        contradictory: passes_docstring
            && (passed("description", 2) || passed("long_description", 3)),
        unreadable: unpacks || blank_doc("description", 2) || blank_doc("docstring", 4),
        group: Group {
            name,
            title: title.text().to_string(),
            description: description.text().to_string(),
            parent,
            origin: Origin::Static,
        },
        fields: GroupFields {
            title,
            description,
            long_description,
        },
    })
}

/// The argument `command_group` binds to `keyword`, passed by keyword or at
/// `position`.
fn argument<'a>(call: &'a ExprCall, keyword: &str, position: usize) -> Option<&'a Expr> {
    call.arguments
        .keywords
        .iter()
        .find(|k| k.arg.as_ref().map(|n| n.as_str()) == Some(keyword))
        .map(|k| &k.value)
        .or_else(|| call.arguments.args.get(position))
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
    fn explicit_title_keeps_docstring_full_description() {
        // When the caller passes both a positional title and a docstring,
        // the title wins for the short slot but the docstring's rendered
        // ``full_description`` (short + long + sections) still populates
        // `description` (clap's `long_about`).
        let src = r#"group = command_group("ci", "CI utilities", docstring=__doc__)"#;
        let m = parse_src(src);
        let groups = extract_groups(
            &m,
            "First paragraph short.\n\nLong paragraph body.",
            &HashMap::new(),
        );
        assert_eq!(groups[0].group.title, "CI utilities");
        assert_eq!(
            groups[0].group.description,
            "First paragraph short.\n\nLong paragraph body."
        );
    }

    #[test]
    fn docstring_first_paragraph_becomes_title() {
        // Single-line docstring: short_description fills `title`,
        // `description` ends up the same single paragraph (no long
        // body, no sections to append).
        let src = r#"group = command_group("widget", docstring=__doc__)"#;
        let m = parse_src(src);
        let groups = extract_groups(
            &m,
            "Example single-line docstring for a group.",
            &HashMap::new(),
        );
        assert_eq!(
            groups[0].group.title,
            "Example single-line docstring for a group."
        );
        assert_eq!(
            groups[0].group.description,
            "Example single-line docstring for a group."
        );
    }

    #[test]
    fn docstring_long_paragraph_becomes_description() {
        // Two-paragraph docstring: first paragraph → title.
        // `description` is the full render (short + long), so
        // `--help` re-states the blurb plus the long body.
        let src = r#"group = command_group("ci", docstring=__doc__)"#;
        let m = parse_src(src);
        let groups = extract_groups(
            &m,
            "Short blurb for parent listing.\n\nLonger prose shown by --help only.",
            &HashMap::new(),
        );
        assert_eq!(groups[0].group.title, "Short blurb for parent listing.");
        assert_eq!(
            groups[0].group.description,
            "Short blurb for parent listing.\n\nLonger prose shown by --help only."
        );
    }

    #[test]
    fn description_kwarg_with_docstring_is_contradictory() {
        // Python's `command_group` refuses both; the build reports it.
        let src = r#"group = command_group("ci", description="kw-desc", docstring=__doc__)"#;
        let m = parse_src(src);
        let groups = extract_groups(&m, "Short title.\n\nLong paragraph.", &HashMap::new());
        assert!(groups[0].contradictory);
        assert!(groups[0].describes);
    }

    #[test]
    fn title_only_call_does_not_describe() {
        let m = parse_src(r#"group = command_group("ci", "CI")"#);
        let groups = extract_groups(&m, "", &HashMap::new());
        assert!(!groups[0].describes);
        assert!(!groups[0].contradictory);
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
