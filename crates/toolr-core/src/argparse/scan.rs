//! Walk a Python file's AST and extract `<x>.add_argument(...)` calls.

use ruff_python_ast::{Expr, ExprCall, ModModule, Stmt};
use ruff_python_parser as parser;
use thiserror::Error;

#[allow(unused_imports)] // Used by Task 11 (with_common_args helper).
use crate::argparse::config::CommonArg;
use crate::manifest::{ArgMetadata, Argument, ArgumentKind};

#[derive(Debug, Error)]
pub enum ScanError {
    #[error("failed to parse {path}: {message}")]
    Parse { path: String, message: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScannedCommand {
    pub name: String,        // filename stem
    pub summary: String,     // first paragraph of module docstring
    pub description: String, // rest of module docstring
    pub arguments: Vec<Argument>,
    pub warnings: Vec<String>,
}

/// argparse keyword arguments we recognise — the signature-shape filter
/// requires every kwarg present on a candidate call to be in this set.
/// Other names mean the call is some unrelated `.add_argument(...)` and
/// we silently skip it.
const ARGPARSE_KWARGS: &[&str] = &[
    "action", "nargs", "const", "default", "type", "choices", "required", "help", "metavar",
    "dest", "version",
];

/// Parse `source_text` (Python) and return a `ScannedCommand`.
/// `command_name` is what the caller wants to label this file's discovered
/// command as (typically the filename stem).
pub fn scan_source(command_name: &str, source_text: &str) -> Result<ScannedCommand, ScanError> {
    let parsed = parser::parse_module(source_text).map_err(|e| ScanError::Parse {
        path: command_name.to_string(),
        message: e.to_string(),
    })?;
    let module = parsed.into_syntax();
    let (summary, description) = split_docstring(&module_docstring(&module));

    let mut out = ScannedCommand {
        name: command_name.to_string(),
        summary,
        description,
        arguments: Vec::new(),
        warnings: Vec::new(),
    };

    for stmt in &module.body {
        visit_stmt(stmt, &mut out);
    }

    Ok(out)
}

/// Recursively walk statements looking for `Expr::Call` whose `.func` is
/// `<anything>.add_argument`. Function/class bodies are descended into.
fn visit_stmt(stmt: &Stmt, out: &mut ScannedCommand) {
    match stmt {
        Stmt::FunctionDef(f) => {
            for s in &f.body {
                visit_stmt(s, out);
            }
        }
        Stmt::ClassDef(c) => {
            for s in &c.body {
                visit_stmt(s, out);
            }
        }
        Stmt::If(i) => {
            for s in &i.body {
                visit_stmt(s, out);
            }
            for clause in &i.elif_else_clauses {
                for s in &clause.body {
                    visit_stmt(s, out);
                }
            }
        }
        Stmt::For(f) => {
            for s in &f.body {
                visit_stmt(s, out);
            }
            for s in &f.orelse {
                visit_stmt(s, out);
            }
        }
        Stmt::While(w) => {
            for s in &w.body {
                visit_stmt(s, out);
            }
            for s in &w.orelse {
                visit_stmt(s, out);
            }
        }
        Stmt::With(w) => {
            for s in &w.body {
                visit_stmt(s, out);
            }
        }
        Stmt::Try(t) => {
            for s in &t.body {
                visit_stmt(s, out);
            }
            for h in &t.handlers {
                let ruff_python_ast::ExceptHandler::ExceptHandler(h) = h;
                for s in &h.body {
                    visit_stmt(s, out);
                }
            }
            for s in &t.orelse {
                visit_stmt(s, out);
            }
            for s in &t.finalbody {
                visit_stmt(s, out);
            }
        }
        Stmt::Expr(e) => visit_expr(&e.value, out),
        _ => {}
    }
}

/// Walk an expression looking for `.add_argument(...)` calls. We only
/// need to inspect Calls — argparse usage is always a method call.
fn visit_expr(expr: &Expr, out: &mut ScannedCommand) {
    if let Expr::Call(call) = expr {
        if let Some(arg) = try_extract_argument(call, &mut out.warnings) {
            out.arguments.push(arg);
        }
    }
}

/// If `call` matches the argparse `add_argument` signature shape, build
/// the corresponding `Argument`. Returns `None` (silently) when the
/// shape doesn't match — this is how we tell argparse calls apart from
/// unrelated `.add_argument(...)` calls on other objects.
fn try_extract_argument(call: &ExprCall, warnings: &mut Vec<String>) -> Option<Argument> {
    // (1) Function must be `<anything>.add_argument`.
    let Expr::Attribute(attr) = call.func.as_ref() else {
        return None;
    };
    if attr.attr.as_str() != "add_argument" {
        return None;
    }

    // (2) Must have at least one positional, and every positional must
    //     be a string literal.
    let positionals = &call.arguments.args;
    if positionals.is_empty() {
        return None;
    }
    let mut pos_strs: Vec<String> = Vec::with_capacity(positionals.len());
    for p in positionals.iter() {
        let Expr::StringLiteral(s) = p else {
            return None;
        };
        pos_strs.push(s.value.to_str().to_string());
    }

    // (3) Every kwarg name must be in the argparse-known set.
    for kw in &call.arguments.keywords {
        let Some(name) = kw.arg.as_ref().map(|n| n.as_str()) else {
            // `**kwargs` splat — unknown name, treat as non-argparse.
            return None;
        };
        if !ARGPARSE_KWARGS.contains(&name) {
            return None;
        }
    }

    // Shape check passed — classify and extract.
    Some(build_argument(&pos_strs, call, warnings))
}

/// Build the `Argument` from a confirmed argparse `add_argument` call.
fn build_argument(positionals: &[String], call: &ExprCall, warnings: &mut Vec<String>) -> Argument {
    // Pick the canonical name: for keyword-style args, prefer the
    // longest `--foo`; for positional, the sole positional string.
    let is_keyword_style = positionals.iter().any(|s| s.starts_with('-'));
    let (name, name_for_warning) = if is_keyword_style {
        let longest = positionals
            .iter()
            .filter(|s| s.starts_with("--"))
            .max_by_key(|s| s.len())
            .or_else(|| positionals.iter().find(|s| s.starts_with('-')))
            .expect("we know at least one positional starts with '-'");
        let stripped = longest.trim_start_matches('-').to_string();
        (stripped, longest.clone())
    } else {
        let n = positionals[0].clone();
        (n.clone(), n)
    };

    // Collect kwargs by name (string-encoded best-effort).
    let mut action: Option<String> = None;
    let mut default: Option<String> = None;
    let mut help_text = String::new();
    let mut choices: Vec<String> = Vec::new();
    let mut type_annotation: Option<String> = None;
    let mut metavar: Option<String> = None;
    for kw in &call.arguments.keywords {
        let Some(kw_name) = kw.arg.as_ref().map(|n| n.as_str()) else {
            continue;
        };
        match kw_name {
            "action" => action = literal_str(&kw.value),
            "default" => default = literal_to_string(&kw.value),
            "help" => {
                if let Some(s) = literal_str(&kw.value) {
                    help_text = s;
                }
            }
            "choices" => {
                if let Some(list) = literal_str_list(&kw.value) {
                    choices = list;
                }
            }
            "type" => {
                let resolved = resolve_type_kwarg(&kw.value);
                match resolved {
                    Some(t) => type_annotation = Some(t),
                    None => {
                        let raw = type_kwarg_repr(&kw.value);
                        warnings.push(format!(
                            "argparse: {name_for_warning}: unresolvable type={raw}"
                        ));
                    }
                }
            }
            "metavar" => metavar = literal_str(&kw.value),
            _ => {}
        }
    }

    let kind = classify_kind(is_keyword_style, action.as_deref());

    let mut metadata = ArgMetadata::default();
    if let Some(mv) = metavar {
        metadata.metavar = Some(mv);
    }

    Argument {
        name,
        kind,
        help: help_text,
        default,
        type_annotation,
        resolved_type: None,
        allowed_values: choices,
        path_constraints: None,
        metadata,
    }
}

fn classify_kind(is_keyword_style: bool, action: Option<&str>) -> ArgumentKind {
    if !is_keyword_style {
        return ArgumentKind::Positional;
    }
    match action {
        Some("store_true") | Some("store_false") => ArgumentKind::Flag,
        Some("append") => ArgumentKind::Repeated,
        _ => ArgumentKind::Optional,
    }
}

/// Returns the type name if `expr` is one of the literal type-builtin
/// names that argparse recognises and we know how to render.
fn resolve_type_kwarg(expr: &Expr) -> Option<String> {
    let Expr::Name(n) = expr else { return None };
    let id = n.id.as_str();
    matches!(id, "int" | "float" | "str" | "bool").then(|| id.to_string())
}

/// Best-effort textual rendering of an unresolved `type=...` value for
/// the warning message.
fn type_kwarg_repr(expr: &Expr) -> String {
    match expr {
        Expr::Name(n) => n.id.as_str().to_string(),
        Expr::Attribute(a) => {
            let prefix = type_kwarg_repr(&a.value);
            format!("{prefix}.{}", a.attr)
        }
        _ => "<expr>".to_string(),
    }
}

fn literal_str(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
        _ => None,
    }
}

fn literal_str_list(expr: &Expr) -> Option<Vec<String>> {
    let elts = match expr {
        Expr::List(l) => &l.elts,
        Expr::Tuple(t) => &t.elts,
        _ => return None,
    };
    let mut out = Vec::with_capacity(elts.len());
    for e in elts {
        out.push(literal_str(e)?);
    }
    Some(out)
}

/// Render a literal default value as a string. Booleans/ints/floats/strs
/// all map cleanly; non-literal expressions yield `None` (the caller
/// treats this as "no default").
fn literal_to_string(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
        Expr::NumberLiteral(n) => match &n.value {
            ruff_python_ast::Number::Int(i) => Some(i.to_string()),
            ruff_python_ast::Number::Float(f) => Some(f.to_string()),
            ruff_python_ast::Number::Complex { real, imag } => Some(format!("({real}+{imag}j)")),
        },
        Expr::BooleanLiteral(b) => Some(if b.value { "true" } else { "false" }.to_string()),
        Expr::NoneLiteral(_) => None,
        _ => None,
    }
}

