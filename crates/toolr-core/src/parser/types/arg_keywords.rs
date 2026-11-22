//! Reject `arg(...)` calls the Python runtime would refuse, so a typo fails the
//! manifest build instead of raising `TypeError` the first time the command runs.

use std::collections::{HashMap, HashSet};

use ruff_python_ast::{Expr, ExprCall, Operator};

use super::supported::UnsupportedType;
use crate::parser::build::edit_distance;
use crate::parser::symbols::{ImportTable, TypeAliasTable};

/// `toolr.arg()`'s keywords. This must match the keyword-only parameters of `arg` in
/// `crates/toolr-py/python/toolr/utils/_signature.py`; the `arg_keywords_match_python_signature`
/// test enforces it.
pub const ACTIVE_ARG_KEYWORDS: &[&str] = &[
    "aliases",
    "metavar",
    "env",
    "hide",
    "help_section",
    "display_order",
    "conflicts_with",
    "requires",
];

fn is_arg_keyword(name: &str) -> bool {
    ACTIVE_ARG_KEYWORDS.contains(&name)
}

/// One [`UnsupportedType`] per unknown keyword, plus one per `arg()` call
/// that passes positional arguments, for every toolr `arg()` call anywhere
/// in the annotation, including through aliases visible from `module`.
pub(super) fn check_arg_calls(
    annotation: &Expr,
    aliases: &TypeAliasTable,
    all_imports: &HashMap<String, ImportTable>,
    module: &str,
) -> Vec<UnsupportedType> {
    let mut walker = Walker {
        aliases,
        all_imports,
        no_imports: ImportTable::default(),
        expanded: HashSet::new(),
        problems: Vec::new(),
    };
    walker.walk(annotation, module);
    walker.problems
}

struct Walker<'a> {
    aliases: &'a TypeAliasTable,
    all_imports: &'a HashMap<String, ImportTable>,
    /// Stands in for a module missing from `all_imports`.
    no_imports: ImportTable,
    expanded: HashSet<(String, String)>,
    problems: Vec<UnsupportedType>,
}

impl<'a> Walker<'a> {
    fn imports_for(&self, scope: &str) -> &ImportTable {
        self.all_imports.get(scope).unwrap_or(&self.no_imports)
    }

    /// `scope` is the module whose imports decide what names in `expr` mean.
    fn walk(&mut self, expr: &Expr, scope: &str) {
        match expr {
            Expr::Call(call) if calls_toolr_arg(call, self.imports_for(scope)) => {
                check_call(call, &mut self.problems)
            }
            Expr::Subscript(sub) => {
                self.walk(&sub.value, scope);
                self.walk(&sub.slice, scope);
            }
            Expr::Tuple(t) => {
                for elt in &t.elts {
                    self.walk(elt, scope);
                }
            }
            Expr::BinOp(b) if b.op == Operator::BitOr => {
                self.walk(&b.left, scope);
                self.walk(&b.right, scope);
            }
            Expr::Name(n) => {
                let Some((target, defined_in)) = self.visible_alias(n.id.as_str(), scope) else {
                    return;
                };
                // Each alias is expanded once, which also breaks `A = B; B = A` cycles.
                if self.expanded.insert((defined_in.clone(), n.id.to_string())) {
                    self.walk(target, &defined_in);
                }
            }
            _ => {}
        }
    }

    /// The alias `name` refers to from `scope`: defined there, or imported
    /// from the module that defines it. The merged table alone would also
    /// match same-named aliases the module never imported.
    fn visible_alias(&self, name: &str, scope: &str) -> Option<(&'a Expr, String)> {
        if let Some(target) = self.aliases.lookup_in(scope, name) {
            return Some((target, scope.to_string()));
        }
        let table = self.imports_for(scope);
        let imported = table.candidates(name).iter().find_map(|c| {
            self.aliases
                .lookup_in(&c.module, &c.original_name)
                .map(|target| (target, c.module.clone()))
        });
        if imported.is_some() || !table.has_star_import() {
            return imported;
        }
        // Star imports from modules we never parsed stay unexpanded: guessing
        // from the merged table could fail a legitimate build.
        table.star_import_modules().iter().find_map(|m| {
            self.aliases
                .lookup_in(m, name)
                .map(|target| (target, m.clone()))
        })
    }
}

