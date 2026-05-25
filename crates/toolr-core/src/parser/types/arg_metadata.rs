//! Extract clap-flavoured argument metadata from `Annotated[T,
//! arg(...), ...]` annotations.
//!
//! `arg(...)` calls inside an `Annotated[...]` wrapper carry the
//! aliases, metavar, env-fallback, conflicts/requires lists, hide
//! flag, display order, and help-section assignment that get plumbed
//! through to clap. This module reads them; `path_constraints.rs`
//! reads the orthogonal filesystem-check kwargs from the same call.

use ruff_python_ast::{Expr, ExprCall};

use super::is_toolr_arg_call;
use super::literals::{literal_str, literal_str_list, literal_u32};
use crate::manifest::{ArgMetadata, HelpSection};
use crate::parser::symbols::ArgSectionTable;

/// Harvest the full clap-flavoured metadata block from any
/// `Annotated[T, arg(...), ...]` annotation. Returns `None` when the
/// annotation isn't `Annotated[...]` or carries no metadata kwargs
/// other than path constraints (which travel separately).
pub fn extract_arg_metadata(
    annotation: &Expr,
    sections: &ArgSectionTable,
) -> Option<ArgMetadata> {
    let Expr::Subscript(sub) = annotation else {
        return None;
    };
    let head = match sub.value.as_ref() {
        Expr::Name(n) => n.id.as_str(),
        Expr::Attribute(a) => a.attr.as_str(),
        _ => return None,
    };
    if head != "Annotated" {
        return None;
    }
    let elts: Vec<&Expr> = match sub.slice.as_ref() {
        Expr::Tuple(t) => t.elts.iter().collect(),
        single => vec![single],
    };
    let mut md = ArgMetadata::default();
    let mut hit = false;
    for elt in elts.iter().skip(1) {
        let Expr::Call(call) = elt else { continue };
        if !is_toolr_arg_call(call) {
            continue;
        }
        for kw in &call.arguments.keywords {
            let Some(name) = kw.arg.as_ref().map(|n| n.as_str()) else {
                continue;
            };
            match name {
                "aliases" => {
                    if let Some(list) = literal_str_list(&kw.value) {
                        md.aliases = list;
                        hit = true;
                    }
                }
                "metavar" => {
                    if let Some(s) = literal_str(&kw.value) {
                        md.metavar = Some(s);
                        hit = true;
                    }
                }
                "env" => {
                    if let Some(s) = literal_str(&kw.value) {
                        md.env = Some(s);
                        hit = true;
                    }
                }
                "hide" => {
                    if let Expr::BooleanLiteral(b) = &kw.value {
                        md.hide = b.value;
                        if b.value {
                            hit = true;
                        }
                    }
                }
                "display_order" => {
                    if let Some(n) = literal_u32(&kw.value) {
                        md.display_order = Some(n);
                        hit = true;
                    }
                }
                "conflicts_with" => {
                    if let Some(list) = literal_str_list(&kw.value) {
                        md.conflicts_with = list;
                        hit = true;
                    }
                }
                "requires" => {
                    if let Some(list) = literal_str_list(&kw.value) {
                        md.requires = list;
                        hit = true;
                    }
                }
                "help_section" => {
                    if let Some(section) = resolve_help_section(&kw.value, sections) {
                        md.help_section = Some(section);
                        hit = true;
                    }
                }
                _ => {}
            }
        }
    }
    if hit { Some(md) } else { None }
}

fn resolve_help_section(value: &Expr, sections: &ArgSectionTable) -> Option<HelpSection> {
    // Three accepted shapes:
    //   1. `arg(help_section=LOGGING)` — reference a module-level
    //      `LOGGING = arg_section("...", description="...")` binding.
    //   2. `arg(help_section="Logging")` — bare string with no description.
    //   3. `arg(help_section=arg_section("Logging", description="..."))` —
    //      inline call, for users who want one-off sections without a
    //      module-level constant.
    match value {
        Expr::Name(n) => sections.lookup(n.id.as_str()).map(|entry| HelpSection {
            title: entry.title.clone(),
            description: entry.description.clone(),
        }),
        Expr::StringLiteral(s) => Some(HelpSection {
            title: s.value.to_str().to_string(),
            description: None,
        }),
        Expr::Call(call) if is_arg_section_call_expr(call) => parse_inline_arg_section(call),
        _ => None,
    }
}

fn is_arg_section_call_expr(call: &ExprCall) -> bool {
    match call.func.as_ref() {
        Expr::Name(n) => n.id.as_str() == "arg_section",
        Expr::Attribute(a) => a.attr.as_str() == "arg_section",
        _ => false,
    }
}

fn parse_inline_arg_section(call: &ExprCall) -> Option<HelpSection> {
    let title = call.arguments.args.first().and_then(literal_str)?;
    let description = call
        .arguments
        .keywords
        .iter()
        .find(|k| k.arg.as_ref().map(|n| n.as_str()) == Some("description"))
        .and_then(|k| literal_str(&k.value));
    Some(HelpSection { title, description })
}
