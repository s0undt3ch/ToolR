//! Built-in argparse scanner: AST-walks Python files declared in
//! `[tool.toolr.argparse.*]` and grafts their `parser.add_argument`
//! calls as manifest children of user-declared dispatcher commands.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::manifest::Command;

pub mod attach;
pub mod config;
pub mod scan;

pub use config::{ArgparseBlock, Attachment, parse_blocks, parse_blocks_from_pyproject};

#[derive(Debug, Error)]
pub enum ArgparseError {
    #[error(transparent)]
    Config(#[from] config::ConfigError),
    #[error(transparent)]
    Scan(#[from] scan::ScanError),
    #[error(transparent)]
    Attach(#[from] attach::AttachError),
}

/// Outcome of running the argparse pipeline for a project.
#[derive(Debug, Clone, Default)]
pub struct GraftResult {
    /// `{parent_dotted_name -> [grafted child Command]}`.
    pub children_by_parent: HashMap<String, Vec<Command>>,
    /// Dotted names of parents that received at least one grafted
    /// child. The caller flips `is_dispatcher = true` on each.
    pub dispatchers: HashSet<String>,
}

/// `tools/pyproject.toml` under `project_root`: where the argparse blocks
/// live. The static hash reads the same file.
pub fn pyproject_path(project_root: &Path) -> PathBuf {
    project_root.join("tools").join("pyproject.toml")
}

/// Run the full argparse pipeline for a project: read pyproject,
/// scan files, validate attachments, graft children, detect
/// collisions. Returns a [`GraftResult`] mapping each parent dotted
/// name to its grafted children and recording which parents received
/// any children (so the caller can flip `is_dispatcher`).
///
/// `parents` maps every potential dispatcher's dotted name to
/// `(module, function)`. The static parser populates this from the
/// freshly-walked registry before calling `run_for_project`.
///
/// Returns an empty [`GraftResult`] when no `tools/pyproject.toml`
/// exists or when it has no `[tool.toolr.argparse.*]` blocks.
pub fn run_for_project(
    project_root: &Path,
    parents: &HashMap<String, (String, String)>,
) -> Result<GraftResult, ArgparseError> {
    let pyproject = pyproject_path(project_root);
    if !pyproject.exists() {
        return Ok(GraftResult::default());
    }
    let blocks = config::parse_blocks_from_pyproject(&pyproject)?;
    if blocks.is_empty() {
        return Ok(GraftResult::default());
    }
    attach::validate_attachments(&blocks, parents)?;

    let mut out: HashMap<String, Vec<Command>> = HashMap::new();
    let mut dispatchers: HashSet<String> = HashSet::new();
    for block in &blocks {
        let scanned: Vec<scan::ScannedCommand> =
            scan::scan_block_paths(project_root, &block.scan_paths, block.django)?
                .into_iter()
                .map(|s| scan::with_common_args(s, &block.common_args))
                .collect();
        for (parent, children) in attach::graft_children(block, &scanned, parents)? {
            if !children.is_empty() {
                dispatchers.insert(parent.clone());
            }
            out.entry(parent).or_default().extend(children);
        }
    }
    // Intra-source duplicates (e.g. multiple Django apps each shipping a
    // `sync.py` swept up by the same glob) get a warning and the first
    // occurrence wins. Cross-source collisions remain a hard error.
    let warnings = attach::dedup_intra_source(&mut out);
    for w in &warnings {
        eprintln!("toolr: warning: {w}");
    }
    // Recompute `dispatchers` to drop any parent whose only child was a
    // dedup victim (rare in practice but keeps the bookkeeping honest).
    dispatchers.retain(|p| {
        out.get(p)
            .map(|children| !children.is_empty())
            .unwrap_or(false)
    });
    attach::validate_no_collisions(&out)?;
    Ok(GraftResult {
        children_by_parent: out,
        dispatchers,
    })
}

#[cfg(test)]
mod tests;
