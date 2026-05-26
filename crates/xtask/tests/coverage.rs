//! Bidirectional public-surface coverage guard for the authoring
//! skill's `references/commands.md`.
//!
//! Every name in `toolr.__all__` must produce a `### \`<Name>\``
//! section in the generated reference, and the reference must not
//! contain any section that is not in `toolr.__all__`. Together these
//! catch two failure modes:
//!
//! - A name added to `__all__` that the AST walker can't resolve — the
//!   generator would skip it silently, leaving downstream agents with
//!   an incomplete reference.
//! - A name removed from `__all__` while the generated file still
//!   carries a section for it (e.g. someone hand-edited the reference,
//!   defeating the drift-defense contract).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn references_commands_md_covers_all_of_toolr_dunder_all() {
    let workspace = workspace_root();
    let init = workspace.join("crates/toolr-py/python/toolr/__init__.py");
    let reference = workspace.join("skills/toolr-command-authoring/references/commands.md");

    let init_source = fs::read_to_string(&init)
        .unwrap_or_else(|e| panic!("reading {}: {e}", init.display()));
    let reference_body = fs::read_to_string(&reference)
        .unwrap_or_else(|e| panic!("reading {}: {e}", reference.display()));

    let declared: BTreeSet<String> = extract_dunder_all(&init_source)
        .unwrap_or_else(|| panic!("could not parse __all__ from {}", init.display()))
        .into_iter()
        .collect();
    let documented: BTreeSet<String> = extract_section_names(&reference_body)
        .into_iter()
        .collect();

    let missing: Vec<&String> = declared.difference(&documented).collect();
    let extra: Vec<&String> = documented.difference(&declared).collect();

    let mut errors = Vec::new();
    if !missing.is_empty() {
        errors.push(format!(
            "names in toolr.__all__ but missing from references/commands.md: {missing:?}",
        ));
    }
    if !extra.is_empty() {
        errors.push(format!(
            "names documented in references/commands.md but not in toolr.__all__: {extra:?}",
        ));
    }
    assert!(
        errors.is_empty(),
        "public-surface coverage violation:\n  {}\n\n\
         Regenerate with `cargo xtask build-skill-refs` after editing toolr.__all__.",
        errors.join("\n  "),
    );
}

/// Tiny single-purpose extractor for `__all__ = [...]`. Avoids pulling
/// the full Python AST machinery into the test crate — the contract
/// is "the source-of-truth `__all__` lives at the package root and is
/// a list of string literals", which a regex-style scan handles fine
/// for our own source.
fn extract_dunder_all(source: &str) -> Option<Vec<String>> {
    let start = source.find("__all__")?;
    let lbracket = source[start..].find('[')?;
    let rbracket = source[start + lbracket..].find(']')?;
    let body = &source[start + lbracket + 1..start + lbracket + rbracket];
    let mut names = Vec::new();
    for raw in body.split(',') {
        let token = raw.trim().trim_matches(|c: char| c.is_whitespace() || c == ',');
        if token.is_empty() {
            continue;
        }
        let unquoted = token.trim_matches(|c| c == '"' || c == '\'');
        if !unquoted.is_empty() {
            names.push(unquoted.to_string());
        }
    }
    Some(names)
}

fn extract_section_names(reference: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in reference.lines() {
        if let Some(rest) = line.strip_prefix("### `") {
            if let Some(end) = rest.find('`') {
                names.push(rest[..end].to_string());
            }
        }
    }
    names
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .expect("workspace root two levels above CARGO_MANIFEST_DIR")
}
