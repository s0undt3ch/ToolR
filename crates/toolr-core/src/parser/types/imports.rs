//! Alias-tracking for `toolr.types` and `toolr.sources` imports.
//!
//! Both tables are populated by walking the module's top-level `import`
//! / `from ... import ...` statements with `ruff_python_parser`. They
//! exist so the rest of the type resolver can answer "is this bare
//! `RP` annotation a reference to `toolr.types.ResolvedPath`?" /
//! "is this `dispatched: DispatchCommand` parameter the runtime
//! injection slot, not a regular CLI arg?" without doing a full
//! symbol-table pass.

use ruff_python_ast::{Expr, ModModule, Stmt};

/// Records which symbols in the current module were imported from
/// `toolr.types` (or its submodules), and whether the user did
/// `import toolr.types as X` so `X.Foo` resolves cleanly too.
#[derive(Debug, Default, Clone)]
pub struct TypeImports {
    /// Symbol → canonical toolr.types name. E.g. `RP → "ResolvedPath"`
    /// after `from toolr.types import ResolvedPath as RP`.
    direct: std::collections::HashMap<String, String>,
    /// Module alias for `import toolr.types as X`. The aliased name
    /// (or `toolr` for `import toolr.types` without alias) maps onto
    /// `toolr.types`.
    module_aliases: Vec<String>,
}

/// Records which local symbols in the current module refer to
/// `toolr.sources.DispatchCommand` (or to the `toolr.sources` module
/// itself, when used as `toolr.sources.DispatchCommand`).
///
/// `DispatchCommand` is a runtime injection slot, not a CLI argument.
/// When a keyword-only parameter's annotation resolves to this type
/// the static parser must skip it during CLI-argument extraction —
/// otherwise the dispatcher command's `DispatchCommand` kwarg lands in
/// the type resolver as an unknown name and rejects the whole module.
///
/// Supported import shapes (mirrors `TypeImports`):
///
/// * `from toolr.sources import DispatchCommand`
/// * `from toolr.sources import DispatchCommand as <alias>`
/// * `import toolr.sources` (or `import toolr.sources as X`), then a
///   `toolr.sources.DispatchCommand` / `X.DispatchCommand` annotation.
///
/// Not currently handled: `from toolr import sources` followed by a
/// `sources.DispatchCommand` reference. That requires tracking which
/// local name aliases the `sources` submodule and is not in the
/// canonical-form spec; we'll add it if a real user hits it.
#[derive(Debug, Default, Clone)]
pub struct SourcesImports {
    /// Local names bound to `toolr.sources.DispatchCommand`. Populated by
    /// `from toolr.sources import DispatchCommand [as <alias>]`.
    direct_aliases: std::collections::HashSet<String>,
    /// Local names that refer to the `toolr.sources` module. Populated by
    /// `import toolr.sources` (default `toolr`) and `import toolr.sources as X`.
    module_aliases: Vec<String>,
}

impl TypeImports {
    /// Walk the module's top-level statements to find imports from
    /// `toolr.types`.
    pub fn from_module(module: &ModModule) -> Self {
        let mut imports = Self::default();
        for stmt in &module.body {
            match stmt {
                Stmt::ImportFrom(import) => {
                    let module_name = import
                        .module
                        .as_ref()
                        .map(|m| m.as_str())
                        .unwrap_or_default();
                    if module_name != "toolr.types" {
                        continue;
                    }
                    for alias in &import.names {
                        let canonical = alias.name.as_str().to_string();
                        let local = alias
                            .asname
                            .as_ref()
                            .map(|n| n.as_str().to_string())
                            .unwrap_or_else(|| canonical.clone());
                        imports.direct.insert(local, canonical);
                    }
                }
                Stmt::Import(import) => {
                    for alias in &import.names {
                        if alias.name.as_str() == "toolr.types" {
                            let local = alias
                                .asname
                                .as_ref()
                                .map(|n| n.as_str().to_string())
                                .unwrap_or_else(|| "toolr".to_string());
                            imports.module_aliases.push(local);
                        }
                    }
                }
                _ => {}
            }
        }
        imports
    }

    /// If `name` was imported from `toolr.types`, return its canonical
    /// `toolr.types` symbol name.
    pub(super) fn resolve_direct(&self, name: &str) -> Option<&str> {
        self.direct.get(name).map(String::as_str)
    }

