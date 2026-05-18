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

/// Mapping of local class name → enum members, for classes that look
/// like an `Enum` subclass. Tracks both the member name (`ADD`) and
/// its serialised value (`"add"`) so we can resolve attribute-style
/// defaults like `Operation.ADD` to the CLI-visible value.
#[derive(Debug, Default, Clone)]
pub struct EnumTable {
    members: HashMap<String, Vec<EnumMember>>,
}

impl EnumTable {
    pub fn from_module(module: &ModModule) -> Self {
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
                table.members.insert(class.name.to_string(), members);
            }
        }
        table
    }

    /// List of serialised values for `class`. Used for `allowed_values`.
    pub fn lookup(&self, class: &str) -> Option<Vec<String>> {
        self.members
            .get(class)
            .map(|m| m.iter().map(|em| em.value.clone()).collect())
    }

    /// Resolve `class.member` to its serialised value. Used when
    /// rendering enum-attribute defaults in `--help`.
    pub fn lookup_member(&self, class: &str, member: &str) -> Option<&str> {
        self.members
            .get(class)?
            .iter()
            .find(|em| em.name == member)
            .map(|em| em.value.as_str())
    }

    pub fn merge(&mut self, other: EnumTable) {
        self.members.extend(other.members);
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
        let table = EnumTable::from_module(&parse(src));
        let vals = table.lookup("Mode").unwrap();
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
        let table = EnumTable::from_module(&parse(src));
        assert_eq!(table.lookup_member("Operation", "ADD"), Some("add"));
        assert_eq!(table.lookup_member("Operation", "SUBTRACT"), Some("subtract"));
        assert_eq!(table.lookup_member("Operation", "MISSING"), None);
        assert_eq!(table.lookup_member("OtherClass", "ADD"), None);
    }

    #[test]
    fn int_enum_member_falls_back_to_name() {
        let src = r#"
from enum import IntEnum

class Code(IntEnum):
    OK = 0
    ERROR = 1
"#;
        let table = EnumTable::from_module(&parse(src));
        // No string value, so we record the member's own name.
        assert_eq!(table.lookup_member("Code", "OK"), Some("OK"));
        assert_eq!(table.lookup_member("Code", "ERROR"), Some("ERROR"));
    }

    #[test]
    fn ignores_non_enum_classes() {
        let src = r#"
class Foo:
    X = "x"
"#;
        let table = EnumTable::from_module(&parse(src));
        assert!(table.lookup("Foo").is_none());
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
