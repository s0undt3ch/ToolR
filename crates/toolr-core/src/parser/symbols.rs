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
                table.members.entry(class.name.to_string()).or_default().push(EnumDef {
                    module: module_path.to_string(),
                    members,
                });
            }
        }
        table
    }

    /// Pick the `class` definition that's actually visible from
    /// `current_module`: a same-module definition always wins. Failing
    /// that, an unambiguous single cross-module definition (the common
    /// "enum lives in a shared module, imported elsewhere" case) is
    /// used. When there are several different-module definitions but
    /// they all serialise to the same member set (e.g. the same
    /// `Environment(StrEnum)` shape copy-pasted or re-exported across a
    /// few modules), the static parser can't tell which one was
    /// actually imported here, but it doesn't matter — any of them
    /// produces the same `allowed_values` / member lookups, so this
    /// resolves rather than rejecting a legitimate import (GH #454).
    /// Only a genuine mismatch in member sets, with no local match, is
    /// ambiguous — that returns `None`, surfacing as an
    /// unsupported-type error instead of a wrong result.
    fn resolve_def(&self, class: &str, current_module: &str) -> Option<&[EnumMember]> {
        let defs = self.members.get(class)?;
        if let Some(d) = defs.iter().find(|d| d.module == current_module) {
            return Some(&d.members);
        }
        match defs.as_slice() {
            [only] => Some(&only.members),
            [first, rest @ ..]
                if rest
                    .iter()
                    .all(|d| same_members(&d.members, &first.members)) =>
            {
                Some(&first.members)
            }
            _ => None,
        }
    }

    /// List of serialised values for `class`. Used for `allowed_values`.
    pub fn lookup(&self, class: &str, current_module: &str) -> Option<Vec<String>> {
        self.resolve_def(class, current_module)
            .map(|m| m.iter().map(|em| em.value.clone()).collect())
    }

    /// Resolve `class.member` to its serialised value. Used when
    /// rendering enum-attribute defaults in `--help`.
    pub fn lookup_member(&self, class: &str, member: &str, current_module: &str) -> Option<&str> {
        self.resolve_def(class, current_module)?
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
    fn collects_string_enum_values() {
        let src = r#"
from enum import StrEnum

class Mode(StrEnum):
    FAST = "fast"
    SLOW = "slow"
"#;
        let table = EnumTable::from_module(&parse(src), "tools.example");
        let vals = table.lookup("Mode", "tools.example").unwrap();
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
        assert_eq!(table.lookup_member("Operation", "ADD", "tools.example"), Some("add"));
        assert_eq!(
            table.lookup_member("Operation", "SUBTRACT", "tools.example"),
            Some("subtract")
        );
        assert_eq!(table.lookup_member("Operation", "MISSING", "tools.example"), None);
        assert_eq!(table.lookup_member("OtherClass", "ADD", "tools.example"), None);
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
        assert_eq!(table.lookup_member("Code", "OK", "tools.example"), Some("OK"));
        assert_eq!(table.lookup_member("Code", "ERROR", "tools.example"), Some("ERROR"));
    }

    #[test]
    fn ignores_non_enum_classes() {
        let src = r#"
class Foo:
    X = "x"
"#;
        let table = EnumTable::from_module(&parse(src), "tools.example");
        assert!(table.lookup("Foo", "tools.example").is_none());
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
            table.lookup("Database", "tools.module_a").unwrap(),
            vec!["primary".to_string()]
        );
        assert_eq!(
            table.lookup("Database", "tools.module_b").unwrap(),
            vec!["replica".to_string()]
        );
        // Neither module's definition wins from an unrelated module —
        // the collision is genuinely ambiguous, so this resolves to
        // nothing rather than silently picking one (GH #449).
        assert!(table.lookup("Database", "tools.module_c").is_none());
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
            table.lookup("Mode", "tools.module_b").unwrap(),
            vec!["fast".to_string()]
        );
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
