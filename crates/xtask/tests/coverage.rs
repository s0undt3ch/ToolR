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

#[test]
fn references_action_md_covers_every_input_and_output() {
    let workspace = workspace_root();
    let action_yml = workspace.join("action.yml");
    let reference =
        workspace.join("skills/toolr-ci-setup/references/action.md");

    let yaml_source = fs::read_to_string(&action_yml)
        .unwrap_or_else(|e| panic!("reading {}: {e}", action_yml.display()));
    let reference_body = fs::read_to_string(&reference)
        .unwrap_or_else(|e| panic!("reading {}: {e}", reference.display()));

    let declared_inputs = extract_top_level_keys_under(&yaml_source, "inputs");
    let declared_outputs = extract_top_level_keys_under(&yaml_source, "outputs");

    for name in &declared_inputs {
        let needle = format!("| `{name}` |");
        assert!(
            reference_body.contains(&needle),
            "input '{name}' declared in action.yml but missing from references/action.md",
        );
    }
    for name in &declared_outputs {
        let needle = format!("| `{name}` |");
        assert!(
            reference_body.contains(&needle),
            "output '{name}' declared in action.yml but missing from references/action.md",
        );
    }

    let documented = extract_backticked_names_in_tables(&reference_body);
    let declared: BTreeSet<&String> = declared_inputs
        .iter()
        .chain(declared_outputs.iter())
        .collect();
    let documented_set: BTreeSet<&String> = documented.iter().collect();
    let extra: Vec<&&String> = documented_set.difference(&declared).collect();
    assert!(
        extra.is_empty(),
        "names documented in references/action.md but not in action.yml: {extra:?}\n\
         Regenerate with `cargo xtask build-skill-refs` after editing action.yml.",
    );
}

// Lift the top-level keys under a YAML section like `inputs:` or
// `outputs:`. Avoids a full YAML parse in the test crate; the
// generator already parses YAML, this is just a check.
fn extract_top_level_keys_under(source: &str, section: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut in_section = false;
    let header = format!("{section}:");
    for line in source.lines() {
        if line.trim_end() == header {
            in_section = true;
            continue;
        }
        if in_section {
            // A non-indented non-empty line ends the section.
            if !line.starts_with(char::is_whitespace) && !line.is_empty() {
                in_section = false;
                continue;
            }
            // Indented `<name>:` with exactly two leading spaces is a
            // key declaration; deeper indents are nested fields.
            if let Some(rest) = line.strip_prefix("  ") {
                if !rest.starts_with(char::is_whitespace) {
                    if let Some(key) = rest.strip_suffix(':') {
                        keys.push(key.to_string());
                    }
                }
            }
        }
    }
    keys
}

// Pull every `name` token that appears in the leftmost column of
// a markdown table row. The renderer uses the form
// `| \`<name>\` | ... |` for every input/output row.
fn extract_backticked_names_in_tables(body: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("| `") {
            if let Some(end) = rest.find('`') {
                names.push(rest[..end].to_string());
            }
        }
    }
    names
}