fn check_call(call: &ExprCall, problems: &mut Vec<UnsupportedType>) {
    // Splats can't be checked statically; Python validates them at call time.
    if call
        .arguments
        .args
        .iter()
        .any(|a| !matches!(a, Expr::Starred(_)))
    {
        problems.push(UnsupportedType::PositionalArgArgument);
    }
    for kw in &call.arguments.keywords {
        let Some(name) = kw.arg.as_ref().map(|n| n.as_str()) else {
            continue;
        };
        if !is_arg_keyword(name) {
            let suggestion = match keyword_hint(name) {
                Some(_) => None,
                None => suggest_arg_keyword(name),
            };
            problems.push(UnsupportedType::UnknownArgKeyword {
                keyword: name.to_string(),
                suggestion,
            });
        }
    }
}

fn is_toolr_module(module: &str) -> bool {
    module == "toolr" || module.starts_with("toolr.")
}

/// Whether `call` resolves to `toolr.arg`, judged from this module's imports.
fn calls_toolr_arg(call: &ExprCall, table: &ImportTable) -> bool {
    match call.func.as_ref() {
        Expr::Name(n) => {
            if table.local_binding_wins(n.id.as_str()) {
                return false;
            }
            let candidates = table.candidates(n.id.as_str());
            if !candidates.is_empty() {
                return candidates
                    .iter()
                    .any(|c| is_toolr_module(&c.module) && c.original_name == "arg");
            }
            // An unresolvable binding (e.g. a star import) keeps the literal
            // name `arg` treated as toolr's.
            n.id.as_str() == "arg"
        }
        Expr::Attribute(a) if a.attr.as_str() == "arg" => {
            let mut root = a.value.as_ref();
            while let Expr::Attribute(inner) = root {
                root = inner.value.as_ref();
            }
            let Expr::Name(r) = root else {
                return false;
            };
            table
                .resolve_module_binding(r.id.as_str())
                .is_some_and(is_toolr_module)
                || table
                    .candidates(r.id.as_str())
                    .iter()
                    .any(|c| is_toolr_module(&c.module))
        }
        _ => false,
    }
}

/// The nearest keyword within the same `(len / 3).max(2)` edit distance the
/// unknown-group hint uses.
pub(super) fn suggest_arg_keyword(keyword: &str) -> Option<String> {
    let max = (keyword.len() / 3).max(2);
    let mut best: Option<(usize, &str)> = None;
    for candidate in ACTIVE_ARG_KEYWORDS {
        let d = edit_distance(keyword, candidate);
        if d <= max && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, candidate));
        }
    }
    best.map(|(_, s)| s.to_string())
}

/// Where toolr takes what an `arg()` keyword it doesn't accept would have set:
/// the path checks now live in `toolr.types`, and argparse-style keywords
/// come from the signature.
pub(super) fn keyword_hint(keyword: &str) -> Option<&'static str> {
    match keyword.strip_prefix("path_").unwrap_or(keyword) {
        "must_exist" => return Some("use `toolr.types.ResolvedPath` instead"),
        "must_be_file" => return Some("use `toolr.types.FilePath` instead"),
        "must_be_dir" => return Some("use `toolr.types.DirectoryPath` instead"),
        _ => {}
    }
    match keyword {
        "help" => Some("help text comes from the docstring's `Args:` section"),
        "type" => Some("the type comes from the annotation"),
        "default" => Some("the default comes from the parameter's default value"),
        "required" => Some(
            "use `T | None` for an optional argument, or `*args: T` for zero or more positionals",
        ),
        "action" => Some(
            "the action comes from the type: `bool` is a flag, `list[T]` appends, \
             `toolr.types.Count` counts",
        ),
        "choices" => Some("use `Literal[...]` or an `enum.Enum` subclass for the choices"),
        "nargs" => Some("use `T | None`, `*args: T` or `tuple[T1, T2, ...]`"),
        "group" => Some(
            "use `conflicts_with=[...]` for mutually exclusive arguments, or `help_section=` \
             for display grouping",
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
