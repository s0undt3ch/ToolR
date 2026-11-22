//! Walk a Python file's AST and extract `<x>.add_argument(...)` calls.

use ruff_python_ast::{Expr, ExprCall, ModModule, Stmt};
use ruff_python_parser as parser;
use thiserror::Error;

use crate::argparse::config::CommonArg;
use crate::manifest::{ArgMetadata, Argument, ArgumentKind, Nargs};

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
    /// True when the module defines Django's `Command` entry point — either `class Command(...)`
    /// directly, or `Command = <Name>` aliased to a class defined earlier in the same module. Lets
    /// `scan_block_paths` keep zero-argument management commands (e.g. a `BaseCommand` subclass
    /// with no `add_arguments` at all) instead of mistaking them for stray helper modules.
    pub is_django_command_class: bool,
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
pub(crate) fn scan_source(
    command_name: &str,
    source_text: &str,
) -> Result<ScannedCommand, ScanError> {
    let parsed = parser::parse_module(source_text).map_err(|e| ScanError::Parse {
        path: command_name.to_string(),
        message: e.to_string(),
    })?;
    let module = parsed.into_syntax();
    let (summary, description) = split_docstring(&module_docstring(&module));
    let is_django_command_class = defines_django_command_class(&module);

    let mut out = ScannedCommand {
        name: command_name.to_string(),
        summary,
        description,
        arguments: Vec::new(),
        warnings: Vec::new(),
        is_django_command_class,
    };

    for stmt in &module.body {
        visit_stmt(stmt, &mut out);
    }

    Ok(out)
}