    /// If `expr` is `<alias>.X` for an alias that points at `toolr.types`,
    /// return `X`. Handles `toolr.types.X` (when imported as
    /// `import toolr.types`) too.
    pub(super) fn resolve_attribute<'a>(&self, expr: &'a Expr) -> Option<&'a str> {
        let Expr::Attribute(attr) = expr else {
            return None;
        };
        let head = match attr.value.as_ref() {
            Expr::Name(n) => n.id.as_str().to_string(),
            Expr::Attribute(inner) => match inner.value.as_ref() {
                Expr::Name(n) if n.id.as_str() == "toolr" && inner.attr.as_str() == "types" => {
                    "toolr.types".to_string()
                }
                _ => return None,
            },
            _ => return None,
        };
        if head == "toolr.types" {
            return Some(attr.attr.as_str());
        }
        if self.module_aliases.iter().any(|a| a == &head) {
            return Some(attr.attr.as_str());
        }
        None
    }
}

impl SourcesImports {
    /// Walk the module's top-level statements to find imports that name
    /// `toolr.sources.DispatchCommand`.
    pub fn from_module(module: &ModModule) -> Self {
        let mut imports = Self::default();
        for stmt in &module.body {
            match stmt {
                Stmt::ImportFrom(import) => {
                    let module_name = import
                        .module
                        .as_ref()
                        .map(|m| m.as_str())
                        .unwrap_or_default();
                    if module_name != "toolr.sources" {
                        continue;
                    }
                    for alias in &import.names {
                        if alias.name.as_str() != "DispatchCommand" {
                            continue;
                        }
                        let local = alias
                            .asname
                            .as_ref()
                            .map(|n| n.as_str().to_string())
                            .unwrap_or_else(|| alias.name.as_str().to_string());
                        imports.direct_aliases.insert(local);
                    }
                }
                Stmt::Import(import) => {
                    for alias in &import.names {
                        if alias.name.as_str() == "toolr.sources" {
                            let local = alias
                                .asname
                                .as_ref()
                                .map(|n| n.as_str().to_string())
                                .unwrap_or_else(|| "toolr".to_string());
                            imports.module_aliases.push(local);
                        }
                    }
                }
                _ => {}
            }
        }
        imports
    }

    /// Whether `expr` is a parameter annotation referring to
    /// `toolr.sources.DispatchCommand` — either as a direct name (with
    /// or without aliasing) or as a `<module-alias>.DispatchCommand`
    /// attribute access.
    ///
    /// Also peels `Annotated[T, ...]` wrappers and handles string
    /// forward references (`"DispatchCommand"`, common under
    /// `from __future__ import annotations`). For forward refs we do a
    /// canonical-name compare against `direct_aliases` rather than
    /// reparsing the string — the realistic shapes are bare names like
    /// `"DispatchCommand"` or `"DC"` (after aliasing), and a literal
    /// match handles both without dragging the ruff parser into the
    /// static-analysis path.
    pub fn is_dispatch_command(&self, expr: &Expr) -> bool {
        // `Annotated[DispatchCommand, arg(...)]` — peel to the inner T.
        if let Some(inner) = peel_annotated_inner(expr) {
            return self.is_dispatch_command(inner);
        }
        // `"DispatchCommand"` / `"DC"` forward-ref string — match the
        // trimmed contents against direct aliases. Doesn't try to handle
        // dotted forward refs like `"toolr.sources.DispatchCommand"`;
        // those are vanishingly rare and a `parse_expression` round-trip
        // would be heavier than the value it adds.
        if let Expr::StringLiteral(s) = expr {
            return self.is_dispatch_command_name(s.value.to_str().trim());
        }
        match expr {
            Expr::Name(n) => self.direct_aliases.contains(n.id.as_str()),
            Expr::Attribute(attr) => {
                if attr.attr.as_str() != "DispatchCommand" {
                    return false;
                }
                match attr.value.as_ref() {
                    // `<alias>.DispatchCommand` for `import toolr.sources as <alias>`.
                    Expr::Name(n) => self.module_aliases.iter().any(|a| a == n.id.as_str()),
                    // `toolr.sources.DispatchCommand` for bare `import toolr.sources`.
                    Expr::Attribute(inner) => matches!(
                        inner.value.as_ref(),
                        Expr::Name(n) if n.id.as_str() == "toolr" && inner.attr.as_str() == "sources"
                    ),
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// Whether `name` is a local symbol bound to
    /// `toolr.sources.DispatchCommand` (e.g. `DispatchCommand` itself
    /// or any `as <alias>` rebinding). Used to resolve string
    /// forward-ref annotations without reparsing.
    pub fn is_dispatch_command_name(&self, name: &str) -> bool {
        self.direct_aliases.contains(name)
    }
}

/// If `expr` is `Annotated[T, ...]`, return `T`; otherwise `None`.
/// Mirrors the helper in `signatures.rs` — kept here so
/// `is_dispatch_command` can peel without that helper being public.
fn peel_annotated_inner(expr: &Expr) -> Option<&Expr> {
    let Expr::Subscript(sub) = expr else {
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
    match sub.slice.as_ref() {
        Expr::Tuple(t) => t.elts.first(),
        single => Some(single),
    }
}
