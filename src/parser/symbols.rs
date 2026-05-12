//! Symbol table for resolving local type names to their declarations.

use std::collections::HashMap;

use ruff_python_ast::{Expr, ModModule, Stmt, StmtClassDef};

/// Mapping of local class name → enum member values, for classes that
/// look like an `Enum` subclass.
#[derive(Debug, Default, Clone)]
pub struct EnumTable {
    members: HashMap<String, Vec<String>>,
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
            let values = class
                .body
                .iter()
                .filter_map(member_value)
                .collect::<Vec<_>>();
            if !values.is_empty() {
                table.members.insert(class.name.to_string(), values);
            }
        }
        table
    }

    pub fn lookup(&self, name: &str) -> Option<&[String]> {
        self.members.get(name).map(|v| v.as_slice())
    }

    pub fn merge(&mut self, other: EnumTable) {
        self.members.extend(other.members);
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

fn member_value(stmt: &Stmt) -> Option<String> {
    let Stmt::Assign(a) = stmt else {
        return None;
    };
    let Expr::StringLiteral(s) = a.value.as_ref() else {
        // For non-string values, fall back to recording the member NAME
        // so callers at least know the variants exist. Richer extraction
        // is future work.
        if a.targets.len() == 1 {
            if let Expr::Name(t) = &a.targets[0] {
                return Some(t.id.as_str().to_string());
            }
        }
        return None;
    };
    Some(s.value.to_str().to_string())
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
        assert_eq!(vals, &["fast".to_string(), "slow".to_string()]);
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
}
