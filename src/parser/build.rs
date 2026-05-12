//! Build a complete static `Manifest` from a `tools/` directory.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use walkdir::WalkDir;

use crate::hash::hash_tools_dir;
use crate::manifest::{Manifest, SCHEMA_VERSION};
use crate::parser::{
    commands::extract_commands, groups::extract_groups, parse_python_file, symbols::EnumTable,
};

/// Build the static portion of a manifest from a tools directory.
pub fn build_static_manifest(tools_dir: &Path) -> Result<Manifest> {
    let py_files = list_python_files(tools_dir);

    // Pass 1: build cross-file enum table from every module.
    let mut enums = EnumTable::default();
    for path in &py_files {
        let module = parse_python_file(path)?;
        enums.merge(EnumTable::from_module(&module));
    }

    // Pass 2: extract groups + commands per file using the merged table.
    let mut all_groups = Vec::new();
    let mut all_commands = Vec::new();
    let mut seen_groups = HashSet::new();
    for path in &py_files {
        let module = parse_python_file(path)?;
        let module_path = module_path_for(tools_dir, path);
        let module_doc = module_docstring(&module);
        let bindings = extract_groups(&module, &module_doc);
        let commands = extract_commands(&module, &module_path, &bindings, &enums);
        for binding in bindings {
            if seen_groups.insert(binding.group.name.clone()) {
                all_groups.push(binding.group);
            }
        }
        all_commands.extend(commands);
    }

    let static_hash = hash_tools_dir(tools_dir)?;
    Ok(Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash,
        dynamic_hash: String::new(),
        groups: all_groups,
        commands: all_commands,
    })
}

fn list_python_files(tools_dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = WalkDir::new(tools_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "py"))
        .map(|e| e.into_path())
        .collect();
    paths.sort();
    paths
}

fn module_path_for(tools_dir: &Path, file: &Path) -> String {
    let rel = file.strip_prefix(tools_dir).unwrap_or(file);
    let mut parts: Vec<String> = rel
        .with_extension("")
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.last().map(String::as_str) == Some("__init__") {
        parts.pop();
    }
    let mut out = String::from("tools");
    for p in parts {
        out.push('.');
        out.push_str(&p);
    }
    out
}

fn module_docstring(module: &ruff_python_ast::ModModule) -> String {
    use ruff_python_ast::Stmt;
    let Some(Stmt::Expr(e)) = module.body.first() else {
        return String::new();
    };
    if let ruff_python_ast::Expr::StringLiteral(s) = e.value.as_ref() {
        return s.value.to_str().to_string();
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(tmp: &Path, name: &str, contents: &str) {
        let path = tmp.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn builds_manifest_from_single_tools_file() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "tools/ci.py",
            r#""""CI utilities."""
group = command_group("ci", "CI utilities", docstring=__doc__)

@group.command
def hello(ctx):
    """Say hello."""
    pass
"#,
        );
        let m = build_static_manifest(&tmp.path().join("tools")).unwrap();
        assert_eq!(m.schema_version, SCHEMA_VERSION);
        assert_eq!(m.groups.len(), 1);
        assert_eq!(m.groups[0].name, "ci");
        assert_eq!(m.commands.len(), 1);
        assert_eq!(m.commands[0].name, "hello");
        assert!(!m.static_hash.is_empty());
    }
}
