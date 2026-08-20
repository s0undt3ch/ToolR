//! Symbol table for resolving local type names to their declarations.

use std::collections::HashMap;

use ruff_python_ast::{Expr, ModModule, Stmt, StmtAssign, StmtClassDef};

/// A single enum member: its Python name (`ADD`) and serialised value
/// (`"add"` for `StrEnum`, or the name itself when the underlying type
/// is opaque).
#[derive(Debug, Clone)]
pub struct EnumMember {
    pub name: String,
    pub value: String,
}

/// A class name's enum members plus the dotted module path that
/// defines it, so that two unrelated classes sharing a bare name
/// (e.g. `Database` in two different `tools/*.py` files) don't
/// collide when tables from multiple modules are merged (GH #449).
#[derive(Debug, Clone)]
struct EnumDef {
    module: String,
    members: Vec<EnumMember>,
}

/// Whether two enum definitions serialise identically — same members,
/// same names, same values, same order. Order matters because it
/// drives `allowed_values` display order in `--help`.
fn same_members(a: &[EnumMember], b: &[EnumMember]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.name == y.name && x.value == y.value)
}

/// Collapse a set of candidate resolutions to one: none found is `None`;
/// exactly one is unambiguous; several that all serialise identically
/// are treated as unambiguous too (picking any of them is observably
/// the same); a genuine mismatch is ambiguous and returns `None`.
fn dedupe_matches(matched: Vec<(String, &[EnumMember])>) -> Option<(String, &[EnumMember])> {
    match matched.as_slice() {
        [] => None,
        [only] => Some(only.clone()),
        [first, rest @ ..] if rest.iter().all(|(_, m)| same_members(m, first.1)) => {
            Some(first.clone())
        }
        _ => None,
    }
}

/// Mapping of local class name → enum members, for classes that look
/// like an `Enum` subclass. Tracks both the member name (`ADD`) and
/// its serialised value (`"add"`) so we can resolve attribute-style
/// defaults like `Operation.ADD` to the CLI-visible value.
///
/// Keyed by bare class name, but each entry carries every module that
/// defines a class with that name — `resolve_def` picks the one that's
/// actually in scope for the caller's module rather than whichever
/// definition happened to be merged in last.
#[derive(Debug, Default, Clone)]
pub struct EnumTable {
    members: HashMap<String, Vec<EnumDef>>,
}

impl EnumTable {
    pub fn from_module(module: &ModModule, module_path: &str) -> Self {
        let mut table = EnumTable::default();
        for stmt in &module.body {
            let Stmt::ClassDef(class) = stmt else {
                continue;
            };
            if !is_enum_subclass(class) {
                continue;
            }
            let members = class
                .body
                .iter()
                .filter_map(member_value)
                .collect::<Vec<_>>();
            if !members.is_empty() {
                table
                    .members
                    .entry(class.name.to_string())
                    .or_default()
                    .push(EnumDef {
                        module: module_path.to_string(),
                        members,
                    });
            }
        }
        table
    }

    /// Pick the `class` definition that's actually visible from
    /// `current_module`, returning its declaring module alongside its
    /// members. Resolution order:
    ///
    /// 1. A same-module definition always wins.
    /// 2. `current_module`'s own explicit imports (`all_imports`) name a
    ///    defining module for `class` — follow it, recursing through
    ///    that module's own imports (cycle/depth guarded) when it's
    ///    itself just a re-export (e.g. an `__init__.py` re-exporting a
    ///    sibling module's class). Multiple import candidates (only
    ///    possible via a `try`/`except` dual-path import) resolve if
    ///    they all land on the same member set, else are ambiguous.
    /// 3. No matching import at all: fall back to the pre-#454
    ///    same-name-across-modules heuristic — an unambiguous single
    ///    cross-module definition, or several definitions that all
    ///    serialise identically, resolve; a genuine mismatch is
    ///    ambiguous. This is the path for code the import scanner can't
    ///    see (star imports, dotted-attribute-chain usage — both
    ///    rejected outright at the call site instead, see
    ///    `resolve_name`) or that simply predates real import tracking.
    fn resolve_def(
        &self,
        class: &str,
        current_module: &str,
        all_imports: &HashMap<String, ImportTable>,
    ) -> Option<(String, &[EnumMember])> {
        if let Some(found) = self.declared_in(class, current_module) {
            return Some(found);
        }
        let empty = ImportTable::default();
        let imports = all_imports.get(current_module).unwrap_or(&empty);
        let candidates = imports.candidates(class);
        if !candidates.is_empty() {
            let matched: Vec<(String, &[EnumMember])> = candidates
                .iter()
                .filter_map(|c| {
                    let mut seen = vec![(c.module.clone(), c.original_name.clone())];
                    self.follow_import_chain(&c.original_name, &c.module, all_imports, &mut seen)
                })
                .collect();
            return dedupe_matches(matched);
        }
        // No explicit import at all for `class` in `current_module`:
        // fall back to the pre-#454 same-name-across-modules heuristic.
        // Only meaningful if some module somewhere literally declares a
        // class with this exact name.
        dedupe_matches(self.members.get(class).map(|defs| {
            defs.iter()
                .map(|d| (d.module.clone(), d.members.as_slice()))
                .collect()
        })?)
    }

