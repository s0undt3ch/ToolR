//! Static AST parsing of `tools/**/*.py` files.

use std::path::Path;

use anyhow::{Context, Result};
use ruff_python_ast::ModModule;
use ruff_python_parser::parse_module;

use crate::text::escape_control_chars;

/// Parse a raw docstring into the project's `Docstring` struct via the
/// shared `SimpleDocstringParser`. Returns `None` only if the parser
/// rejects the input (very unusual — in practice it accepts every
/// docstring shape). Used by both `@command` function docstrings and
/// `command_group(docstring=...)` so the "first paragraph → short,
/// remainder → long" contract is enforced in one place.
pub(crate) fn parse_docstring(raw: &str) -> Option<crate::Docstring> {
    crate::SimpleDocstringParser::new().parse(raw).ok()
}

pub mod groups;
pub use groups::{GroupBinding, extract_groups};

pub mod commands;
pub use commands::extract_commands;

pub mod signatures;
pub use signatures::extract_arguments;

pub mod symbols;
pub use symbols::{ArgSectionEntry, ArgSectionTable, EnumTable, TypeAliasTable};

pub mod types;
pub use types::{
    PathCompletion, SourcesImports, SupportedType, TypeImports, UnsupportedType,
    resolve as resolve_type,
};

pub mod names;

pub mod build;
pub use build::{
    BuildError, BuiltManifest, build_static_manifest, build_static_manifest_with_venv,
};

/// Parse a single Python file and return its module AST.
pub fn parse_python_file(path: &Path) -> Result<ModModule> {
    let source =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let parsed = parse_module(&source).map_err(|e| {
        let (line, column) = line_and_column(&source, usize::from(e.location.start()));
        // ruff's message quotes the offending token from the repo file, and both the stale-cache
        // warning and the fatal fresh-clone error print this text, so escape it here once.
        anyhow::anyhow!(escape_control_chars(&format!(
            "parsing {}: {} (line {line}, column {column})",
            path.display(),
            e.error
        )))
    })?;
    Ok(parsed.into_syntax())
}

/// The 1-based line and column (in characters) of byte `offset` in `source`; an offset past the
/// end or inside a character counts as the nearest earlier boundary.
fn line_and_column(source: &str, offset: usize) -> (usize, usize) {
    let mut end = offset.min(source.len());
    while !source.is_char_boundary(end) {
        end -= 1;
    }
    let before = &source[..end];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, column)
}

#[cfg(test)]
mod tests;