/// Grab the module's leading string-literal as the raw docstring.
fn module_docstring(module: &ModModule) -> String {
    let Some(Stmt::Expr(e)) = module.body.first() else {
        return String::new();
    };
    let Expr::StringLiteral(s) = e.value.as_ref() else {
        return String::new();
    };
    s.value.to_str().to_string()
}

/// Split a docstring into (summary, description). The summary is the
/// first paragraph (everything before the first blank line); the
/// description is everything after it, with leading blank lines
/// stripped.
fn split_docstring(raw: &str) -> (String, String) {
    let trimmed = raw.trim_start_matches('\n');
    let mut summary = String::new();
    let mut rest_start = None;
    for (idx, line) in trimmed.lines().enumerate() {
        if line.trim().is_empty() {
            rest_start = Some(idx + 1);
            break;
        }
        if !summary.is_empty() {
            summary.push(' ');
        }
        summary.push_str(line.trim());
    }
    let description = match rest_start {
        Some(start) => trimmed
            .lines()
            .skip(start)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string(),
        None => String::new(),
    };
    (summary, description)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_positional_optional_flag_and_repeated() {
        let source = r#"
"""Migrate the database.

Handles schema migrations and rolling back.
"""
def add_arguments(self, parser):
    parser.add_argument('app_label')
    parser.add_argument('--database', default='default', help='Target DB')
    parser.add_argument('--check', action='store_true', help='Dry run')
    parser.add_argument('--exclude', action='append')
"#;
        let scanned = scan_source("migrate", source).unwrap();
        assert_eq!(scanned.name, "migrate");
        assert_eq!(scanned.summary, "Migrate the database.");
        assert!(scanned.description.contains("Handles schema migrations"));

        let names: Vec<_> = scanned.arguments.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["app_label", "database", "check", "exclude"]);

        assert_eq!(scanned.arguments[0].kind, ArgumentKind::Positional);
        assert_eq!(scanned.arguments[1].kind, ArgumentKind::Optional);
        assert_eq!(scanned.arguments[1].default.as_deref(), Some("default"));
        assert_eq!(scanned.arguments[2].kind, ArgumentKind::Flag);
        assert_eq!(scanned.arguments[3].kind, ArgumentKind::Repeated);
    }

    #[test]
    fn empty_file_yields_command_with_no_args() {
        let scanned = scan_source("empty", "").unwrap();
        assert!(scanned.arguments.is_empty());
        assert_eq!(scanned.name, "empty");
    }

    #[test]
    fn unresolvable_type_emits_warning_and_no_type_annotation() {
        let source = r#"
def add_arguments(self, parser):
    parser.add_argument('--count', type=parse_count)
"#;
        let scanned = scan_source("x", source).unwrap();
        assert_eq!(scanned.arguments.len(), 1);
        assert_eq!(scanned.arguments[0].type_annotation, None);
        assert!(scanned.warnings.iter().any(|w| w.contains("type=")));
    }

    #[test]
    fn matches_any_receiver_name() {
        // Django-style (`parser`), nested attribute (`self.parser`), and
        // a custom name should all be picked up — receiver is not checked.
        let source = r#"
def add_arguments(self, parser):
    parser.add_argument('a')
    self.parser.add_argument('--b', action='store_true')
    sub_parser.add_argument('c')
"#;
        let scanned = scan_source("x", source).unwrap();
        let names: Vec<_> = scanned.arguments.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn skips_calls_whose_positional_is_not_a_string_literal() {
        // First positional is an int — not an argparse name/flag, skip.
        let source = "def f(): mylist.add_argument(42, default='x')";
        let scanned = scan_source("x", source).unwrap();
        assert!(scanned.arguments.is_empty());
        assert!(scanned.warnings.is_empty(), "skipping non-argparse calls should not warn");
    }

    #[test]
    fn skips_calls_with_unknown_kwarg() {
        // `frobnicate` is not in the argparse kwarg set; this is some other
        // method named `add_argument`. Skip silently.
        let source = "def f(): widget.add_argument('--x', frobnicate=True)";
        let scanned = scan_source("x", source).unwrap();
        assert!(scanned.arguments.is_empty());
        assert!(scanned.warnings.is_empty());
    }

    #[test]
    fn skips_calls_with_no_positional_arguments() {
        let source = "def f(): widget.add_argument(action='store_true')";
        let scanned = scan_source("x", source).unwrap();
        assert!(scanned.arguments.is_empty());
        assert!(scanned.warnings.is_empty());
    }
}
