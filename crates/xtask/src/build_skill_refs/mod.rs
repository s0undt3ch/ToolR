//! `build-skill-refs` subcommand — regenerates the `references/*.md`
//! files under `skills/*/` from toolr's own source.
//!
//! Adding a new skill: implement a generator function returning a
//! [`Generated`] value and register it inside [`run`].

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

mod authoring;
mod ci_setup;
mod docs_section;
mod packaging;
mod sections;
mod self_contained;
mod types;

/// One regenerated file, ready to either write to disk or compare
/// against the committed version when `--check` is in effect.
pub struct Generated {
    /// Absolute path the body belongs to.
    pub path: PathBuf,
    /// Rendered markdown body, including the trailing newline. The
    /// generator guarantees byte-identical output across runs against
    /// the same source tree.
    pub body: String,
}

/// A Windows checkout hands sources over with CRLF; every generated body
/// and every lint result must match the LF one byte for byte.
pub(super) fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Read a source file as text with its newlines normalised.
pub(super) fn read_text(path: &Path) -> Result<String> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(normalize_newlines(&text))
}

/// Entry point invoked by `main`.
pub fn run(check: bool) -> Result<()> {
    let root = repo_root()?;

    // The registry. Each entry contributes one `references/*.md` file.
    // Order is presentational only — `apply` writes (or compares) each
    // entry independently.
    let mut outputs: Vec<Generated> = vec![
        authoring::commands(&root)?,
        authoring::testing_api(&root)?,
        authoring::testing_examples(&root)?,
        authoring::docstrings(&root)?,
        types::types_reference(&root)?,
        types::supported_types_snippet(&root)?,
        types::path_constraints_snippet(&root)?,
        packaging::packaging(&root)?,
        ci_setup::action(&root)?,
        sections::arguments_reference(&root)?,
        sections::external_sources_reference(&root)?,
        sections::packaging_example(&root)?,
    ];
    outputs.extend(sections::prek_hook_references(&root)?);

    apply(outputs, check)?;
    self_contained::lint(&root)
}

/// Either write each [`Generated`] to disk or, in `--check` mode,
/// collect the paths whose committed bodies do not match.
fn apply(outputs: Vec<Generated>, check: bool) -> Result<()> {
    let mut drift = Vec::new();
    for out in outputs {
        let current = std::fs::read_to_string(&out.path).ok();
        if current.as_deref() == Some(out.body.as_str()) {
            continue;
        }
        if check {
            drift.push(out.path);
        } else {
            if let Some(parent) = out.path.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("creating parent directory for {}", out.path.display())
                })?;
            }
            std::fs::write(&out.path, &out.body)
                .with_context(|| format!("writing {}", out.path.display()))?;
        }
    }

    if !drift.is_empty() {
        let listing = drift
            .iter()
            .map(|p| format!("  {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n");
        anyhow::bail!(
            "skill references are out of date — run `cargo xtask build-skill-refs`:\n{listing}",
        );
    }

    Ok(())
}

/// Resolve the workspace root (one above this crate's manifest dir).
///
/// `xtask` lives at `<repo>/crates/xtask`, so `repo_root` is
/// `manifest_dir/../..` — robust against the working directory the
/// alias is invoked from.
fn repo_root() -> Result<PathBuf> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let root = Path::new(manifest_dir)
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .context("crates/xtask is not nested two levels under the repo root")?;
    Ok(root)
}