    /// A same-module `ClassDef` for `class`, if one exists — the one
    /// resolution step that never depends on imports.
    fn declared_in(&self, class: &str, module: &str) -> Option<(String, &[EnumMember])> {
        self.members
            .get(class)?
            .iter()
            .find(|d| d.module == module)
            .map(|d| (d.module.clone(), d.members.as_slice()))
    }

    /// Follow an import candidate to its actual declaration, recursing
    /// through further re-exports (e.g. an `__init__.py` re-exporting a
    /// sibling module's class, or a chain of `as`-aliased re-imports).
    /// Cycle/depth guarded via `seen`. Deliberately does **not** apply
    /// the same-name-across-modules guessing heuristic at any point in
    /// the chain — an explicit import that turns out to point nowhere
    /// real must fail cleanly, not silently fall back to some unrelated
    /// module's same-named class.
    fn follow_import_chain(
        &self,
        class: &str,
        module: &str,
        all_imports: &HashMap<String, ImportTable>,
        seen: &mut Vec<(String, String)>,
    ) -> Option<(String, &[EnumMember])> {
        if let Some(found) = self.declared_in(class, module) {
            return Some(found);
        }
        let empty = ImportTable::default();
        let imports = all_imports.get(module).unwrap_or(&empty);
        let candidates = imports.candidates(class);
        if candidates.is_empty() {
            return None;
        }
        let matched: Vec<(String, &[EnumMember])> = candidates
            .iter()
            .filter_map(|c| {
                let key = (c.module.clone(), c.original_name.clone());
                if seen.contains(&key) || seen.len() > 8 {
                    return None; // cycle or pathological depth
                }
                seen.push(key);
                self.follow_import_chain(&c.original_name, &c.module, all_imports, seen)
            })
            .collect();
        dedupe_matches(matched)
    }

    /// Declaring module and serialised values for `class`. Used for
    /// `allowed_values` and for carrying the defining module onto
    /// `SupportedType::Enum`.
    pub fn lookup(
        &self,
        class: &str,
        current_module: &str,
        all_imports: &HashMap<String, ImportTable>,
    ) -> Option<(String, Vec<String>)> {
        self.resolve_def(class, current_module, all_imports)
            .map(|(module, m)| (module, m.iter().map(|em| em.value.clone()).collect()))
    }

    /// Resolve `class.member` to its serialised value. Used when
    /// rendering enum-attribute defaults in `--help`.
    pub fn lookup_member(
        &self,
        class: &str,
        member: &str,
        current_module: &str,
        all_imports: &HashMap<String, ImportTable>,
    ) -> Option<&str> {
        self.resolve_def(class, current_module, all_imports)?
            .1
            .iter()
            .find(|em| em.name == member)
            .map(|em| em.value.as_str())
    }

    pub fn merge(&mut self, other: EnumTable) {
        for (name, defs) in other.members {
            self.members.entry(name).or_default().extend(defs);
        }
    }
}

/// One module-level import statement that bound a name into scope,
/// resolved to the absolute module it actually names.
#[derive(Debug, Clone)]
pub struct ImportedFrom {
    pub module: String,
    pub original_name: String,
    pub via_type_checking: bool,
}

/// Every `from X import Y [as Z]` bound in a module's top-level scope —
/// including relative imports, `TYPE_CHECKING`-guarded imports, and
/// `try`/`except` dual-path imports. Deliberately does *not* track
/// `import X` + attribute-chain usage (`X.Y`) as a resolvable source for
/// arbitrary user classes — that shape is rejected outright for
/// command-signature resolution (see `has_star_import` and the
/// module-binding tracking used for that rejection).
///
/// Used by [`EnumTable::resolve_def`] so a class imported from a
/// different module than the one declaring it resolves to exactly that
/// module, rather than guessing across same-named classes elsewhere.
#[derive(Debug, Default, Clone)]
pub struct ImportTable {
    entries: HashMap<String, Vec<ImportedFrom>>,
}

