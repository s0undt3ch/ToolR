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
    fn syntax_error_names_the_file_message_line_and_column() {
        let f = write_tmp("x = 1\ndef not closed(\n");
        let err = parse_python_file(f.path()).unwrap_err().to_string();
        let head = format!("parsing {}: ", f.path().display());
        let rest = err.strip_prefix(&head).expect(&err);
        assert!(rest.ends_with("(line 2, column 5)"), "{rest}");
        assert!(!rest.contains("byte range"), "{rest}");
    }

    #[test]
    fn syntax_error_text_has_no_raw_control_characters() {
        let f = write_tmp("\x1b]0;pwn\x07 = 1\n");
        let err = parse_python_file(f.path()).unwrap_err().to_string();
        assert!(!err.chars().any(char::is_control), "{err:?}");
        assert!(err.contains("\\u{1b}"), "{err}");
    }

    #[test]
    fn syntax_error_text_escapes_bidi_marks() {
        let f = write_tmp("a\u{202e}b = 1\n");
        let err = parse_python_file(f.path()).unwrap_err().to_string();
        assert!(!err.contains('\u{202e}'), "{err:?}");
        assert!(err.contains("\\u{202e}"), "{err}");
    }

    #[test]
    fn line_and_column_counts_characters_and_clamps() {
        assert_eq!(line_and_column("", 0), (1, 1));
        assert_eq!(line_and_column("ab\ncd", 4), (2, 2));
        assert_eq!(line_and_column("é=\n", 2), (1, 2));
        // Inside the two-byte `é`, and past the end.
        assert_eq!(line_and_column("é=", 1), (1, 1));
        assert_eq!(line_and_column("a\n", 99), (2, 1));
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