/// True when the module's top level defines Django's `Command` entry point: a literal `class
/// Command(...)`, or `Command = <Name>` where `<Name>` is a class defined at module top level.
/// Only same-module classes are resolved — an alias to an imported name (`Command =
/// SomeImportedClass`) isn't verifiable without import resolution, so it's treated as unresolved
/// rather than guessed at.
fn defines_django_command_class(module: &ModModule) -> bool {
    let mut top_level_classes: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for stmt in &module.body {
        if let Stmt::ClassDef(class_def) = stmt {
            top_level_classes.insert(class_def.name.as_str());
        }
    }
    if top_level_classes.contains("Command") {
        return true;
    }
    for stmt in &module.body {
        let Stmt::Assign(assign) = stmt else {
            continue;
        };
        if assign.targets.len() != 1 {
            continue;
        }
        let Expr::Name(target) = &assign.targets[0] else {
            continue;
        };
        if target.id.as_str() != "Command" {
            continue;
        }
        if let Expr::Name(value) = assign.value.as_ref()
            && top_level_classes.contains(value.id.as_str())
        {
            return true;
        }
    }
    false
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
    if let Expr::Call(call) = expr
        && let Some(arg) = try_extract_argument(call, &mut out.warnings)
    {
        out.arguments.push(arg);
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
    let mut nargs: Option<String> = None;
    let mut nargs_repr: Option<String> = None;
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
            "nargs" => {
                nargs = nargs_value(&kw.value);
                nargs_repr = nargs_kwarg_repr(&kw.value);
            }
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

    let kind = classify_kind(is_keyword_style, action.as_deref(), nargs.as_deref());

    // `nargs=N` combined with `action="append"` (repeating groups of N
    // values) isn't representable by any `ArgumentKind` — degrades to a
    // single value silently. `argparse.REMAINDER` on a keyword-style arg
    // is nonsensical in argparse itself and stays unsupported too. Warn
    // so these are visible rather than silently wrong at runtime.
    if let Some(raw) = &nargs_repr {
        let is_fixed_arity = nargs.as_deref().is_some_and(|n| n.parse::<usize>().is_ok());
        let fixed_arity_with_append = is_fixed_arity && action.as_deref() == Some("append");
        let handled = !fixed_arity_with_append
            && (matches!(nargs.as_deref(), Some("+") | Some("*") | Some("?"))
                || (!is_keyword_style && nargs.as_deref() == Some("REMAINDER"))
                || is_fixed_arity);
        if !handled {
            warnings.push(format!(
                "argparse: {name_for_warning}: unsupported nargs={raw} (treated as a single value; may not match argparse's runtime behaviour; see https://github.com/s0undt3ch/ToolR/issues/423)"
            ));
        }
    }

    let mut metadata = ArgMetadata::default();
    if let Some(mv) = metavar {
        metadata.metavar = Some(mv);
    }
    metadata.nargs = match kind {
        ArgumentKind::Repeated if is_keyword_style => match nargs.as_deref() {
            Some("+") => Some(Nargs::Plus),
            Some("*") => Some(Nargs::Star),
            // action="append": one value per occurrence, no extra shape.
            _ => None,
        },
        ArgumentKind::VarPositional => match nargs.as_deref() {
            Some("+") => Some(Nargs::Plus),
            // "*" and REMAINDER are both zero-or-more.
            _ => Some(Nargs::Star),
        },
        ArgumentKind::FixedArity => nargs
            .as_deref()
            .and_then(|n| n.parse::<usize>().ok())
            .map(Nargs::Fixed),
        _ => None,
    };

    // When the source spells its long flag with underscores
    // (`--skip_warm_cache`), cli.rs still normalises the canonical CLI
    // form to dashes for help and shell completion
    // (`--skip-warm-cache`). Register the underscored spelling as a
    // hidden alias so users who type the upstream tool's exact flag
    // still get a match.
    if is_keyword_style && name.contains('_') {
        metadata.aliases.push(format!("--{name}"));
    }

    // argparse commonly declares a short flag alongside the long one
    // (`add_argument("-s", "--run-synchronously", ...)`). Only the
    // longest `--flag` became the canonical `name` above — every other
    // flag-shaped positional (short flags, additional long spellings)
    // must still be registered so clap accepts it too.
    if is_keyword_style {
        for p in positionals {
            if p != &name_for_warning {
                metadata.aliases.push(p.clone());
            }
        }
    }

    // Preserve the literal long-flag spelling from the source. The
    // scanner's canonical `name` strips the leading `--` and may pick
    // between aliases (`add_argument("--user_ids", "--user-ids", ...)`
    // ties on length and last-wins), so `name` alone isn't enough to
    // round-trip the source's exact spelling. `long_flag` carries it
    // verbatim for the dispatch path to emit unchanged.
    let long_flag = if is_keyword_style {
        Some(name_for_warning.clone())
    } else {
        None
    };

    Argument {
        name,
        kind,
        help: help_text,
        default,
        type_annotation,
        resolved_type: None,
        allowed_values: choices,
        metadata,
        long_flag,
    }
}

fn classify_kind(
    is_keyword_style: bool,
    action: Option<&str>,
    nargs: Option<&str>,
) -> ArgumentKind {
    if !is_keyword_style {
        return match nargs {
            Some("+") | Some("*") | Some("REMAINDER") => ArgumentKind::VarPositional,
            Some("?") => ArgumentKind::OptionalPositional,
            Some(n) if n.parse::<usize>().is_ok() => ArgumentKind::FixedArity,
            _ => ArgumentKind::Positional,
        };
    }
    match action {
        Some("store_true") | Some("store_false") => ArgumentKind::Flag,
        Some("append") => ArgumentKind::Repeated,
        _ => match nargs {
            Some("+") | Some("*") => ArgumentKind::Repeated,
            Some(n) if n.parse::<usize>().is_ok() => ArgumentKind::FixedArity,
            _ => ArgumentKind::Optional,
        },
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

/// Normalise a `nargs=...` value into the internal string form
/// `classify_kind` and the warning gate match on: the literal string for
/// `"?"`/`"+"`/`"*"`, the decimal string for an int count, or the
/// sentinel `"REMAINDER"` for `argparse.REMAINDER` — recognised either
/// as a dotted attribute (`argparse.REMAINDER`, tolerant of any import
/// alias for the `argparse` module) or a bare name (`REMAINDER`, from
/// `from argparse import REMAINDER`).
fn nargs_value(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
        Expr::NumberLiteral(n) => match &n.value {
            ruff_python_ast::Number::Int(i) => Some(i.to_string()),
            _ => None,
        },
        Expr::Attribute(a) if a.attr.as_str() == "REMAINDER" => Some("REMAINDER".to_string()),
        Expr::Name(n) if n.id.as_str() == "REMAINDER" => Some("REMAINDER".to_string()),
        _ => None,
    }
}

/// Best-effort textual rendering of a `nargs=...` value for warnings,
/// covering every shape the argparse docs allow: an int count, `"?"` /
/// `"+"` / `"*"`, and `argparse.REMAINDER` (a dotted attribute).
fn nargs_kwarg_repr(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringLiteral(s) => Some(format!("{:?}", s.value.to_str())),
        Expr::NumberLiteral(n) => match &n.value {
            ruff_python_ast::Number::Int(i) => Some(i.to_string()),
            _ => None,
        },
        Expr::Name(_) | Expr::Attribute(_) => Some(type_kwarg_repr(expr)),
        _ => None,
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

use std::path::Path;

/// Every file a block's `scan_paths` globs match under `root`, sorted and
/// deduplicated. Shared by the scanner and the static hash so both agree on
/// the file set; underscore-prefixed files are included here and skipped by
/// the scanner.
pub fn expand_scan_paths(
    root: &Path,
    scan_paths: &[String],
) -> Result<Vec<std::path::PathBuf>, ScanError> {
    let mut all_paths: Vec<std::path::PathBuf> = Vec::new();
    // The root is a literal path; only the user's `scan_paths` entry is a glob.
    let root_prefix = crate::glob_path::escape_path(root).ok_or_else(|| ScanError::Parse {
        path: root.display().to_string(),
        message: "non-utf8 project root".into(),
    })?;
    for pattern in scan_paths {
        // An absolute entry replaces the root, as `Path::join` did.
        let pattern_str = if Path::new(pattern).is_absolute() {
            pattern.clone()
        } else {
            format!("{root_prefix}/{pattern}")
        };
        // Bad glob pattern is a hard error so the user finds their typo.
        for path in glob::glob(&pattern_str)
            .map_err(|e| ScanError::Parse {
                path: pattern.clone(),
                message: e.to_string(),
            })?
            .flatten()
        {
            if path.is_file() {
                all_paths.push(path);
            }
        }
    }
    all_paths.sort();
    all_paths.dedup();
    Ok(all_paths)
}

/// Expand every glob in `scan_paths` against `root`, scan each match, and return one
/// `ScannedCommand` per file. Files that failed to parse become placeholder `ScannedCommand`s
/// (no arguments) with the failure recorded in their `warnings` field — the rest of the build
/// continues. Underscore-prefixed filenames (`_helpers.py`, `__init__.py`) are always skipped.
///
/// `django` opts this block into one additional Django management-command convention: a file
/// with no `add_argument()` calls is still kept if it defines Django's `Command` entry point.
/// Plain argparse blocks (`django == false`) treat a file with no `add_argument()` calls as a
/// non-command helper regardless of its shape.
pub fn scan_block_paths(
    root: &Path,
    scan_paths: &[String],
    django: bool,
) -> Result<Vec<ScannedCommand>, ScanError> {
    let all_paths = expand_scan_paths(root, scan_paths)?;

    let mut out = Vec::with_capacity(all_paths.len());
    for path in all_paths {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        // `_helpers.py`/`__init__.py` are plain Python convention for "not a command" —
        // applies regardless of `django`.
        if stem.starts_with('_') {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(err) => {
                let mut placeholder = ScannedCommand {
                    name: stem.clone(),
                    ..Default::default()
                };
                placeholder
                    .warnings
                    .push(format!("failed to read {}: {}", path.display(), err));
                out.push(placeholder);
                continue;
            }
        };
        match scan_source(&stem, &text) {
            Ok(cmd) => {
                // A no-arg file is usually a helper, not a command — grafting it would risk a clap
                // stem collision. Kept anyway for a Django block whose file defines `Command`:
                // that's a genuine zero-arg command.
                if cmd.arguments.is_empty() && !(django && cmd.is_django_command_class) {
                    continue;
                }
                out.push(cmd);
            }
            Err(ScanError::Parse { message, .. }) => {
                let mut placeholder = ScannedCommand {
                    name: stem,
                    ..Default::default()
                };
                placeholder.warnings.push(format!(
                    "failed to parse {}: {}",
                    path.display(),
                    message
                ));
                out.push(placeholder);
            }
        }
    }
    Ok(out)
}

/// Append `common` args to `scanned`, skipping any whose `name` collides
/// with an existing argument. The file's own argument always wins on
/// collision.
pub fn with_common_args(mut scanned: ScannedCommand, common: &[CommonArg]) -> ScannedCommand {
    let existing: std::collections::HashSet<&str> =
        scanned.arguments.iter().map(|a| a.name.as_str()).collect();
    let extras: Vec<Argument> = common
        .iter()
        .filter(|c| !existing.contains(c.name.as_str()))
        .map(|c| Argument {
            name: c.name.clone(),
            kind: c.kind,
            help: c.help.clone(),
            default: c.default.clone(),
            type_annotation: None,
            resolved_type: None,
            allowed_values: c.choices.clone().unwrap_or_default(),
            metadata: Default::default(),
            // common_args declared in `[tool.toolr.argparse.<name>]`
            // are toolr-config inputs, not scanned source — there's no
            // upstream literal to preserve.
            long_flag: None,
        })
        .collect();
    scanned.arguments.extend(extras);
    scanned
}

#[cfg(test)]
mod tests;