impl ImportTable {
    pub fn from_module(module: &ModModule, module_path: &str, is_package: bool) -> Self {
        let mut table = Self::default();
        for stmt in &module.body {
            table.collect_stmt(stmt, module_path, is_package, false);
        }
        table
    }

    fn collect_stmt(
        &mut self,
        stmt: &Stmt,
        module_path: &str,
        is_package: bool,
        via_type_checking: bool,
    ) {
        match stmt {
            Stmt::ImportFrom(import) => {
                self.collect_import_from(import, module_path, is_package, via_type_checking);
            }
            Stmt::If(if_stmt) if is_type_checking_test(&if_stmt.test) => {
                for inner in &if_stmt.body {
                    self.collect_stmt(inner, module_path, is_package, true);
                }
                // Deliberately not walking elif_else_clauses: `else:` on a
                // TYPE_CHECKING guard is real runtime code (the opposite
                // branch), not another TYPE_CHECKING import source.
            }
            Stmt::Try(try_stmt) => {
                for inner in &try_stmt.body {
                    self.collect_stmt(inner, module_path, is_package, via_type_checking);
                }
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(h) = handler;
                    for inner in &h.body {
                        self.collect_stmt(inner, module_path, is_package, via_type_checking);
                    }
                }
                // Deliberately not walking `orelse`/`finalbody` — see the
                // design doc for why this stays out of scope.
            }
            _ => {}
        }
    }

    fn collect_import_from(
        &mut self,
        import: &ruff_python_ast::StmtImportFrom,
        module_path: &str,
        is_package: bool,
        via_type_checking: bool,
    ) {
        let target_module = if import.level == 0 {
            match import.module.as_ref().map(|m| m.as_str()) {
                Some(m) => m.to_string(),
                None => return,
            }
        } else {
            let Some(m) = resolve_relative_module(
                import.level,
                import.module.as_ref().map(|m| m.as_str()),
                module_path,
                is_package,
            ) else {
                return;
            };
            m
        };
        for alias in &import.names {
            if alias.name.as_str() == "*" {
                continue;
            }
            let original_name = alias.name.as_str().to_string();
            let local = alias
                .asname
                .as_ref()
                .map(|n| n.as_str().to_string())
                .unwrap_or_else(|| original_name.clone());
            self.entries.entry(local).or_default().push(ImportedFrom {
                module: target_module.clone(),
                original_name,
                via_type_checking,
            });
        }
    }

    /// All candidates bound to `name` by an explicit import. Empty if
    /// `name` was never imported. More than one entry means either a
    /// `try`/`except` dual-path import, or (rarely) the same name
    /// re-imported more than once in the same module.
    pub fn candidates(&self, name: &str) -> &[ImportedFrom] {
        self.entries.get(name).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// Whether `expr` is exactly `TYPE_CHECKING` or `typing.TYPE_CHECKING`.
/// Anything more complex (`TYPE_CHECKING and DEBUG`, `not TYPE_CHECKING`)
/// isn't recognised — that's not the documented mypy/ruff-endorsed
/// pattern, and treating it as "definitely a TYPE_CHECKING guard" would
/// be guessing at intent.
fn is_type_checking_test(expr: &Expr) -> bool {
    match expr {
        Expr::Name(n) => n.id.as_str() == "TYPE_CHECKING",
        Expr::Attribute(a) => {
            a.attr.as_str() == "TYPE_CHECKING"
                && matches!(a.value.as_ref(), Expr::Name(n) if n.id.as_str() == "typing")
        }
        _ => false,
    }
}

/// Resolve a relative import's absolute target module.
///
/// `level` is the dot-count (`from . import X` -> 1, `from .. import X`
/// -> 2). `module` is the part after the dots (`None` for bare
/// `from . import X`). `current_module` is the *importing* module's own
/// dotted path, as computed by `module_path_for_prefix`. For a plain
/// file, that path includes the file's own trailing segment, which one
/// dot must drop to reach "this file's package" — so `level` segments
/// are popped. For an `__init__.py`, `module_path_for_prefix` has
/// already collapsed the path to the package itself (no trailing
/// `__init__` segment to drop), so one dot means "this same package,"
/// i.e. only `level - 1` segments are popped.
fn resolve_relative_module(
    level: u32,
    module: Option<&str>,
    current_module: &str,
    is_package: bool,
) -> Option<String> {
    let pops = if is_package {
        level.saturating_sub(1)
    } else {
        level
    };
    let mut segments: Vec<&str> = current_module.split('.').collect();
    for _ in 0..pops {
        segments.pop()?;
    }
    let mut out = segments.join(".");
    if let Some(m) = module {
        if !out.is_empty() {
            out.push('.');
        }
        out.push_str(m);
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Module-level `NAME = <literal>` assignments. Used so a function
/// parameter default like `mode: str = DEFAULT_MODE` resolves to the
/// underlying literal when `DEFAULT_MODE = "fast"` is defined in the
/// same module.
///
/// Only directly-literal RHS values are stored (strings, numbers,
/// booleans, `None`). Names pointing at other names, function calls,
/// attribute references, and other compound expressions are skipped
/// — looking those up still yields `None`, which keeps the existing
/// `<expr>` sentinel path active for unresolvable cases.
#[derive(Debug, Default, Clone)]
pub struct ConstTable {
    values: HashMap<String, String>,
}

impl ConstTable {
    pub fn from_module(module: &ModModule) -> Self {
        let mut table = ConstTable::default();
        for stmt in &module.body {
            let Stmt::Assign(assign) = stmt else {
                continue;
            };
            // Only handle single-target assignments: `NAME = value`.
            // Multi-target / tuple / attribute targets are skipped.
            let [target] = assign.targets.as_slice() else {
                continue;
            };
            let Expr::Name(name) = target else {
                continue;
            };
            if let Some(value) = literal_value(&assign.value) {
                table.values.insert(name.id.to_string(), value);
            }
        }
        table
    }

    /// Resolve a bare name reference to its module-level literal value
    /// when known. Returns `None` if the name isn't a tracked literal
    /// constant.
    pub fn lookup(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    pub fn merge(&mut self, other: ConstTable) {
        self.values.extend(other.values);
    }
}

/// Resolve an `Expr` to its serialised literal value. Mirrors the
/// primitives handled by `literal_default` in `signatures.rs` (kept
/// in sync intentionally: a Python parameter default and a
/// module-level constant share the same notion of "resolvable
/// literal"). Returns `None` for non-literal expressions.
fn literal_value(expr: &Expr) -> Option<String> {
    use ruff_python_ast::Number;
    match expr {
        Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
        Expr::NumberLiteral(n) => Some(match &n.value {
            Number::Int(i) => i.to_string(),
            Number::Float(f) => f.to_string(),
            Number::Complex { real, imag } => format!("({real}+{imag}j)"),
        }),
        Expr::BooleanLiteral(b) => Some(if b.value { "true" } else { "false" }.to_string()),
        Expr::NoneLiteral(_) => Some(String::new()),
        _ => None,
    }
}

/// Module-level type aliases that the rust static parser knows how to
/// follow. Triggered by patterns like:
///
/// ```python
/// CommitHash = Annotated[str | None, arg(aliases=["--sha"])]
/// MaybeName  = str | None
/// HostList   = list[str]
/// ```
///
/// The RHS must look like a parameter annotation (a `Name` / `Attribute`
/// / `Subscript` / `BinOp` shape). Anything else — function calls,
/// numeric literals, builders — is ignored. The resolver consults the
/// table after exhausting primitives / `toolr.types` / enums, so user
/// shadowing is impossible.
#[derive(Debug, Default, Clone)]
pub struct TypeAliasTable {
    aliases: HashMap<String, Expr>,
}

impl TypeAliasTable {
    pub fn from_module(module: &ModModule) -> Self {
        let mut table = TypeAliasTable::default();
        for stmt in &module.body {
            let Stmt::Assign(StmtAssign { targets, value, .. }) = stmt else {
                continue;
            };
            if targets.len() != 1 {
                continue;
            }
            let Expr::Name(target) = &targets[0] else {
                continue;
            };
            if !looks_like_annotation(value.as_ref()) {
                continue;
            }
            table
                .aliases
                .insert(target.id.as_str().to_string(), (**value).clone());
        }
        table
    }

    /// Returns the underlying annotation expression for `name`, if it
    /// was assigned via a module-level type alias.
    pub fn lookup(&self, name: &str) -> Option<&Expr> {
        self.aliases.get(name)
    }

    pub fn merge(&mut self, other: TypeAliasTable) {
        self.aliases.extend(other.aliases);
    }
}

fn looks_like_annotation(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Name(_) | Expr::Attribute(_) | Expr::Subscript(_) | Expr::BinOp(_)
    )
}

/// One resolved `arg_section(...)` binding at module scope.
#[derive(Debug, Clone, Default)]
pub struct ArgSectionEntry {
    pub title: String,
    pub description: Option<String>,
}

/// Module-level bindings of the form
/// ``NAME = arg_section("Title", description="...")``. The type
/// resolver consults this table when an `arg(help_section=NAME)`
/// reference is encountered inside an `Annotated[...]` annotation.
#[derive(Debug, Default, Clone)]
pub struct ArgSectionTable {
    sections: HashMap<String, ArgSectionEntry>,
}

impl ArgSectionTable {
    pub fn from_module(module: &ModModule) -> Self {
        let mut table = ArgSectionTable::default();
        for stmt in &module.body {
            let Stmt::Assign(StmtAssign { targets, value, .. }) = stmt else {
                continue;
            };
            if targets.len() != 1 {
                continue;
            }
            let Expr::Name(target) = &targets[0] else {
                continue;
            };
            let Expr::Call(call) = value.as_ref() else {
                continue;
            };
            if !is_arg_section_call(call) {
                continue;
            }
            let Some(entry) = parse_arg_section_call(call) else {
                continue;
            };
            table.sections.insert(target.id.as_str().to_string(), entry);
        }
        table
    }

    pub fn lookup(&self, name: &str) -> Option<&ArgSectionEntry> {
        self.sections.get(name)
    }

    pub fn merge(&mut self, other: ArgSectionTable) {
        self.sections.extend(other.sections);
    }
}

fn is_arg_section_call(call: &ruff_python_ast::ExprCall) -> bool {
    match call.func.as_ref() {
        Expr::Name(n) => n.id.as_str() == "arg_section",
        Expr::Attribute(a) => a.attr.as_str() == "arg_section",
        _ => false,
    }
}

fn parse_arg_section_call(call: &ruff_python_ast::ExprCall) -> Option<ArgSectionEntry> {
    let title = call.arguments.args.first().and_then(literal_str)?;
    let description = call
        .arguments
        .keywords
        .iter()
        .find(|k| k.arg.as_ref().map(|n| n.as_str()) == Some("description"))
        .and_then(|k| literal_str(&k.value));
    Some(ArgSectionEntry { title, description })
}

fn literal_str(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
        _ => None,
    }
}

fn is_enum_subclass(class: &StmtClassDef) -> bool {
    let Some(args) = class.arguments.as_ref() else {
        return false;
    };
    args.args.iter().any(matches_enum_name)
}

fn matches_enum_name(expr: &Expr) -> bool {
    let name = match expr {
        Expr::Name(n) => n.id.as_str(),
        Expr::Attribute(a) => a.attr.as_str(),
        _ => return false,
    };
    matches!(name, "Enum" | "IntEnum" | "StrEnum" | "Flag" | "IntFlag")
}

fn member_value(stmt: &Stmt) -> Option<EnumMember> {
    let Stmt::Assign(a) = stmt else {
        return None;
    };
    let member_name = match a.targets.first()? {
        Expr::Name(n) => n.id.as_str().to_string(),
        _ => return None,
    };
    let value = match a.value.as_ref() {
        Expr::StringLiteral(s) => s.value.to_str().to_string(),
        // Non-string values (IntEnum / Flag): fall back to the member
        // name. Richer extraction is future work.
        _ => member_name.clone(),
    };
    Some(EnumMember {
        name: member_name,
        value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_python_file;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn parse(src: &str) -> ModModule {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(src.as_bytes()).unwrap();
        parse_python_file(f.path()).unwrap()
    }

    #[test]
    fn import_table_resolves_direct_absolute_import() {
        let src = "from tools.metrics._common import Environment\n";
        let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
        let candidates = table.candidates("Environment");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].module, "tools.metrics._common");
        assert_eq!(candidates[0].original_name, "Environment");
        assert!(!candidates[0].via_type_checking);
    }

    #[test]
    fn import_table_resolves_aliased_import() {
        let src = "from tools.metrics._common import Environment as Env\n";
        let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
        let candidates = table.candidates("Env");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].module, "tools.metrics._common");
        assert_eq!(candidates[0].original_name, "Environment");
    }

    #[test]
    fn import_table_ignores_unrelated_names() {
        let src = "from tools.metrics._common import Environment\n";
        let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
        assert!(table.candidates("SomethingElse").is_empty());
    }

    #[test]
    fn import_table_handles_multiple_names_one_statement() {
        let src = "from tools.metrics._common import Environment, Region as R\n";
        let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
        assert_eq!(
            table.candidates("Environment")[0].module,
            "tools.metrics._common"
        );
        assert_eq!(table.candidates("R")[0].original_name, "Region");
    }

    #[test]
    fn import_table_resolves_relative_sibling_import() {
        let src = "from ._common import Environment\n";
        let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
        assert_eq!(
            table.candidates("Environment")[0].module,
            "tools.metrics._common"
        );
    }

    #[test]
    fn import_table_resolves_relative_parent_import() {
        // `from .. import shared` inside `tools.metrics.sub.analyse`
        // binds the *submodule* `shared` living in the parent package
        // `tools.metrics` — same bookkeeping shape as any other import
        // (module = the package, original_name = what's imported from
        // it). Task 6 is responsible for rejecting this as a class
        // source when used in a command signature (it names a module,
        // not a class).
        let src = "from .. import shared\n";
        let table = ImportTable::from_module(&parse(src), "tools.metrics.sub.analyse", false);
        assert_eq!(table.candidates("shared")[0].module, "tools.metrics");
        assert_eq!(table.candidates("shared")[0].original_name, "shared");
    }

    #[test]
    fn import_table_resolves_relative_import_from_package_root() {
        let src = "from ._common import Environment\n";
        let table = ImportTable::from_module(&parse(src), "tools.metrics", true);
        assert_eq!(
            table.candidates("Environment")[0].module,
            "tools.metrics._common"
        );
    }

    #[test]
    fn import_table_tags_type_checking_import() {
        let src = "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from tools.metrics._common import Environment\n";
        let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
        let c = table.candidates("Environment");
        assert_eq!(c.len(), 1);
        assert!(c[0].via_type_checking);
    }

    #[test]
    fn import_table_recognises_dotted_type_checking() {
        let src = "import typing\nif typing.TYPE_CHECKING:\n    from tools.metrics._common import Environment\n";
        let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
        assert!(table.candidates("Environment")[0].via_type_checking);
    }

    #[test]
    fn import_table_ignores_type_checking_nested_in_function() {
        let src = r#"
from typing import TYPE_CHECKING

def helper():
    if TYPE_CHECKING:
        from tools.metrics._common import Environment
"#;
        let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
        assert!(table.candidates("Environment").is_empty());
    }

    #[test]
    fn import_table_collects_both_try_except_branches() {
        let src = r#"
try:
    from tools.metrics._common import Environment
except ImportError:
    from tools.metrics._legacy import Environment
"#;
        let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
        let c = table.candidates("Environment");
        assert_eq!(c.len(), 2);
        let modules: Vec<&str> = c.iter().map(|i| i.module.as_str()).collect();
        assert!(modules.contains(&"tools.metrics._common"));
        assert!(modules.contains(&"tools.metrics._legacy"));
    }

    #[test]
    fn import_table_records_star_import_presence() {
        let src = "from tools.metrics._common import *\n";
        let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
        assert!(table.candidates("anything_at_all").is_empty());
    }

    #[test]
    fn collects_string_enum_values() {
        let src = r#"
from enum import StrEnum

class Mode(StrEnum):
    FAST = "fast"
    SLOW = "slow"
"#;
        let table = EnumTable::from_module(&parse(src), "tools.example");
        let (module, vals) = table
            .lookup("Mode", "tools.example", &std::collections::HashMap::new())
            .unwrap();
        assert_eq!(module, "tools.example");
        assert_eq!(vals, vec!["fast".to_string(), "slow".to_string()]);
    }

    #[test]
    fn lookup_member_returns_serialised_value() {
        let src = r#"
from enum import StrEnum

class Operation(StrEnum):
    ADD = "add"
    SUBTRACT = "subtract"
"#;
        let table = EnumTable::from_module(&parse(src), "tools.example");
        assert_eq!(
            table.lookup_member(
                "Operation",
                "ADD",
                "tools.example",
                &std::collections::HashMap::new()
            ),
            Some("add")
        );
        assert_eq!(
            table.lookup_member(
                "Operation",
                "SUBTRACT",
                "tools.example",
                &std::collections::HashMap::new()
            ),
            Some("subtract")
        );
        assert_eq!(
            table.lookup_member(
                "Operation",
                "MISSING",
                "tools.example",
                &std::collections::HashMap::new()
            ),
            None
        );
        assert_eq!(
            table.lookup_member(
                "OtherClass",
                "ADD",
                "tools.example",
                &std::collections::HashMap::new()
            ),
            None
        );
    }

    #[test]
    fn int_enum_member_falls_back_to_name() {
        let src = r#"
from enum import IntEnum

class Code(IntEnum):
    OK = 0
    ERROR = 1
"#;
        let table = EnumTable::from_module(&parse(src), "tools.example");
        // No string value, so we record the member's own name.
        assert_eq!(
            table.lookup_member(
                "Code",
                "OK",
                "tools.example",
                &std::collections::HashMap::new()
            ),
            Some("OK")
        );
        assert_eq!(
            table.lookup_member(
                "Code",
                "ERROR",
                "tools.example",
                &std::collections::HashMap::new()
            ),
            Some("ERROR")
        );
    }

    #[test]
    fn ignores_non_enum_classes() {
        let src = r#"
class Foo:
    X = "x"
"#;
        let table = EnumTable::from_module(&parse(src), "tools.example");
        assert!(table
            .lookup("Foo", "tools.example", &std::collections::HashMap::new())
            .is_none());
    }

    #[test]
    fn colliding_bare_names_resolve_per_module_not_last_merged_wins() {
        let a = parse(
            r#"
from enum import StrEnum

class Database(StrEnum):
    PRIMARY = "primary"
"#,
        );
        let b = parse(
            r#"
from enum import StrEnum

class Database(StrEnum):
    REPLICA = "replica"
"#,
        );
        let mut table = EnumTable::from_module(&a, "tools.module_a");
        table.merge(EnumTable::from_module(&b, "tools.module_b"));

        assert_eq!(
            table
                .lookup(
                    "Database",
                    "tools.module_a",
                    &std::collections::HashMap::new()
                )
                .unwrap(),
            ("tools.module_a".to_string(), vec!["primary".to_string()])
        );
        assert_eq!(
            table
                .lookup(
                    "Database",
                    "tools.module_b",
                    &std::collections::HashMap::new()
                )
                .unwrap(),
            ("tools.module_b".to_string(), vec!["replica".to_string()])
        );
        // Neither module's definition wins from an unrelated module —
        // the collision is genuinely ambiguous, so this resolves to
        // nothing rather than silently picking one (GH #449).
        assert!(table
            .lookup(
                "Database",
                "tools.module_c",
                &std::collections::HashMap::new()
            )
            .is_none());
    }

    #[test]
    fn single_definition_resolves_from_an_unrelated_module() {
        let src = r#"
from enum import StrEnum

class Mode(StrEnum):
    FAST = "fast"
"#;
        let table = EnumTable::from_module(&parse(src), "tools.module_a");

        assert_eq!(
            table
                .lookup("Mode", "tools.module_b", &std::collections::HashMap::new())
                .unwrap(),
            ("tools.module_a".to_string(), vec!["fast".to_string()])
        );
    }

    #[test]
    fn resolve_def_prefers_explicit_import_over_guessing() {
        // Two modules declare a *different*-shaped `Database` class.
        // Without an import this is the #449 ambiguous case; with an
        // explicit import naming one of them, it resolves to exactly
        // that one instead of erroring or guessing.
        let a = parse(
            "from enum import StrEnum\nclass Database(StrEnum):\n    PRIMARY = \"primary\"\n",
        );
        let b = parse(
            "from enum import StrEnum\nclass Database(StrEnum):\n    REPLICA = \"replica\"\n",
        );
        let mut table = EnumTable::from_module(&a, "tools.module_a");
        table.merge(EnumTable::from_module(&b, "tools.module_b"));

        let mut all_imports: HashMap<String, ImportTable> = HashMap::new();
        all_imports.insert(
            "tools.module_c".to_string(),
            ImportTable::from_module(
                &parse("from tools.module_b import Database\n"),
                "tools.module_c",
                false,
            ),
        );
        assert_eq!(
            table
                .lookup("Database", "tools.module_c", &all_imports)
                .unwrap(),
            ("tools.module_b".to_string(), vec!["replica".to_string()])
        );
    }

    #[test]
    fn resolve_def_import_pointing_nowhere_known_is_none() {
        let a = parse(
            "from enum import StrEnum\nclass Database(StrEnum):\n    PRIMARY = \"primary\"\n",
        );
        let table = EnumTable::from_module(&a, "tools.module_a");
        let mut all_imports: HashMap<String, ImportTable> = HashMap::new();
        all_imports.insert(
            "tools.module_c".to_string(),
            ImportTable::from_module(
                &parse("from tools.somewhere_else import Database\n"),
                "tools.module_c",
                false,
            ),
        );
        assert!(table
            .lookup("Database", "tools.module_c", &all_imports)
            .is_none());
    }

    #[test]
    fn resolve_def_follows_reexport_chain_through_init() {
        // tools/metrics/_common.py declares Environment.
        // tools/metrics/__init__.py does `from ._common import Environment`.
        // tools/analyse.py does `from tools.metrics import Environment`.
        let common = parse("class Environment(enum.StrEnum):\n    PRODUCTION = \"production\"\n");
        let enums = EnumTable::from_module(&common, "tools.metrics._common");

        let mut all_imports: HashMap<String, ImportTable> = HashMap::new();
        all_imports.insert(
            "tools.metrics".to_string(),
            ImportTable::from_module(
                &parse("from ._common import Environment\n"),
                "tools.metrics",
                true,
            ),
        );
        all_imports.insert(
            "tools.analyse".to_string(),
            ImportTable::from_module(
                &parse("from tools.metrics import Environment\n"),
                "tools.analyse",
                false,
            ),
        );

        assert_eq!(
            enums
                .lookup("Environment", "tools.analyse", &all_imports)
                .unwrap(),
            (
                "tools.metrics._common".to_string(),
                vec!["production".to_string()]
            )
        );
    }

    #[test]
    fn resolve_def_follows_aliasing_chain() {
        // mod1 imports Foo from tools.real and re-aliases it as Bar;
        // mod2 imports Bar from mod1.
        let real = parse("class Foo(enum.StrEnum):\n    A = \"a\"\n");
        let enums = EnumTable::from_module(&real, "tools.real");

        let mut all_imports: HashMap<String, ImportTable> = HashMap::new();
        all_imports.insert(
            "tools.mod1".to_string(),
            ImportTable::from_module(
                &parse("from tools.real import Foo as Bar\n"),
                "tools.mod1",
                false,
            ),
        );
        all_imports.insert(
            "tools.mod2".to_string(),
            ImportTable::from_module(&parse("from tools.mod1 import Bar\n"), "tools.mod2", false),
        );

        assert_eq!(
            enums.lookup("Bar", "tools.mod2", &all_imports).unwrap(),
            ("tools.real".to_string(), vec!["a".to_string()])
        );
    }

    #[test]
    fn resolve_def_chain_cycle_guard_does_not_hang() {
        // mod_a imports X from mod_b; mod_b imports X from mod_a. Neither
        // declares X. Must terminate with None, not loop forever.
        let enums = EnumTable::default();
        let mut all_imports: HashMap<String, ImportTable> = HashMap::new();
        all_imports.insert(
            "tools.mod_a".to_string(),
            ImportTable::from_module(&parse("from tools.mod_b import X\n"), "tools.mod_a", false),
        );
        all_imports.insert(
            "tools.mod_b".to_string(),
            ImportTable::from_module(&parse("from tools.mod_a import X\n"), "tools.mod_b", false),
        );
        assert!(enums.lookup("X", "tools.mod_a", &all_imports).is_none());
    }

    #[test]
    fn arg_section_collects_module_bindings() {
        let src = r#"
LOGGING = arg_section("Logging Options", description="Control verbosity.")
"#;
        let table = ArgSectionTable::from_module(&parse(src));
        let entry = table.lookup("LOGGING").unwrap();
        assert_eq!(entry.title, "Logging Options");
        assert_eq!(entry.description.as_deref(), Some("Control verbosity."));
    }

    #[test]
    fn arg_section_without_description_is_none() {
        let src = r#"NETWORK = arg_section("Network Options")"#;
        let table = ArgSectionTable::from_module(&parse(src));
        let entry = table.lookup("NETWORK").unwrap();
        assert_eq!(entry.title, "Network Options");
        assert!(entry.description.is_none());
    }

    #[test]
    fn arg_section_ignores_non_arg_section_calls() {
        let src = r#"FOO = something_else("title")"#;
        let table = ArgSectionTable::from_module(&parse(src));
        assert!(table.lookup("FOO").is_none());
    }
}
