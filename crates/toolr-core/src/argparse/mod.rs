//! Built-in argparse scanner: AST-walks Python files declared in
//! `[tool.toolr.argparse.*]` and grafts their `parser.add_argument`
//! calls as manifest children of user-declared dispatcher commands.

use std::collections::{HashMap, HashSet};
use std::path::Path;

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
    let pyproject = project_root.join("tools").join("pyproject.toml");
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
        let scanned: Vec<scan::ScannedCommand> = scan::scan_block_paths(project_root, &block.scan_paths)?
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
    attach::validate_no_collisions(&out)?;
    Ok(GraftResult {
        children_by_parent: out,
        dispatchers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn run_for_project_returns_grafted_children() {
        let project = tempfile::tempdir().unwrap();
        let tools = project.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        let cmds = project.path().join("apps/billing/management/commands");
        std::fs::create_dir_all(&cmds).unwrap();
        std::fs::write(
            cmds.join("sync.py"),
            "def add_arguments(self, parser):\n    parser.add_argument('--force', action='store_true')\n",
        )
        .unwrap();
        std::fs::write(
            tools.join("pyproject.toml"),
            r#"
                [tool.toolr.argparse.django]
                scan_paths = ["apps/*/management/commands/*.py"]

                [[tool.toolr.argparse.django.attach]]
                parent = "django"
            "#,
        )
        .unwrap();

        let mut parents = HashMap::new();
        parents.insert(
            "django".to_string(),
            ("tools.dispatcher".to_string(), "django".to_string()),
        );

        let result = run_for_project(project.path(), &parents).unwrap();
        let django_children = result.children_by_parent.get("django").unwrap();
        assert_eq!(django_children.len(), 1);
        assert_eq!(django_children[0].name, "sync");
        assert_eq!(
            django_children[0].dispatched_from.as_deref(),
            Some("argparse:django"),
        );

        // `django` received grafted children, so it's in `dispatchers`.
        assert!(result.dispatchers.contains("django"));
    }
}
