//! Static AST parsing of `tools/**/*.py` files.

use std::path::Path;

use anyhow::{Context, Result};
use ruff_python_ast::ModModule;
use ruff_python_parser::parse_module;

pub mod groups;
pub use groups::{GroupBinding, extract_groups};

pub mod commands;
pub use commands::extract_commands;

/// Parse a single Python file and return its module AST.
pub fn parse_python_file(path: &Path) -> Result<ModModule> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let parsed = parse_module(&source)
        .with_context(|| format!("parsing {}", path.display()))?;
    Ok(parsed.into_syntax())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_tmp(source: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(source.as_bytes()).unwrap();
        f
    }

    #[test]
    fn parses_a_simple_module() {
        let f = write_tmp("x = 1\n");
        let module = parse_python_file(f.path()).expect("should parse");
        assert!(!module.body.is_empty());
    }

    #[test]
    fn returns_error_on_syntax_error() {
        let f = write_tmp("def broken(\n");
        let err = parse_python_file(f.path()).expect_err("should fail");
        assert!(err.to_string().contains("parsing"));
    }
}
